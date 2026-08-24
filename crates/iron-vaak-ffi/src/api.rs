//! 既存`EmbeddingRunner`だけを使う、safeなopaque-handle API中核。

use crate::abi::{
    diagnostic_origin, status, value_tag, AbiInfoV0, CallEnvelopeV0, DiagnosticEnvelopeV0,
    HostLayoutEntryV0, PreparedHandleV0, RunnerHandleV0, MAX_NAME_BYTES, MAX_RECORDS,
    MAX_SOURCE_BYTES,
};
use crate::codec::{
    decode_snapshot, encode_patch, CodecError, CodecErrorKind, PatchBatchV0, PatchEntryV0,
    SnapshotBatchV0, WireValueV0,
};
use std::cell::{RefCell, RefMut};
use std::collections::{HashMap, HashSet};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;
use vaak::ast::{HostItem, ValueType};
use vaak::embedding::{
    prepare, EmbeddingRunner, HostLayout, PrepareStage, PreparedProgram, PreparedRunError,
};
use vaak::interp::Eval;
use vaak::value::Value;

const PREPARED_KIND: u8 = 1;
const RUNNER_KIND: u8 = 2;
const MAX_GENERATION: u32 = 0x00ff_ffff;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticV0 {
    pub envelope: DiagnosticEnvelopeV0,
    pub message_utf8: Vec<u8>,
}

impl DiagnosticV0 {
    fn error(origin: u32, span_start: u64, span_length: u64, message: impl Into<String>) -> Self {
        let message_utf8 = message.into().into_bytes();
        Self {
            envelope: DiagnosticEnvelopeV0::error(
                origin,
                span_start,
                span_length,
                message_utf8.len() as u64,
            ),
            message_utf8,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ApiResult<T> {
    pub envelope: CallEnvelopeV0,
    pub value: Option<T>,
    pub diagnostic: Option<DiagnosticV0>,
}

impl<T> ApiResult<T> {
    fn ok(value: T) -> Self {
        Self {
            envelope: CallEnvelopeV0::default(),
            value: Some(value),
            diagnostic: None,
        }
    }

    fn transport_error(status_code: u32, message: impl Into<String>) -> Self {
        Self {
            envelope: CallEnvelopeV0::with_status(status_code),
            value: None,
            diagnostic: Some(DiagnosticV0::error(
                diagnostic_origin::FFI_DECODE,
                0,
                0,
                message,
            )),
        }
    }

    fn internal_panic() -> Self {
        Self {
            envelope: CallEnvelopeV0::with_status(status::INTERNAL_PANIC),
            value: None,
            diagnostic: Some(internal_panic_diagnostic()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrepareReplyV0 {
    pub prepared: Option<PreparedHandleV0>,
    pub diagnostics: Vec<DiagnosticV0>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgramStatusV0 {
    Completed,
    ProgramError,
    HostContractError,
    InternalPanic,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TopLevelOutcomeV0 {
    Akasha,
    Value(WireValueV0),
    Paradox { span_start: u64, span_length: u64 },
    Escape,
    UnsupportedAggregate,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RunnerReportV0 {
    pub program_status: ProgramStatusV0,
    pub top_level: Option<TopLevelOutcomeV0>,
    /// hostへまだapplyしていない一transaction候補。
    pub patch_wire: Vec<u8>,
    pub diagnostics: Vec<DiagnosticV0>,
}

/// raw pointerを扱うC shimより内側の、safeなnative API。
///
/// `Rc`/`RefCell`を意図的に用い、v0 checkpointではcross-thread共有を公開保証しない。
/// managed facadeは一つのinstanceを専用serial executorへ束縛する。
pub struct NativeApi {
    registry: RefCell<Registry>,
}

impl Default for NativeApi {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeApi {
    pub fn new() -> Self {
        Self {
            registry: RefCell::new(Registry::default()),
        }
    }

    pub fn abi_info(&self) -> AbiInfoV0 {
        AbiInfoV0::default()
    }

    /// UTF-8 sourceと固定HostLayoutを一度だけprepareする。
    pub fn prepare(
        &self,
        source_utf8: &[u8],
        entries: &[HostLayoutEntryV0],
        name_blob_utf8: &[u8],
    ) -> ApiResult<PrepareReplyV0> {
        boundary(|| self.prepare_inner(source_utf8, entries, name_blob_utf8))
    }

    /// zero/staleを含む同じprepared tokenの反復destroyはno-opである。
    pub fn prepared_destroy(&self, handle: PreparedHandleV0) -> ApiResult<()> {
        boundary(|| {
            if handle == PreparedHandleV0::INVALID {
                return ApiResult::ok(());
            }
            if handle_kind(handle.0) != PREPARED_KIND {
                return ApiResult::transport_error(
                    status::STALE_HANDLE,
                    "prepared handleのkindが一致しない",
                );
            }
            self.registry
                .borrow_mut()
                .prepared
                .remove_idempotent(handle.0, PREPARED_KIND);
            ApiResult::ok(())
        })
    }

    /// runnerはpreparedへのstrong referenceを保持する。
    pub fn runner_new(&self, prepared: PreparedHandleV0) -> ApiResult<RunnerHandleV0> {
        boundary(|| {
            let prepared = {
                let registry = self.registry.borrow();
                match registry.prepared.get(prepared.0, PREPARED_KIND) {
                    Some(prepared) => Rc::clone(prepared),
                    None => {
                        return ApiResult::transport_error(
                            status::STALE_HANDLE,
                            "prepared handleがstaleまたは不正である",
                        )
                    }
                }
            };
            let runner = Rc::new(RefCell::new(RunnerRecord {
                prepared,
                runner: EmbeddingRunner::new(),
                state: RunnerState::Idle,
                report: None,
            }));
            let raw = self
                .registry
                .borrow_mut()
                .runners
                .insert(runner, RUNNER_KIND);
            ApiResult::ok(RunnerHandleV0(raw))
        })
    }

    /// zero/staleの反復destroyはno-op。実行中だけは`BUSY`を返す。
    pub fn runner_destroy(&self, handle: RunnerHandleV0) -> ApiResult<()> {
        boundary(|| {
            if handle == RunnerHandleV0::INVALID {
                return ApiResult::ok(());
            }
            if handle_kind(handle.0) != RUNNER_KIND {
                return ApiResult::transport_error(
                    status::STALE_HANDLE,
                    "runner handleのkindが一致しない",
                );
            }
            let runner = {
                let registry = self.registry.borrow();
                registry.runners.get(handle.0, RUNNER_KIND).cloned()
            };
            let Some(runner) = runner else {
                return ApiResult::ok(());
            };
            let guard = match runner.try_borrow_mut() {
                Ok(guard) => guard,
                Err(_) => {
                    return ApiResult::transport_error(
                        status::BUSY,
                        "実行中のrunnerはdestroyできない",
                    )
                }
            };
            drop(guard);
            self.registry
                .borrow_mut()
                .runners
                .remove_idempotent(handle.0, RUNNER_KIND);
            ApiResult::ok(())
        })
    }

    /// snapshotを一括decodeし、同じ`EmbeddingRunner`を再利用して一回走らせる。
    ///
    /// runtime error時もC-2/S-22のafter-stateをPatchへ保持するが、このcrateは
    /// Patchをhostへapplyしない。
    pub fn runner_run(
        &self,
        handle: RunnerHandleV0,
        snapshot_wire: &[u8],
        run_id: [u8; 16],
        transaction_id: [u8; 16],
    ) -> ApiResult<()> {
        boundary(|| self.runner_run_inner(handle, snapshot_wire, run_id, transaction_id))
    }

    /// native-owned reportのsafe copy。実C shimではall-or-zero copy APIへ写す。
    pub fn runner_report(&self, handle: RunnerHandleV0) -> ApiResult<RunnerReportV0> {
        boundary(|| {
            let runner = match self.runner_cell(handle) {
                Ok(runner) => runner,
                Err(error) => return error.into_result(),
            };
            let runner = match runner.try_borrow() {
                Ok(runner) => runner,
                Err(_) => {
                    return ApiResult::transport_error(
                        status::BUSY,
                        "実行中のrunnerからreportをcopyできない",
                    )
                }
            };
            match &runner.report {
                Some(report) => ApiResult::ok(report.clone()),
                None if runner.state == RunnerState::Poisoned => ApiResult::transport_error(
                    status::POISONED,
                    "runnerはpanic後にpoisonされ、reportが無い",
                ),
                None => ApiResult::transport_error(
                    status::INVALID_ARGUMENT,
                    "runnerにはcopyできるreportが無い",
                ),
            }
        })
    }

    pub fn runner_clear_report(&self, handle: RunnerHandleV0) -> ApiResult<()> {
        boundary(|| {
            let runner = match self.runner_cell(handle) {
                Ok(runner) => runner,
                Err(error) => return error.into_result(),
            };
            let mut runner = match runner.try_borrow_mut() {
                Ok(runner) => runner,
                Err(_) => {
                    return ApiResult::transport_error(
                        status::BUSY,
                        "実行中のrunner reportはclearできない",
                    )
                }
            };
            if runner.state == RunnerState::Poisoned {
                return ApiResult::transport_error(
                    status::POISONED,
                    "poisoned runnerはreport copyとdestroy以外に使えない",
                );
            }
            runner.report = None;
            runner.state = RunnerState::Idle;
            ApiResult::ok(())
        })
    }

    fn prepare_inner(
        &self,
        source_utf8: &[u8],
        entries: &[HostLayoutEntryV0],
        name_blob_utf8: &[u8],
    ) -> ApiResult<PrepareReplyV0> {
        if source_utf8.len() as u64 > MAX_SOURCE_BYTES {
            return ApiResult::transport_error(
                status::LIMIT_EXCEEDED,
                "source byte数が上限を越える",
            );
        }
        if name_blob_utf8.len() as u64 > MAX_NAME_BYTES {
            return ApiResult::transport_error(
                status::LIMIT_EXCEEDED,
                "HostLayout name blobが上限を越える",
            );
        }
        if entries.len() > MAX_RECORDS as usize {
            return ApiResult::transport_error(
                status::LIMIT_EXCEEDED,
                "HostLayout slot数が上限を越える",
            );
        }
        let source = match std::str::from_utf8(source_utf8) {
            Ok(source) => source,
            Err(_) => {
                return ApiResult::transport_error(
                    status::MALFORMED_WIRE,
                    "sourceが正しいUTF-8でない",
                )
            }
        };
        if std::str::from_utf8(name_blob_utf8).is_err() {
            return ApiResult::transport_error(
                status::MALFORMED_WIRE,
                "HostLayout name blobが正しいUTF-8でない",
            );
        }
        let bindings = match decode_layout(entries, name_blob_utf8) {
            Ok(bindings) => bindings,
            Err(error) => return error.into_result(),
        };
        let layout_entries = bindings
            .iter()
            .map(|binding| {
                (
                    binding.name.clone(),
                    HostItem::Value(value_type(binding.wire_type)),
                )
            })
            .collect();
        let layout = match HostLayout::new(layout_entries) {
            Ok(layout) => layout,
            Err(error) => {
                return ApiResult::transport_error(status::INVALID_ARGUMENT, error.to_string())
            }
        };
        match prepare(source, &layout) {
            Ok(program) => {
                let prepared = Rc::new(PreparedRecord { program, bindings });
                let raw = self
                    .registry
                    .borrow_mut()
                    .prepared
                    .insert(prepared, PREPARED_KIND);
                ApiResult::ok(PrepareReplyV0 {
                    prepared: Some(PreparedHandleV0(raw)),
                    diagnostics: Vec::new(),
                })
            }
            Err(error) => {
                let diagnostics = error
                    .diagnostics()
                    .iter()
                    .map(|diagnostic| {
                        let origin = match diagnostic.stage {
                            PrepareStage::Parse => diagnostic_origin::PREPARE_PARSE,
                            PrepareStage::Check => diagnostic_origin::PREPARE_CHECK,
                            PrepareStage::TypeCheck => diagnostic_origin::PREPARE_TYPE_CHECK,
                            PrepareStage::Compile => diagnostic_origin::PREPARE_COMPILE,
                        };
                        DiagnosticV0::error(
                            origin,
                            diagnostic.span.start as u64,
                            diagnostic.span.end.saturating_sub(diagnostic.span.start) as u64,
                            diagnostic.message.clone(),
                        )
                    })
                    .collect();
                ApiResult::ok(PrepareReplyV0 {
                    prepared: None,
                    diagnostics,
                })
            }
        }
    }

    fn runner_run_inner(
        &self,
        handle: RunnerHandleV0,
        snapshot_wire: &[u8],
        run_id: [u8; 16],
        transaction_id: [u8; 16],
    ) -> ApiResult<()> {
        let runner = match self.runner_cell(handle) {
            Ok(runner) => runner,
            Err(error) => return error.into_result(),
        };
        let mut runner = match runner.try_borrow_mut() {
            Ok(runner) => runner,
            Err(_) => {
                return ApiResult::transport_error(
                    status::BUSY,
                    "同じrunnerへの同時runまたは再入はできない",
                )
            }
        };
        if runner.state == RunnerState::Poisoned {
            return ApiResult::transport_error(
                status::POISONED,
                "runnerは以前のpanicでpoisonされている",
            );
        }
        runner.state = RunnerState::Running;
        runner.report = None;
        let result = catch_unwind(AssertUnwindSafe(|| {
            execute_run(&mut runner, snapshot_wire, run_id, transaction_id)
        }));
        match result {
            Ok(Ok(report)) => {
                runner.report = Some(report);
                runner.state = RunnerState::ReportReady;
                ApiResult::ok(())
            }
            Ok(Err(error)) => {
                runner.state = RunnerState::Idle;
                error.into_result()
            }
            Err(_) => {
                runner.report = Some(RunnerReportV0 {
                    program_status: ProgramStatusV0::InternalPanic,
                    top_level: None,
                    patch_wire: Vec::new(),
                    diagnostics: vec![internal_panic_diagnostic()],
                });
                runner.state = RunnerState::Poisoned;
                ApiResult::internal_panic()
            }
        }
    }

    fn runner_cell(&self, handle: RunnerHandleV0) -> Result<Rc<RefCell<RunnerRecord>>, ApiFailure> {
        let runner = {
            let registry = self.registry.borrow();
            registry.runners.get(handle.0, RUNNER_KIND).cloned()
        };
        runner.ok_or_else(|| {
            ApiFailure::new(status::STALE_HANDLE, "runner handleがstaleまたは不正である")
        })
    }

    #[cfg(test)]
    fn runner_cell_for_test(&self, handle: RunnerHandleV0) -> Rc<RefCell<RunnerRecord>> {
        self.registry
            .borrow()
            .runners
            .get(handle.0, RUNNER_KIND)
            .cloned()
            .expect("runner")
    }

    #[cfg(test)]
    fn panic_runner_for_test(&self, handle: RunnerHandleV0) -> ApiResult<()> {
        boundary(|| {
            let runner = match self.runner_cell(handle) {
                Ok(runner) => runner,
                Err(error) => return error.into_result(),
            };
            let mut runner = runner.borrow_mut();
            runner.state = RunnerState::Running;
            let caught = catch_unwind(AssertUnwindSafe(|| panic!("test panic")));
            assert!(caught.is_err());
            runner.report = Some(RunnerReportV0 {
                program_status: ProgramStatusV0::InternalPanic,
                top_level: None,
                patch_wire: Vec::new(),
                diagnostics: vec![internal_panic_diagnostic()],
            });
            runner.state = RunnerState::Poisoned;
            ApiResult::internal_panic()
        })
    }
}

fn execute_run(
    runner: &mut RefMut<'_, RunnerRecord>,
    snapshot_wire: &[u8],
    run_id: [u8; 16],
    transaction_id: [u8; 16],
) -> Result<RunnerReportV0, ApiFailure> {
    let snapshot = decode_snapshot(snapshot_wire).map_err(codec_failure)?;
    let snapshot_values = snapshot_values(&runner.prepared, &snapshot)
        .map_err(|message| ApiFailure::new(status::MALFORMED_WIRE, message))?;
    let mut host_values = runner
        .prepared
        .program
        .host_values(snapshot_values.values)
        .map_err(|error| ApiFailure::new(status::MALFORMED_WIRE, error.to_string()))?;
    let prepared = Rc::clone(&runner.prepared);
    let run_result = runner
        .runner
        .run_values_without_functions(&prepared.program, &mut host_values)
        .map_err(prepared_run_failure)?;
    let after_values = host_values.into_values();
    let mut diagnostics = Vec::new();
    let (program_status, top_level) = match run_result {
        Ok(eval) => (ProgramStatusV0::Completed, Some(eval_outcome(eval))),
        Err(error) => {
            diagnostics.push(DiagnosticV0::error(
                diagnostic_origin::VAAK_RUNTIME,
                error.span.start as u64,
                error.span.end.saturating_sub(error.span.start) as u64,
                error.msg,
            ));
            (ProgramStatusV0::ProgramError, None)
        }
    };
    let mut patch_entries = Vec::new();
    for (index, ((value, binding), before_value)) in after_values
        .into_iter()
        .zip(&runner.prepared.bindings)
        .zip(snapshot_values.before)
        .enumerate()
    {
        let after = vaak_value_to_wire(value, binding.wire_type).map_err(|message| {
            ApiFailure::new(
                status::MALFORMED_WIRE,
                format!("run後host値{index}をwireへ戻せない: {message}"),
            )
        })?;
        if after != before_value {
            patch_entries.push(PatchEntryV0 {
                entity_id: binding.entity_id,
                property_id: binding.property_id,
                expected_property_revision: snapshot_values.revisions[index],
                capability_table_index: binding.capability_table_index,
                value: after,
            });
        }
    }
    patch_entries.sort_by_key(|entry| (entry.entity_id, entry.property_id));
    let patch = PatchBatchV0 {
        schema_id: snapshot.schema_id,
        session_id: snapshot.session_id,
        run_id,
        transaction_id,
        base_snapshot_revision: snapshot.snapshot_revision,
        entries: patch_entries,
    };
    let patch_wire = encode_patch(&patch).map_err(codec_failure)?;
    Ok(RunnerReportV0 {
        program_status,
        top_level,
        patch_wire,
        diagnostics,
    })
}

fn snapshot_values(
    prepared: &PreparedRecord,
    snapshot: &SnapshotBatchV0,
) -> Result<SnapshotValues, String> {
    if snapshot.entries.len() != prepared.bindings.len() {
        return Err(format!(
            "snapshotは{} property必要だが{} propertyだった",
            prepared.bindings.len(),
            snapshot.entries.len()
        ));
    }
    let by_key: HashMap<_, _> = snapshot
        .entries
        .iter()
        .map(|entry| ((entry.entity_id, entry.property_id), entry))
        .collect();
    let mut values = Vec::with_capacity(prepared.bindings.len());
    let mut before = Vec::with_capacity(prepared.bindings.len());
    let mut revisions = Vec::with_capacity(prepared.bindings.len());
    for binding in &prepared.bindings {
        let entry = by_key
            .get(&(binding.entity_id, binding.property_id))
            .ok_or_else(|| {
                format!(
                    "HostLayout property ({}, {}) がsnapshotに無い",
                    binding.entity_id, binding.property_id
                )
            })?;
        if entry.value.type_tag() != binding.wire_type {
            return Err(format!(
                "property ({}, {}) の型tagがHostLayoutと一致しない",
                binding.entity_id, binding.property_id
            ));
        }
        values.push(wire_to_vaak_value(&entry.value));
        before.push(entry.value.clone());
        revisions.push(entry.property_revision);
    }
    Ok(SnapshotValues {
        values,
        before,
        revisions,
    })
}

fn decode_layout(
    entries: &[HostLayoutEntryV0],
    name_blob: &[u8],
) -> Result<Vec<BindingSpec>, ApiFailure> {
    let mut bindings = Vec::with_capacity(entries.len());
    let mut properties = HashSet::with_capacity(entries.len());
    for (index, entry) in entries.iter().copied().enumerate() {
        if entry.struct_size as usize != std::mem::size_of::<HostLayoutEntryV0>()
            || entry.flags != 0
            || entry.reserved1 != 0
        {
            return Err(ApiFailure::new(
                status::ABI_MISMATCH,
                format!("HostLayout entry {index}のstruct size/flags/reservedが不正である"),
            ));
        }
        if entry.slot_index as usize != index {
            return Err(ApiFailure::new(
                status::MALFORMED_WIRE,
                "HostLayout slot_indexは0からのcanonical順でなければならない",
            ));
        }
        if !is_supported_value_tag(entry.value_type) {
            return Err(ApiFailure::new(
                status::MALFORMED_WIRE,
                format!("HostLayout entry {index}のvalue type tagが未対応である"),
            ));
        }
        if !properties.insert((entry.entity_id, entry.property_id)) {
            return Err(ApiFailure::new(
                status::MALFORMED_WIRE,
                "HostLayout property keyが重複している",
            ));
        }
        let start = match usize::try_from(entry.name_offset) {
            Ok(start) => start,
            Err(_) => {
                return Err(ApiFailure::new(
                    status::MALFORMED_WIRE,
                    "HostLayout name offsetをusizeへ変換できない",
                ))
            }
        };
        let length = match usize::try_from(entry.name_length) {
            Ok(length) => length,
            Err(_) => {
                return Err(ApiFailure::new(
                    status::MALFORMED_WIRE,
                    "HostLayout name lengthをusizeへ変換できない",
                ))
            }
        };
        let end = match start.checked_add(length) {
            Some(end) => end,
            None => {
                return Err(ApiFailure::new(
                    status::MALFORMED_WIRE,
                    "HostLayout name範囲がoverflowした",
                ))
            }
        };
        let bytes = match name_blob.get(start..end) {
            Some(bytes) => bytes,
            None => {
                return Err(ApiFailure::new(
                    status::MALFORMED_WIRE,
                    "HostLayout name範囲がblob外である",
                ))
            }
        };
        let name = match std::str::from_utf8(bytes) {
            Ok(name) => name.to_string(),
            Err(_) => {
                return Err(ApiFailure::new(
                    status::MALFORMED_WIRE,
                    "HostLayout entry nameが正しいUTF-8でない",
                ))
            }
        };
        bindings.push(BindingSpec {
            name,
            wire_type: entry.value_type,
            entity_id: entry.entity_id,
            property_id: entry.property_id,
            capability_table_index: entry.capability_table_index,
        });
    }
    Ok(bindings)
}

fn value_type(tag: u32) -> ValueType {
    match tag {
        value_tag::U1 => ValueType::U1,
        value_tag::U8 => ValueType::U8,
        value_tag::U16 => ValueType::U16,
        value_tag::U32 => ValueType::U32,
        value_tag::I32 => ValueType::I32,
        value_tag::I64 => ValueType::I64,
        value_tag::F32 => ValueType::F32,
        value_tag::F64 => ValueType::F64,
        value_tag::UTF8 | value_tag::BYTES => ValueType::Str,
        _ => unreachable!("decode_layoutで検査済み"),
    }
}

fn is_supported_value_tag(tag: u32) -> bool {
    matches!(
        tag,
        value_tag::U1
            | value_tag::U8
            | value_tag::U16
            | value_tag::U32
            | value_tag::I32
            | value_tag::I64
            | value_tag::F32
            | value_tag::F64
            | value_tag::UTF8
            | value_tag::BYTES
    )
}

fn wire_to_vaak_value(value: &WireValueV0) -> Value {
    match value {
        WireValueV0::U1(value) => Value::U1(*value),
        WireValueV0::U8(value) => Value::U8(*value),
        WireValueV0::U16(value) => Value::U16(*value),
        WireValueV0::U32(value) => Value::U32(*value),
        WireValueV0::I32(value) => Value::I32(*value),
        WireValueV0::I64(value) => Value::I64(*value),
        WireValueV0::F32(value) => Value::F32(*value),
        WireValueV0::F64(value) => Value::F64(*value),
        WireValueV0::Utf8(value) => Value::Str(Box::new(value.as_bytes().to_vec())),
        WireValueV0::Bytes(value) => Value::Str(Box::new(value.clone())),
    }
}

fn vaak_value_to_wire(value: Value, tag: u32) -> Result<WireValueV0, String> {
    match (value, tag) {
        (Value::U1(value), value_tag::U1) => Ok(WireValueV0::U1(value)),
        (Value::U8(value), value_tag::U8) => Ok(WireValueV0::U8(value)),
        (Value::U16(value), value_tag::U16) => Ok(WireValueV0::U16(value)),
        (Value::U32(value), value_tag::U32) => Ok(WireValueV0::U32(value)),
        (Value::I32(value), value_tag::I32) => Ok(WireValueV0::I32(value)),
        (Value::I64(value), value_tag::I64) => Ok(WireValueV0::I64(value)),
        (Value::F32(value), value_tag::F32) if value.is_finite() => Ok(WireValueV0::F32(value)),
        (Value::F64(value), value_tag::F64) if value.is_finite() => Ok(WireValueV0::F64(value)),
        (Value::Str(value), value_tag::UTF8) => String::from_utf8(*value)
            .map(WireValueV0::Utf8)
            .map_err(|_| "UTF8 propertyがrun後に不正byte列になった".into()),
        (Value::Str(value), value_tag::BYTES) => Ok(WireValueV0::Bytes(*value)),
        _ => Err("値の型またはfinite制約が固定HostLayoutと一致しない".into()),
    }
}

fn eval_outcome(eval: Eval) -> TopLevelOutcomeV0 {
    match eval {
        Eval::Akasha => TopLevelOutcomeV0::Akasha,
        Eval::Value(value) => match value_to_untyped_wire(value) {
            Some(value) => TopLevelOutcomeV0::Value(value),
            None => TopLevelOutcomeV0::UnsupportedAggregate,
        },
        Eval::Paradox(span) => TopLevelOutcomeV0::Paradox {
            span_start: span.start as u64,
            span_length: span.end.saturating_sub(span.start) as u64,
        },
        Eval::Escape(_) => TopLevelOutcomeV0::Escape,
    }
}

fn value_to_untyped_wire(value: Value) -> Option<WireValueV0> {
    match value {
        Value::U1(value) => Some(WireValueV0::U1(value)),
        Value::U8(value) => Some(WireValueV0::U8(value)),
        Value::U16(value) => Some(WireValueV0::U16(value)),
        Value::U32(value) => Some(WireValueV0::U32(value)),
        Value::I32(value) => Some(WireValueV0::I32(value)),
        Value::I64(value) => Some(WireValueV0::I64(value)),
        Value::F32(value) if value.is_finite() => Some(WireValueV0::F32(value)),
        Value::F64(value) if value.is_finite() => Some(WireValueV0::F64(value)),
        Value::Str(value) => Some(WireValueV0::Bytes(*value)),
        _ => None,
    }
}

fn codec_failure(error: CodecError) -> ApiFailure {
    let status_code = match error.kind {
        CodecErrorKind::Malformed => status::MALFORMED_WIRE,
        CodecErrorKind::LimitExceeded => status::LIMIT_EXCEEDED,
    };
    ApiFailure::new(status_code, error.message)
}

fn prepared_run_failure(error: PreparedRunError) -> ApiFailure {
    ApiFailure::new(
        status::INVALID_ARGUMENT,
        format!("prepared runner契約違反: {error}"),
    )
}

fn boundary<T>(operation: impl FnOnce() -> ApiResult<T>) -> ApiResult<T> {
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(result) => result,
        Err(_) => ApiResult::internal_panic(),
    }
}

fn internal_panic_diagnostic() -> DiagnosticV0 {
    DiagnosticV0::error(
        diagnostic_origin::INTERNAL_PANIC,
        0,
        0,
        "native boundary内でpanicを捕捉した",
    )
}

#[derive(Clone)]
struct BindingSpec {
    name: String,
    wire_type: u32,
    entity_id: u64,
    property_id: u32,
    capability_table_index: u32,
}

struct SnapshotValues {
    values: Vec<Value>,
    before: Vec<WireValueV0>,
    revisions: Vec<u64>,
}

struct ApiFailure {
    status: u32,
    message: String,
}

impl ApiFailure {
    fn new(status: u32, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    fn into_result<T>(self) -> ApiResult<T> {
        ApiResult::transport_error(self.status, self.message)
    }
}

struct PreparedRecord {
    program: PreparedProgram,
    bindings: Vec<BindingSpec>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RunnerState {
    Idle,
    Running,
    ReportReady,
    Poisoned,
}

struct RunnerRecord {
    prepared: Rc<PreparedRecord>,
    runner: EmbeddingRunner,
    state: RunnerState,
    report: Option<RunnerReportV0>,
}

#[derive(Default)]
struct Registry {
    prepared: SlotTable<Rc<PreparedRecord>>,
    runners: SlotTable<Rc<RefCell<RunnerRecord>>>,
}

struct Slot<T> {
    generation: u32,
    value: Option<T>,
}

struct SlotTable<T> {
    slots: Vec<Slot<T>>,
    free: Vec<usize>,
}

impl<T> Default for SlotTable<T> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
        }
    }
}

impl<T> SlotTable<T> {
    fn insert(&mut self, value: T, kind: u8) -> u64 {
        let index = match self.free.pop() {
            Some(index) => {
                let slot = &mut self.slots[index];
                slot.generation = next_generation(slot.generation);
                slot.value = Some(value);
                index
            }
            None => {
                let index = self.slots.len();
                self.slots.push(Slot {
                    generation: 1,
                    value: Some(value),
                });
                index
            }
        };
        encode_handle(kind, self.slots[index].generation, index)
    }

    fn get(&self, raw: u64, kind: u8) -> Option<&T> {
        let (actual_kind, generation, index) = decode_handle(raw)?;
        if actual_kind != kind {
            return None;
        }
        let slot = self.slots.get(index)?;
        (slot.generation == generation)
            .then_some(slot.value.as_ref())
            .flatten()
    }

    fn remove_idempotent(&mut self, raw: u64, kind: u8) {
        let Some((actual_kind, generation, index)) = decode_handle(raw) else {
            return;
        };
        if actual_kind != kind {
            return;
        }
        let Some(slot) = self.slots.get_mut(index) else {
            return;
        };
        if slot.generation != generation || slot.value.is_none() {
            return;
        }
        slot.value = None;
        self.free.push(index);
    }
}

fn encode_handle(kind: u8, generation: u32, index: usize) -> u64 {
    let one_based = u32::try_from(index)
        .expect("record上限によりhandle indexはu32内")
        .checked_add(1)
        .expect("record上限によりhandle indexはoverflowしない");
    (u64::from(kind) << 56) | (u64::from(generation) << 32) | u64::from(one_based)
}

fn decode_handle(raw: u64) -> Option<(u8, u32, usize)> {
    if raw == 0 {
        return None;
    }
    let kind = (raw >> 56) as u8;
    let generation = ((raw >> 32) & u64::from(MAX_GENERATION)) as u32;
    let one_based = raw as u32;
    if generation == 0 || one_based == 0 {
        return None;
    }
    Some((kind, generation, one_based as usize - 1))
}

fn handle_kind(raw: u64) -> u8 {
    (raw >> 56) as u8
}

fn next_generation(generation: u32) -> u32 {
    if generation >= MAX_GENERATION {
        1
    } else {
        generation + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{decode_patch, encode_snapshot, SnapshotEntryV0};

    fn one_i64_layout() -> (Vec<HostLayoutEntryV0>, Vec<u8>) {
        let names = b"n".to_vec();
        (
            vec![HostLayoutEntryV0::new(0, value_tag::I64, 0, 10, 23, 0, 1)],
            names,
        )
    }

    fn prepare_increment(api: &NativeApi) -> PreparedHandleV0 {
        let (layout, names) = one_i64_layout();
        let reply = api.prepare(b"n += 1; n", &layout, &names);
        assert_eq!(reply.envelope.transport_status, status::OK);
        reply.value.expect("reply").prepared.expect("prepared")
    }

    fn snapshot(value: i64) -> Vec<u8> {
        encode_snapshot(&SnapshotBatchV0 {
            schema_id: [1; 16],
            session_id: [2; 16],
            snapshot_revision: 3,
            entries: vec![SnapshotEntryV0 {
                entity_id: 0,
                property_id: 10,
                property_revision: 4,
                value: WireValueV0::I64(value),
            }],
        })
        .expect("snapshot")
    }

    #[test]
    fn utf8_sourceと固定layoutを一度prepareしてrunnerを再利用する() {
        let api = NativeApi::new();
        let prepared = prepare_increment(&api);
        let runner = api.runner_new(prepared).value.expect("runner");
        for value in [1, 10, 100] {
            let run = api.runner_run(runner, &snapshot(value), [7; 16], [8; 16]);
            assert_eq!(run.envelope.transport_status, status::OK);
            let report = api.runner_report(runner).value.expect("report");
            assert_eq!(report.program_status, ProgramStatusV0::Completed);
            let patch = decode_patch(&report.patch_wire).expect("patch");
            assert_eq!(patch.entries.len(), 1);
            assert_eq!(patch.entries[0].value, WireValueV0::I64(value + 1));
            assert_eq!(patch.entries[0].expected_property_revision, 4);
            assert_eq!(patch.entries[0].capability_table_index, 23);
        }
    }

    #[test]
    fn preparedを先にdestroyしてもrunnerのstrong参照で走る() {
        let api = NativeApi::new();
        let prepared = prepare_increment(&api);
        let runner = api.runner_new(prepared).value.expect("runner");
        assert_eq!(
            api.prepared_destroy(prepared).envelope.transport_status,
            status::OK
        );
        assert_eq!(
            api.prepared_destroy(prepared).envelope.transport_status,
            status::OK
        );
        assert_eq!(
            api.runner_run(runner, &snapshot(4), [0; 16], [0; 16])
                .envelope
                .transport_status,
            status::OK
        );
    }

    #[test]
    fn runnerのdestroyは零と反復を安全なno_opにする() {
        let api = NativeApi::new();
        assert_eq!(
            api.runner_destroy(RunnerHandleV0::INVALID)
                .envelope
                .transport_status,
            status::OK
        );
        let prepared = prepare_increment(&api);
        let runner = api.runner_new(prepared).value.expect("runner");
        assert_eq!(
            api.runner_destroy(runner).envelope.transport_status,
            status::OK
        );
        assert_eq!(
            api.runner_destroy(runner).envelope.transport_status,
            status::OK
        );
        assert_eq!(
            api.runner_run(runner, &snapshot(1), [0; 16], [0; 16])
                .envelope
                .transport_status,
            status::STALE_HANDLE
        );
    }

    #[test]
    fn 同じrunnerへの再入をbusyで拒む() {
        let api = NativeApi::new();
        let prepared = prepare_increment(&api);
        let runner = api.runner_new(prepared).value.expect("runner");
        let cell = api.runner_cell_for_test(runner);
        let _running = cell.borrow_mut();
        let result = api.runner_run(runner, &snapshot(1), [0; 16], [0; 16]);
        assert_eq!(result.envelope.transport_status, status::BUSY);
    }

    #[test]
    fn panicを境界内で捕捉してrunnerをpoisonする() {
        let api = NativeApi::new();
        let prepared = prepare_increment(&api);
        let runner = api.runner_new(prepared).value.expect("runner");
        let panic = api.panic_runner_for_test(runner);
        assert_eq!(panic.envelope.transport_status, status::INTERNAL_PANIC);
        let report = api.runner_report(runner).value.expect("panic report");
        assert_eq!(report.program_status, ProgramStatusV0::InternalPanic);
        assert_eq!(
            api.runner_run(runner, &snapshot(1), [0; 16], [0; 16])
                .envelope
                .transport_status,
            status::POISONED
        );
    }

    #[test]
    fn runtime_error時もafter_state_patchをreportへ保持する() {
        let api = NativeApi::new();
        let (layout, names) = one_i64_layout();
        let reply = api.prepare(b"n := 42; n := n / 0; 0", &layout, &names);
        let prepared = reply.value.expect("reply").prepared.expect("prepared");
        let runner = api.runner_new(prepared).value.expect("runner");
        assert_eq!(
            api.runner_run(runner, &snapshot(1), [3; 16], [4; 16])
                .envelope
                .transport_status,
            status::OK
        );
        let report = api.runner_report(runner).value.expect("report");
        assert_eq!(report.program_status, ProgramStatusV0::ProgramError);
        assert_eq!(report.diagnostics.len(), 1);
        let patch = decode_patch(&report.patch_wire).expect("patch");
        assert_eq!(patch.entries[0].value, WireValueV0::I64(42));
    }

    #[test]
    fn 不正utf8_sourceとlayout範囲外をprepare前に拒む() {
        let api = NativeApi::new();
        let (mut layout, names) = one_i64_layout();
        assert_eq!(
            api.prepare(&[0xff], &layout, &names)
                .envelope
                .transport_status,
            status::MALFORMED_WIRE
        );
        layout[0].name_offset = u64::MAX;
        assert_eq!(
            api.prepare(b"0", &layout, &names).envelope.transport_status,
            status::MALFORMED_WIRE
        );
    }

    #[test]
    fn prepare診断をtransport失敗と混ぜない() {
        let api = NativeApi::new();
        let (layout, names) = one_i64_layout();
        let reply = api.prepare(b"(", &layout, &names);
        assert_eq!(reply.envelope.transport_status, status::OK);
        let reply = reply.value.expect("reply");
        assert!(reply.prepared.is_none());
        assert!(!reply.diagnostics.is_empty());
    }
}
