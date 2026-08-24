//! IRON VAAK v0 のraw C ABI shim。
//!
//! `iron-vaak-ffi`のsafe coreだけを呼び、raw pointerをsliceへ変える処理はこのcrateへ隔離する。
//! Context・prepared・runnerはopaque整数handleであり、managed object、Unity object、Lua stateを
//! native側へ保持しない。

#![deny(unsafe_op_in_unsafe_fn)]

use iron_vaak_ffi::abi::{
    program_status, status, top_level_kind, AbiInfoV0, CallEnvelopeV0, ContextHandleV0,
    DiagnosticEnvelopeV0, DiagnosticsHandleV0, HostLayoutEntryV0, PreparedHandleV0, RunnerHandleV0,
    RunnerReportInfoV0,
};
use iron_vaak_ffi::{
    ApiResult, DiagnosticV0, NativeApi, PrepareReplyV0, ProgramStatusV0, RunnerReportV0,
    TopLevelOutcomeV0, WireValueV0,
};
use std::collections::HashMap;
use std::mem::{align_of, size_of};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;
use std::slice;
use std::sync::{Arc, Mutex, OnceLock};

const MAX_DIAGNOSTIC_SETS: usize = u16::MAX as usize;

struct FfiContext {
    api: NativeApi,
    diagnostics: Mutex<DiagnosticRegistry>,
}

impl FfiContext {
    fn new() -> Self {
        Self {
            api: NativeApi::new(),
            diagnostics: Mutex::new(DiagnosticRegistry::default()),
        }
    }

    fn attach_diagnostics(
        &self,
        mut envelope: CallEnvelopeV0,
        diagnostics: Vec<DiagnosticV0>,
    ) -> CallEnvelopeV0 {
        if diagnostics.is_empty() {
            return envelope;
        }
        match self
            .diagnostics
            .lock()
            .expect("diagnostic registry mutexがpoisonされた")
            .insert(diagnostics)
        {
            Some(id) => envelope.diagnostic_id = id,
            None => envelope.transport_status = status::LIMIT_EXCEEDED,
        }
        envelope
    }

    fn finish<T>(&self, result: ApiResult<T>) -> (CallEnvelopeV0, Option<T>) {
        let diagnostics = result.diagnostic.into_iter().collect();
        (
            self.attach_diagnostics(result.envelope, diagnostics),
            result.value,
        )
    }
}

#[derive(Default)]
struct DiagnosticRegistry {
    next: u64,
    sets: HashMap<u64, Arc<[DiagnosticV0]>>,
}

impl DiagnosticRegistry {
    fn insert(&mut self, diagnostics: Vec<DiagnosticV0>) -> Option<u64> {
        if self.sets.len() >= MAX_DIAGNOSTIC_SETS {
            return None;
        }
        loop {
            self.next = self.next.wrapping_add(1);
            if self.next == 0 || self.sets.contains_key(&self.next) {
                continue;
            }
            self.sets.insert(self.next, diagnostics.into());
            return Some(self.next);
        }
    }
}

#[derive(Default)]
struct ContextRegistry {
    next: u64,
    contexts: HashMap<u64, Arc<FfiContext>>,
}

impl ContextRegistry {
    fn insert(&mut self, context: Arc<FfiContext>) -> u64 {
        loop {
            self.next = self.next.wrapping_add(1);
            if self.next == 0 || self.contexts.contains_key(&self.next) {
                continue;
            }
            self.contexts.insert(self.next, context);
            return self.next;
        }
    }
}

fn contexts() -> &'static Mutex<ContextRegistry> {
    static CONTEXTS: OnceLock<Mutex<ContextRegistry>> = OnceLock::new();
    CONTEXTS.get_or_init(|| Mutex::new(ContextRegistry::default()))
}

fn context(handle: ContextHandleV0) -> Result<Arc<FfiContext>, CallEnvelopeV0> {
    if handle == ContextHandleV0::INVALID {
        return Err(CallEnvelopeV0::with_status(status::STALE_HANDLE));
    }
    contexts()
        .lock()
        .expect("context registry mutexがpoisonされた")
        .contexts
        .get(&handle.0)
        .cloned()
        .ok_or_else(|| CallEnvelopeV0::with_status(status::STALE_HANDLE))
}

fn invalid_argument() -> CallEnvelopeV0 {
    CallEnvelopeV0::with_status(status::INVALID_ARGUMENT)
}

fn internal_panic() -> CallEnvelopeV0 {
    CallEnvelopeV0::with_status(status::INTERNAL_PANIC)
}

fn is_aligned<T>(pointer: *const T) -> bool {
    (pointer as usize).is_multiple_of(align_of::<T>())
}

unsafe fn input_bytes<'a>(pointer: *const u8, length: u64) -> Result<&'a [u8], CallEnvelopeV0> {
    let length = usize::try_from(length).map_err(|_| invalid_argument())?;
    if length == 0 {
        return Ok(&[]);
    }
    if pointer.is_null() || length > isize::MAX as usize {
        return Err(invalid_argument());
    }
    // SAFETY: caller owns a readable range of `length` bytes for this call; null and isize bounds
    // were checked above. The FFI contract forbids concurrent mutation of borrowed input.
    Ok(unsafe { slice::from_raw_parts(pointer, length) })
}

unsafe fn input_records<'a, T>(pointer: *const T, count: u64) -> Result<&'a [T], CallEnvelopeV0> {
    let count = usize::try_from(count).map_err(|_| invalid_argument())?;
    if count == 0 {
        return Ok(&[]);
    }
    let bytes = count
        .checked_mul(size_of::<T>())
        .filter(|bytes| *bytes <= isize::MAX as usize)
        .ok_or_else(invalid_argument)?;
    if pointer.is_null() || !is_aligned(pointer) || bytes == 0 {
        return Err(invalid_argument());
    }
    // SAFETY: caller owns `count` aligned initialized records for the duration of this call.
    Ok(unsafe { slice::from_raw_parts(pointer, count) })
}

unsafe fn read_id(pointer: *const u8) -> Result<[u8; 16], CallEnvelopeV0> {
    let bytes = unsafe { input_bytes(pointer, 16)? };
    let mut id = [0; 16];
    id.copy_from_slice(bytes);
    Ok(id)
}

unsafe fn write_value<T: Copy>(pointer: *mut T, value: T) -> Result<(), CallEnvelopeV0> {
    if pointer.is_null() || !is_aligned(pointer) {
        return Err(invalid_argument());
    }
    // SAFETY: caller supplied a writable, aligned output record for this call.
    unsafe { pointer.write(value) };
    Ok(())
}

unsafe fn copy_bytes(
    bytes: &[u8],
    output: *mut u8,
    capacity: u64,
) -> Result<CallEnvelopeV0, CallEnvelopeV0> {
    let mut envelope = CallEnvelopeV0 {
        required_bytes: bytes.len() as u64,
        ..CallEnvelopeV0::default()
    };
    if capacity < bytes.len() as u64 {
        envelope.transport_status = status::BUFFER_TOO_SMALL;
        return Ok(envelope);
    }
    if !bytes.is_empty() {
        let capacity = usize::try_from(capacity).map_err(|_| invalid_argument())?;
        if output.is_null() || capacity > isize::MAX as usize {
            return Err(invalid_argument());
        }
        // SAFETY: capacity was checked against the source length and the caller owns the output.
        unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), output, bytes.len()) };
    }
    envelope.written_bytes = bytes.len() as u64;
    Ok(envelope)
}

unsafe fn export(
    output_envelope: *mut CallEnvelopeV0,
    operation: impl FnOnce() -> CallEnvelopeV0,
) -> u32 {
    if output_envelope.is_null() || !is_aligned(output_envelope) {
        return status::INVALID_ARGUMENT;
    }
    let envelope = catch_unwind(AssertUnwindSafe(operation)).unwrap_or_else(|_| internal_panic());
    let result = envelope.transport_status;
    // SAFETY: validated above; writing the envelope is the final operation at the ABI boundary.
    unsafe { output_envelope.write(envelope) };
    result
}

fn report_info(report: &RunnerReportV0) -> RunnerReportInfoV0 {
    let mut info = RunnerReportInfoV0::empty();
    info.program_status = match report.program_status {
        ProgramStatusV0::Completed => program_status::COMPLETED,
        ProgramStatusV0::ProgramError => program_status::PROGRAM_ERROR,
        ProgramStatusV0::HostContractError => program_status::HOST_CONTRACT_ERROR,
        ProgramStatusV0::InternalPanic => program_status::INTERNAL_PANIC,
    };
    info.patch_bytes = report.patch_wire.len() as u64;
    info.diagnostic_count = report.diagnostics.len() as u64;
    match report.top_level.as_ref() {
        None => info.top_level_kind = top_level_kind::NONE,
        Some(TopLevelOutcomeV0::Akasha) => info.top_level_kind = top_level_kind::AKASHA,
        Some(TopLevelOutcomeV0::Value(value)) => {
            info.top_level_kind = top_level_kind::VALUE;
            info.top_level_value_type = value.type_tag();
            match value {
                WireValueV0::U1(value) => info.top_level_scalar_bits = u64::from(*value),
                WireValueV0::U8(value) => info.top_level_scalar_bits = u64::from(*value),
                WireValueV0::U16(value) => info.top_level_scalar_bits = u64::from(*value),
                WireValueV0::U32(value) => info.top_level_scalar_bits = u64::from(*value),
                WireValueV0::I32(value) => {
                    info.top_level_scalar_bits = u64::from(u32::from_le_bytes(value.to_le_bytes()))
                }
                WireValueV0::I64(value) => info.top_level_scalar_bits = *value as u64,
                WireValueV0::F32(value) => info.top_level_scalar_bits = u64::from(value.to_bits()),
                WireValueV0::F64(value) => info.top_level_scalar_bits = value.to_bits(),
                WireValueV0::Utf8(value) => info.top_level_bytes = value.len() as u64,
                WireValueV0::Bytes(value) => info.top_level_bytes = value.len() as u64,
            }
        }
        Some(TopLevelOutcomeV0::Paradox {
            span_start,
            span_length,
        }) => {
            info.top_level_kind = top_level_kind::PARADOX;
            info.top_level_span_start = *span_start;
            info.top_level_span_length = *span_length;
        }
        Some(TopLevelOutcomeV0::Escape) => info.top_level_kind = top_level_kind::ESCAPE,
        Some(TopLevelOutcomeV0::UnsupportedAggregate) => {
            info.top_level_kind = top_level_kind::UNSUPPORTED_AGGREGATE
        }
    }
    info
}

fn top_level_bytes(report: &RunnerReportV0) -> &[u8] {
    match report.top_level.as_ref() {
        Some(TopLevelOutcomeV0::Value(WireValueV0::Utf8(value))) => value.as_bytes(),
        Some(TopLevelOutcomeV0::Value(WireValueV0::Bytes(value))) => value,
        _ => &[],
    }
}

/// # Safety
/// `output_info` and `output_envelope` must be aligned writable records for this call.
#[no_mangle]
pub unsafe extern "C" fn iron_vaak_v0_abi_info(
    output_info: *mut AbiInfoV0,
    output_envelope: *mut CallEnvelopeV0,
) -> u32 {
    unsafe {
        export(output_envelope, || {
            if write_value(output_info, AbiInfoV0::default()).is_err() {
                return invalid_argument();
            }
            CallEnvelopeV0::default()
        })
    }
}

/// # Safety
/// Both output pointers must be aligned writable records for this call.
#[no_mangle]
pub unsafe extern "C" fn iron_vaak_v0_context_create(
    output_context: *mut ContextHandleV0,
    output_envelope: *mut CallEnvelopeV0,
) -> u32 {
    unsafe {
        export(output_envelope, || {
            let context = Arc::new(FfiContext::new());
            let raw = contexts()
                .lock()
                .expect("context registry mutexがpoisonされた")
                .insert(context);
            if write_value(output_context, ContextHandleV0(raw)).is_err() {
                contexts()
                    .lock()
                    .expect("context registry mutexがpoisonされた")
                    .contexts
                    .remove(&raw);
                return invalid_argument();
            }
            CallEnvelopeV0::default()
        })
    }
}

/// # Safety
/// `output_envelope` must be an aligned writable record for this call.
#[no_mangle]
pub unsafe extern "C" fn iron_vaak_v0_context_destroy(
    context_handle: ContextHandleV0,
    output_envelope: *mut CallEnvelopeV0,
) -> u32 {
    unsafe {
        export(output_envelope, || {
            if context_handle != ContextHandleV0::INVALID {
                contexts()
                    .lock()
                    .expect("context registry mutexがpoisonされた")
                    .contexts
                    .remove(&context_handle.0);
            }
            CallEnvelopeV0::default()
        })
    }
}

/// # Safety
/// Every non-empty input range must be readable and non-overlapping with the aligned writable
/// output records for this call. `entries` must point to initialized `HostLayoutEntryV0` records.
#[no_mangle]
pub unsafe extern "C" fn iron_vaak_v0_prepare(
    context_handle: ContextHandleV0,
    source: *const u8,
    source_length: u64,
    entries: *const HostLayoutEntryV0,
    entry_count: u64,
    names: *const u8,
    names_length: u64,
    output_prepared: *mut PreparedHandleV0,
    output_envelope: *mut CallEnvelopeV0,
) -> u32 {
    unsafe {
        export(output_envelope, || {
            let context = match context(context_handle) {
                Ok(context) => context,
                Err(envelope) => return envelope,
            };
            let source = match input_bytes(source, source_length) {
                Ok(source) => source.to_vec(),
                Err(envelope) => return envelope,
            };
            let entries = match input_records(entries, entry_count) {
                Ok(entries) => entries.to_vec(),
                Err(envelope) => return envelope,
            };
            let names = match input_bytes(names, names_length) {
                Ok(names) => names.to_vec(),
                Err(envelope) => return envelope,
            };
            if write_value(output_prepared, PreparedHandleV0::INVALID).is_err() {
                return invalid_argument();
            }
            let result = context.api.prepare(&source, &entries, &names);
            let mut diagnostics = result.diagnostic.into_iter().collect::<Vec<_>>();
            let mut prepared = PreparedHandleV0::INVALID;
            if let Some(PrepareReplyV0 {
                prepared: result_prepared,
                diagnostics: result_diagnostics,
            }) = result.value
            {
                prepared = result_prepared.unwrap_or(PreparedHandleV0::INVALID);
                diagnostics.extend(result_diagnostics);
            }
            if write_value(output_prepared, prepared).is_err() {
                if prepared != PreparedHandleV0::INVALID {
                    let _ = context.api.prepared_destroy(prepared);
                }
                return invalid_argument();
            }
            context.attach_diagnostics(result.envelope, diagnostics)
        })
    }
}

/// # Safety
/// `output_envelope` must be an aligned writable record for this call.
#[no_mangle]
pub unsafe extern "C" fn iron_vaak_v0_prepared_destroy(
    context_handle: ContextHandleV0,
    prepared: PreparedHandleV0,
    output_envelope: *mut CallEnvelopeV0,
) -> u32 {
    unsafe {
        export(output_envelope, || {
            let context = match context(context_handle) {
                Ok(context) => context,
                Err(envelope) => return envelope,
            };
            let (envelope, _) = context.finish(context.api.prepared_destroy(prepared));
            envelope
        })
    }
}

/// # Safety
/// Both output pointers must be aligned writable records for this call.
#[no_mangle]
pub unsafe extern "C" fn iron_vaak_v0_runner_create(
    context_handle: ContextHandleV0,
    prepared: PreparedHandleV0,
    output_runner: *mut RunnerHandleV0,
    output_envelope: *mut CallEnvelopeV0,
) -> u32 {
    unsafe {
        export(output_envelope, || {
            let context = match context(context_handle) {
                Ok(context) => context,
                Err(envelope) => return envelope,
            };
            if write_value(output_runner, RunnerHandleV0::INVALID).is_err() {
                return invalid_argument();
            }
            let (envelope, runner) = context.finish(context.api.runner_new(prepared));
            if envelope.transport_status == status::OK {
                if let Some(runner) = runner {
                    if write_value(output_runner, runner).is_err() {
                        let _ = context.api.runner_destroy(runner);
                        return invalid_argument();
                    }
                }
            }
            envelope
        })
    }
}

/// # Safety
/// `output_envelope` must be an aligned writable record for this call.
#[no_mangle]
pub unsafe extern "C" fn iron_vaak_v0_runner_destroy(
    context_handle: ContextHandleV0,
    runner: RunnerHandleV0,
    output_envelope: *mut CallEnvelopeV0,
) -> u32 {
    unsafe {
        export(output_envelope, || {
            let context = match context(context_handle) {
                Ok(context) => context,
                Err(envelope) => return envelope,
            };
            let (envelope, _) = context.finish(context.api.runner_destroy(runner));
            envelope
        })
    }
}

/// # Safety
/// `snapshot` must be readable for `snapshot_length`; both ID pointers must be readable for
/// exactly 16 bytes; `output_envelope` must be aligned and writable. Ranges must not overlap.
#[no_mangle]
pub unsafe extern "C" fn iron_vaak_v0_runner_run(
    context_handle: ContextHandleV0,
    runner: RunnerHandleV0,
    snapshot: *const u8,
    snapshot_length: u64,
    run_id: *const u8,
    transaction_id: *const u8,
    output_envelope: *mut CallEnvelopeV0,
) -> u32 {
    unsafe {
        export(output_envelope, || {
            let context = match context(context_handle) {
                Ok(context) => context,
                Err(envelope) => return envelope,
            };
            let snapshot = match input_bytes(snapshot, snapshot_length) {
                Ok(snapshot) => snapshot,
                Err(envelope) => return envelope,
            };
            let run_id = match read_id(run_id) {
                Ok(id) => id,
                Err(envelope) => return envelope,
            };
            let transaction_id = match read_id(transaction_id) {
                Ok(id) => id,
                Err(envelope) => return envelope,
            };
            let (envelope, _) = context.finish(context.api.runner_run(
                runner,
                snapshot,
                run_id,
                transaction_id,
            ));
            envelope
        })
    }
}

/// # Safety
/// `output_info` and `output_envelope` must be aligned writable records for this call.
#[no_mangle]
pub unsafe extern "C" fn iron_vaak_v0_runner_report_info(
    context_handle: ContextHandleV0,
    runner: RunnerHandleV0,
    output_info: *mut RunnerReportInfoV0,
    output_envelope: *mut CallEnvelopeV0,
) -> u32 {
    unsafe {
        export(output_envelope, || {
            let context = match context(context_handle) {
                Ok(context) => context,
                Err(envelope) => return envelope,
            };
            let (envelope, report) = context.finish(context.api.runner_report(runner));
            if envelope.transport_status != status::OK {
                return envelope;
            }
            let Some(report) = report else {
                return internal_panic();
            };
            if write_value(output_info, report_info(&report)).is_err() {
                return invalid_argument();
            }
            envelope
        })
    }
}

/// # Safety
/// A non-zero `capacity` requires `output` to address that writable range. The envelope must be
/// aligned and writable, and output ranges must not overlap.
#[no_mangle]
pub unsafe extern "C" fn iron_vaak_v0_runner_report_patch_copy(
    context_handle: ContextHandleV0,
    runner: RunnerHandleV0,
    output: *mut u8,
    capacity: u64,
    output_envelope: *mut CallEnvelopeV0,
) -> u32 {
    unsafe {
        export(output_envelope, || {
            let context = match context(context_handle) {
                Ok(context) => context,
                Err(envelope) => return envelope,
            };
            let (envelope, report) = context.finish(context.api.runner_report(runner));
            if envelope.transport_status != status::OK {
                return envelope;
            }
            let Some(report) = report else {
                return internal_panic();
            };
            match copy_bytes(&report.patch_wire, output, capacity) {
                Ok(copy) => copy,
                Err(error) => error,
            }
        })
    }
}

/// # Safety
/// A non-zero `capacity` requires `output` to address that writable range. The envelope must be
/// aligned and writable, and output ranges must not overlap.
#[no_mangle]
pub unsafe extern "C" fn iron_vaak_v0_runner_report_top_level_copy(
    context_handle: ContextHandleV0,
    runner: RunnerHandleV0,
    output: *mut u8,
    capacity: u64,
    output_envelope: *mut CallEnvelopeV0,
) -> u32 {
    unsafe {
        export(output_envelope, || {
            let context = match context(context_handle) {
                Ok(context) => context,
                Err(envelope) => return envelope,
            };
            let (envelope, report) = context.finish(context.api.runner_report(runner));
            if envelope.transport_status != status::OK {
                return envelope;
            }
            let Some(report) = report else {
                return internal_panic();
            };
            match copy_bytes(top_level_bytes(&report), output, capacity) {
                Ok(copy) => copy,
                Err(error) => error,
            }
        })
    }
}

/// # Safety
/// Diagnostic/envelope pointers must be aligned writable records. A non-zero message capacity
/// requires a writable byte range. All output ranges must not overlap.
#[no_mangle]
pub unsafe extern "C" fn iron_vaak_v0_runner_report_diagnostic_copy(
    context_handle: ContextHandleV0,
    runner: RunnerHandleV0,
    index: u64,
    output_diagnostic: *mut DiagnosticEnvelopeV0,
    output_message: *mut u8,
    message_capacity: u64,
    output_envelope: *mut CallEnvelopeV0,
) -> u32 {
    unsafe {
        export(output_envelope, || {
            let context = match context(context_handle) {
                Ok(context) => context,
                Err(envelope) => return envelope,
            };
            let (envelope, report) = context.finish(context.api.runner_report(runner));
            if envelope.transport_status != status::OK {
                return envelope;
            }
            let Some(report) = report else {
                return internal_panic();
            };
            let Some(diagnostic) = usize::try_from(index)
                .ok()
                .and_then(|index| report.diagnostics.get(index))
            else {
                return invalid_argument();
            };
            let copy = match copy_bytes(&diagnostic.message_utf8, output_message, message_capacity)
            {
                Ok(copy) => copy,
                Err(error) => return error,
            };
            if copy.transport_status != status::OK {
                return copy;
            }
            let mut diagnostic_envelope = diagnostic.envelope;
            diagnostic_envelope.message_offset = 0;
            if write_value(output_diagnostic, diagnostic_envelope).is_err() {
                return invalid_argument();
            }
            copy
        })
    }
}

/// # Safety
/// `output_envelope` must be an aligned writable record for this call.
#[no_mangle]
pub unsafe extern "C" fn iron_vaak_v0_runner_clear_report(
    context_handle: ContextHandleV0,
    runner: RunnerHandleV0,
    output_envelope: *mut CallEnvelopeV0,
) -> u32 {
    unsafe {
        export(output_envelope, || {
            let context = match context(context_handle) {
                Ok(context) => context,
                Err(envelope) => return envelope,
            };
            let (envelope, _) = context.finish(context.api.runner_clear_report(runner));
            envelope
        })
    }
}

/// # Safety
/// `output_count` and `output_envelope` must be aligned writable records for this call.
#[no_mangle]
pub unsafe extern "C" fn iron_vaak_v0_diagnostics_count(
    context_handle: ContextHandleV0,
    diagnostics_handle: DiagnosticsHandleV0,
    output_count: *mut u64,
    output_envelope: *mut CallEnvelopeV0,
) -> u32 {
    unsafe {
        export(output_envelope, || {
            let context = match context(context_handle) {
                Ok(context) => context,
                Err(envelope) => return envelope,
            };
            let diagnostics = context
                .diagnostics
                .lock()
                .expect("diagnostic registry mutexがpoisonされた");
            let Some(set) = diagnostics.sets.get(&diagnostics_handle.0) else {
                return CallEnvelopeV0::with_status(status::STALE_HANDLE);
            };
            if write_value(output_count, set.len() as u64).is_err() {
                return invalid_argument();
            }
            CallEnvelopeV0::default()
        })
    }
}

/// # Safety
/// Diagnostic/envelope pointers must be aligned writable records. A non-zero message capacity
/// requires a writable byte range. All output ranges must not overlap.
#[no_mangle]
pub unsafe extern "C" fn iron_vaak_v0_diagnostic_copy(
    context_handle: ContextHandleV0,
    diagnostics_handle: DiagnosticsHandleV0,
    index: u64,
    output_diagnostic: *mut DiagnosticEnvelopeV0,
    output_message: *mut u8,
    message_capacity: u64,
    output_envelope: *mut CallEnvelopeV0,
) -> u32 {
    unsafe {
        export(output_envelope, || {
            let context = match context(context_handle) {
                Ok(context) => context,
                Err(envelope) => return envelope,
            };
            let diagnostics = context
                .diagnostics
                .lock()
                .expect("diagnostic registry mutexがpoisonされた");
            let Some(diagnostic) = diagnostics
                .sets
                .get(&diagnostics_handle.0)
                .and_then(|set| usize::try_from(index).ok().and_then(|index| set.get(index)))
            else {
                return CallEnvelopeV0::with_status(status::STALE_HANDLE);
            };
            let copy = match copy_bytes(&diagnostic.message_utf8, output_message, message_capacity)
            {
                Ok(copy) => copy,
                Err(error) => return error,
            };
            if copy.transport_status != status::OK {
                return copy;
            }
            let mut diagnostic_envelope = diagnostic.envelope;
            diagnostic_envelope.message_offset = 0;
            if write_value(output_diagnostic, diagnostic_envelope).is_err() {
                return invalid_argument();
            }
            copy
        })
    }
}

/// # Safety
/// `output_envelope` must be an aligned writable record for this call.
#[no_mangle]
pub unsafe extern "C" fn iron_vaak_v0_diagnostics_destroy(
    context_handle: ContextHandleV0,
    diagnostics_handle: DiagnosticsHandleV0,
    output_envelope: *mut CallEnvelopeV0,
) -> u32 {
    unsafe {
        export(output_envelope, || {
            let context = match context(context_handle) {
                Ok(context) => context,
                Err(envelope) => return envelope,
            };
            if diagnostics_handle != DiagnosticsHandleV0::INVALID {
                context
                    .diagnostics
                    .lock()
                    .expect("diagnostic registry mutexがpoisonされた")
                    .sets
                    .remove(&diagnostics_handle.0);
            }
            CallEnvelopeV0::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iron_vaak_ffi::abi::value_tag;
    use iron_vaak_ffi::codec::{encode_snapshot, SnapshotBatchV0, SnapshotEntryV0};

    unsafe fn new_context() -> ContextHandleV0 {
        let mut envelope = CallEnvelopeV0::default();
        let mut context = ContextHandleV0::INVALID;
        assert_eq!(
            unsafe { iron_vaak_v0_context_create(&mut context, &mut envelope) },
            status::OK
        );
        context
    }

    #[test]
    fn raw境界でprepare_once_run_manyとcopyを行う() {
        unsafe {
            let context = new_context();
            let source = b"n += 1; n";
            let names = b"n";
            let layout = [HostLayoutEntryV0::new(0, value_tag::I64, 0, 10, 23, 0, 1)];
            let mut envelope = CallEnvelopeV0::default();
            let mut prepared = PreparedHandleV0::INVALID;
            assert_eq!(
                iron_vaak_v0_prepare(
                    context,
                    source.as_ptr(),
                    source.len() as u64,
                    layout.as_ptr(),
                    layout.len() as u64,
                    names.as_ptr(),
                    names.len() as u64,
                    &mut prepared,
                    &mut envelope,
                ),
                status::OK
            );
            assert_ne!(prepared, PreparedHandleV0::INVALID);
            let mut runner = RunnerHandleV0::INVALID;
            assert_eq!(
                iron_vaak_v0_runner_create(context, prepared, &mut runner, &mut envelope),
                status::OK
            );
            for value in [1, 10, 100] {
                let snapshot = encode_snapshot(&SnapshotBatchV0 {
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
                .expect("snapshot");
                assert_eq!(
                    iron_vaak_v0_runner_run(
                        context,
                        runner,
                        snapshot.as_ptr(),
                        snapshot.len() as u64,
                        [7; 16].as_ptr(),
                        [8; 16].as_ptr(),
                        &mut envelope,
                    ),
                    status::OK
                );
                let mut info = RunnerReportInfoV0::empty();
                assert_eq!(
                    iron_vaak_v0_runner_report_info(context, runner, &mut info, &mut envelope),
                    status::OK
                );
                assert_eq!(info.program_status, program_status::COMPLETED);
                assert_eq!(info.top_level_value_type, value_tag::I64);
                assert_eq!(info.top_level_scalar_bits as i64, value + 1);
                let mut too_small = [0xaa; 4];
                assert_eq!(
                    iron_vaak_v0_runner_report_patch_copy(
                        context,
                        runner,
                        too_small.as_mut_ptr(),
                        too_small.len() as u64,
                        &mut envelope,
                    ),
                    status::BUFFER_TOO_SMALL
                );
                assert_eq!(too_small, [0xaa; 4]);
                assert_eq!(envelope.written_bytes, 0);
                let mut patch = vec![0; info.patch_bytes as usize];
                assert_eq!(
                    iron_vaak_v0_runner_report_patch_copy(
                        context,
                        runner,
                        patch.as_mut_ptr(),
                        patch.len() as u64,
                        &mut envelope,
                    ),
                    status::OK
                );
                assert_eq!(envelope.written_bytes, patch.len() as u64);
            }
            assert_eq!(
                iron_vaak_v0_runner_destroy(context, runner, &mut envelope),
                status::OK
            );
            assert_eq!(
                iron_vaak_v0_prepared_destroy(context, prepared, &mut envelope),
                status::OK
            );
            assert_eq!(
                iron_vaak_v0_context_destroy(context, &mut envelope),
                status::OK
            );
        }
    }

    #[test]
    fn prepare診断はopaque_setとしてcopyできる() {
        unsafe {
            let context = new_context();
            let mut envelope = CallEnvelopeV0::default();
            let mut prepared = PreparedHandleV0::INVALID;
            assert_eq!(
                iron_vaak_v0_prepare(
                    context,
                    b"(".as_ptr(),
                    1,
                    ptr::null(),
                    0,
                    ptr::null(),
                    0,
                    &mut prepared,
                    &mut envelope,
                ),
                status::OK
            );
            assert_eq!(prepared, PreparedHandleV0::INVALID);
            assert_ne!(envelope.diagnostic_id, 0);
            let diagnostics = DiagnosticsHandleV0(envelope.diagnostic_id);
            let mut count = 0;
            assert_eq!(
                iron_vaak_v0_diagnostics_count(context, diagnostics, &mut count, &mut envelope),
                status::OK
            );
            assert!(count > 0);
            let mut diagnostic = DiagnosticEnvelopeV0::error(0, 0, 0, 0);
            assert_eq!(
                iron_vaak_v0_diagnostic_copy(
                    context,
                    diagnostics,
                    0,
                    &mut diagnostic,
                    ptr::null_mut(),
                    0,
                    &mut envelope,
                ),
                status::BUFFER_TOO_SMALL
            );
            let mut message = vec![0; envelope.required_bytes as usize];
            assert_eq!(
                iron_vaak_v0_diagnostic_copy(
                    context,
                    diagnostics,
                    0,
                    &mut diagnostic,
                    message.as_mut_ptr(),
                    message.len() as u64,
                    &mut envelope,
                ),
                status::OK
            );
            assert!(std::str::from_utf8(&message).is_ok());
            assert_eq!(
                iron_vaak_v0_diagnostics_destroy(context, diagnostics, &mut envelope),
                status::OK
            );
            assert_eq!(
                iron_vaak_v0_context_destroy(context, &mut envelope),
                status::OK
            );
        }
    }
}
