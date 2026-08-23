//! 組み立て済みのVaak programをhostへ埋め込むためのAPI。
//!
//! [`prepare`] はparse・静的検査・型検査・VM組み立てを一度だけ行う。
//! [`EmbeddingRunner`] は既存の [`crate::vm::Runner`] を包み、同じprogramを
//! 走らせるたびにarenaやstackの容量を再利用する。
//!
//! このmoduleはVaakの言語意味を足さない。最上位programだけを扱い、named entry、
//! 中断・再開、phase、WASM、個々のhost functionの意味はhost側の契約である。

use crate::ast::{BindKind, HostItem, HostSig, ValueType};
use crate::host::{
    vm_binding_values_uncloned, vm_binding_writeback, HostBinding, HostFn, VmBindingState,
    MAX_HOST_VALUE_NODES,
};
use crate::interp::Eval;
use crate::span::Span;
use crate::value::{HostFns, MapKey, NoHostFns, Value};
use crate::vm::{Program2, RtErr, Runner};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock, Weak};

const MAX_HOST_TYPE_DESCRIPTOR_DEPTH: usize = 256;
const MAX_HOST_TYPE_DESCRIPTOR_NODES: usize = 1 << 16;
const MAX_HOST_FUNCTION_PARAMETERS: usize = u16::MAX as usize;
const MAX_HOST_VALUE_SLOTS: usize = u16::MAX as usize;
const MAX_HOST_FUNCTION_SLOTS: usize = u16::MAX as usize + 1;
const MAX_HOST_LAYOUT_ENTRIES: usize = MAX_HOST_VALUE_SLOTS + MAX_HOST_FUNCTION_SLOTS;

/// Hostが見せる名前を、**順序・名前・型まで含めて**固定したdescriptor。
///
/// 値の順序はVMのhost value slotに、呼べる名前の順序は[`HostFns`]のindexに
/// それぞれ対応する。二つを別々に並べ直さず、登録時の一列をそのまま保持する。
#[derive(Clone, Debug, PartialEq)]
pub struct HostLayout {
    entries: Arc<[(String, HostItem)]>,
}

impl HostLayout {
    /// Layoutを作る。
    ///
    /// 同じ名前、VMが表現できない関数引数個数、検査上限を越える型記述子は拒む。
    /// 型記述子の深さ・要素数をここで一度だけ制限するため、prepare/run側は
    /// 公開入力に対して再帰的な`Clone`や`Eq`を無制限には行わない。
    pub fn new(entries: Vec<(String, HostItem)>) -> Result<Self, HostLayoutError> {
        let value_count = entries
            .iter()
            .filter(|(_, item)| matches!(item, HostItem::Value(_)))
            .count();
        let function_count = entries.len() - value_count;
        let cardinality_error = if entries.len() > MAX_HOST_LAYOUT_ENTRIES {
            Some(HostLayoutError::TooManyEntries {
                limit: MAX_HOST_LAYOUT_ENTRIES,
                actual: entries.len(),
            })
        } else if value_count > MAX_HOST_VALUE_SLOTS {
            Some(HostLayoutError::TooManyValueSlots {
                limit: MAX_HOST_VALUE_SLOTS,
                actual: value_count,
            })
        } else if function_count > MAX_HOST_FUNCTION_SLOTS {
            Some(HostLayoutError::TooManyFunctionSlots {
                limit: MAX_HOST_FUNCTION_SLOTS,
                actual: function_count,
            })
        } else {
            None
        };
        if let Some(error) = cardinality_error {
            drop_host_entries_iteratively(entries);
            return Err(error);
        }
        let duplicate = {
            let mut names = HashSet::with_capacity(entries.len());
            entries.iter().find_map(|(name, _)| {
                (!names.insert(name.as_str())).then(|| HostLayoutError::DuplicateName(name.clone()))
            })
        };
        if let Some(error) = duplicate {
            drop_host_entries_iteratively(entries);
            return Err(error);
        }
        let mut visited = 0usize;
        let invalid = entries.iter().find_map(|(name, item)| {
            let error = match item {
                HostItem::Value(ty) => validate_host_type_descriptor(ty, &mut visited).err(),
                HostItem::Fn(signature) => {
                    if signature.params.len() > MAX_HOST_FUNCTION_PARAMETERS {
                        Some(HostTypeDescriptorError::TooManyFunctionParameters {
                            actual: signature.params.len(),
                        })
                    } else {
                        signature
                            .params
                            .iter()
                            .find_map(|ty| validate_host_type_descriptor(ty, &mut visited).err())
                            .or_else(|| {
                                signature.ret.as_ref().and_then(|ty| {
                                    validate_host_type_descriptor(ty, &mut visited).err()
                                })
                            })
                    }
                }
            };
            error.map(|error| host_type_descriptor_layout_error(name, error))
        });
        if let Some(error) = invalid {
            drop_host_entries_iteratively(entries);
            return Err(error);
        }
        Ok(Self {
            entries: Arc::from(entries),
        })
    }

    /// 固定した列。返したsliceからlayoutを変更することはできない。
    pub fn entries(&self) -> &[(String, HostItem)] {
        &self.entries
    }

    /// 値として見せる名前の数。
    pub fn value_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|(_, item)| matches!(item, HostItem::Value(_)))
            .count()
    }

    /// 呼べる名前の数。
    pub fn function_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|(_, item)| matches!(item, HostItem::Fn(_)))
            .count()
    }

    fn shares_identity(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.entries, &other.entries)
    }

    fn value_entries(&self) -> impl Iterator<Item = (&str, &ValueType)> {
        self.entries.iter().filter_map(|(name, item)| match item {
            HostItem::Value(ty) => Some((name.as_str(), ty)),
            HostItem::Fn(_) => None,
        })
    }

    fn function_entries(&self) -> impl Iterator<Item = (&str, &HostSig)> {
        self.entries.iter().filter_map(|(name, item)| match item {
            HostItem::Value(_) => None,
            HostItem::Fn(signature) => Some((name.as_str(), signature)),
        })
    }
}

/// [`HostLayout`]そのものが不正である。
#[derive(Clone, Debug, PartialEq)]
pub enum HostLayoutError {
    DuplicateName(String),
    TooManyEntries {
        limit: usize,
        actual: usize,
    },
    TooManyValueSlots {
        limit: usize,
        actual: usize,
    },
    TooManyFunctionSlots {
        limit: usize,
        actual: usize,
    },
    TypeDescriptorTooDeep {
        name: String,
        limit: usize,
    },
    TypeDescriptorTooLarge {
        name: String,
        limit: usize,
    },
    TooManyFunctionParameters {
        name: String,
        limit: usize,
        actual: usize,
    },
}

impl std::fmt::Display for HostLayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateName(name) => write!(f, "host名 `{name}` が重複している"),
            Self::TooManyEntries { limit, actual } => {
                write!(f, "host layoutが{actual}項目あり、VM上限{limit}を越える")
            }
            Self::TooManyValueSlots { limit, actual } => {
                write!(f, "host値が{actual}個あり、VM上限{limit}を越える")
            }
            Self::TooManyFunctionSlots { limit, actual } => {
                write!(f, "host関数が{actual}個あり、VM上限{limit}を越える")
            }
            Self::TypeDescriptorTooDeep { name, limit } => {
                write!(f, "host `{name}` の型記述子が深さ上限{limit}を越える")
            }
            Self::TypeDescriptorTooLarge { name, limit } => {
                write!(f, "host `{name}` の型記述子が要素数上限{limit}を越える")
            }
            Self::TooManyFunctionParameters {
                name,
                limit,
                actual,
            } => write!(
                f,
                "host関数 `{name}` の引数が{actual}個あり、VM上限{limit}を越える"
            ),
        }
    }
}

impl std::error::Error for HostLayoutError {}

enum HostTypeDescriptorError {
    TooDeep,
    TooLarge,
    TooManyFunctionParameters { actual: usize },
}

fn validate_host_type_descriptor(
    ty: &ValueType,
    visited: &mut usize,
) -> Result<(), HostTypeDescriptorError> {
    let mut pending = vec![(ty, 0usize)];
    while let Some((ty, depth)) = pending.pop() {
        if depth > MAX_HOST_TYPE_DESCRIPTOR_DEPTH {
            return Err(HostTypeDescriptorError::TooDeep);
        }
        *visited += 1;
        if *visited > MAX_HOST_TYPE_DESCRIPTOR_NODES {
            return Err(HostTypeDescriptorError::TooLarge);
        }
        let child_depth = depth + 1;
        match ty {
            ValueType::Array(element) => pending.push((element.as_ref(), child_depth)),
            ValueType::Map(key, value) | ValueType::Hash(key, value) => {
                pending.push((key.as_ref(), child_depth));
                pending.push((value.as_ref(), child_depth));
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_runtime_host_signature(name: &str, signature: &HostSig) -> Result<(), HostLayoutError> {
    if signature.params.len() > MAX_HOST_FUNCTION_PARAMETERS {
        return Err(HostLayoutError::TooManyFunctionParameters {
            name: name.to_string(),
            limit: MAX_HOST_FUNCTION_PARAMETERS,
            actual: signature.params.len(),
        });
    }
    let mut visited = 0usize;
    for ty in &signature.params {
        validate_host_type_descriptor(ty, &mut visited)
            .map_err(|error| host_type_descriptor_layout_error(name, error))?;
    }
    if let Some(ty) = &signature.ret {
        validate_host_type_descriptor(ty, &mut visited)
            .map_err(|error| host_type_descriptor_layout_error(name, error))?;
    }
    Ok(())
}

fn host_type_descriptor_layout_error(
    name: &str,
    error: HostTypeDescriptorError,
) -> HostLayoutError {
    match error {
        HostTypeDescriptorError::TooDeep => HostLayoutError::TypeDescriptorTooDeep {
            name: name.to_string(),
            limit: MAX_HOST_TYPE_DESCRIPTOR_DEPTH,
        },
        HostTypeDescriptorError::TooLarge => HostLayoutError::TypeDescriptorTooLarge {
            name: name.to_string(),
            limit: MAX_HOST_TYPE_DESCRIPTOR_NODES,
        },
        HostTypeDescriptorError::TooManyFunctionParameters { actual } => {
            HostLayoutError::TooManyFunctionParameters {
                name: name.to_string(),
                limit: MAX_HOST_FUNCTION_PARAMETERS,
                actual,
            }
        }
    }
}

fn host_signatures_equal(left: &HostSig, right: &HostSig) -> bool {
    left.params.len() == right.params.len()
        && left
            .params
            .iter()
            .zip(&right.params)
            .all(|(left, right)| value_types_equal(left, right))
        && match (&left.ret, &right.ret) {
            (Some(left), Some(right)) => value_types_equal(left, right),
            (None, None) => true,
            _ => false,
        }
}

/// 不正layoutにも任意深さのowned `ValueType`が入るので、通常の再帰dropへ渡さない。
fn drop_host_entries_iteratively(entries: Vec<(String, HostItem)>) {
    for (_, item) in entries {
        match item {
            HostItem::Value(ty) => drop_value_type_iteratively(ty),
            HostItem::Fn(signature) => drop_host_signature_iteratively(signature),
        }
    }
}

fn drop_host_signature_iteratively(signature: HostSig) {
    for ty in signature.params {
        drop_value_type_iteratively(ty);
    }
    if let Some(ty) = signature.ret {
        drop_value_type_iteratively(ty);
    }
}

fn drop_value_type_iteratively(ty: ValueType) {
    let mut pending = vec![ty];
    while let Some(ty) = pending.pop() {
        match ty {
            ValueType::Array(element) => pending.push(*element),
            ValueType::Map(key, value) | ValueType::Hash(key, value) => {
                pending.push(*key);
                pending.push(*value);
            }
            _ => {}
        }
    }
}

/// 拒否するhost値は検証上限外でもよいので、再帰的なderive dropへ渡さない。
fn drop_value_iteratively(value: Value) {
    drop_values_iteratively(vec![value]);
}

fn drop_values_iteratively(mut pending: Vec<Value>) {
    while let Some(value) = pending.pop() {
        match value {
            Value::Array(array) => {
                let crate::value::ArrayVal { elem, mut items } = *array;
                drop_value_type_iteratively(elem);
                pending.append(&mut items);
            }
            Value::Map(map) => {
                let crate::value::MapVal { key, val, entries } = *map;
                drop_value_type_iteratively(key);
                drop_value_type_iteratively(val);
                pending.extend(entries.into_values());
            }
            Value::Hash(hash) => {
                let crate::value::HashVal {
                    key,
                    val,
                    entries,
                    index,
                } = *hash;
                drop_value_type_iteratively(key);
                drop_value_type_iteratively(val);
                drop(index);
                pending.extend(entries.into_iter().flatten().map(|(_, value)| value));
            }
            Value::Struct(value) => {
                let crate::value::StructVal { name, fields } = *value;
                drop(name);
                pending.extend(fields.into_iter().map(|(_, value)| value));
            }
            Value::Str(value) => drop(value),
            Value::U1(_)
            | Value::U8(_)
            | Value::U16(_)
            | Value::U32(_)
            | Value::I32(_)
            | Value::I64(_)
            | Value::F32(_)
            | Value::F64(_) => {}
        }
    }
}

/// Prepareのどの段で見つかったか。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrepareStage {
    Parse,
    Check,
    TypeCheck,
    Compile,
}

/// Prepare時の一件の診断。位置は入力UTF-8のbyte offsetである。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrepareDiagnostic {
    pub stage: PrepareStage,
    pub message: String,
    pub span: Span,
}

/// Prepareは一つ以上の診断をまとめて返す。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrepareError {
    diagnostics: Vec<PrepareDiagnostic>,
}

impl PrepareError {
    pub fn diagnostics(&self) -> &[PrepareDiagnostic] {
        &self.diagnostics
    }
}

impl std::fmt::Display for PrepareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (i, diagnostic) in self.diagnostics.iter().enumerate() {
            if i != 0 {
                f.write_str("; ")?;
            }
            write!(f, "{}", diagnostic.message)?;
        }
        Ok(())
    }
}

impl std::error::Error for PrepareError {}

/// Parse・検査・VM組み立てを終えた不変なtop-level program。
///
/// 内部の[`Program2`]は直接変更できない。複数の[`EmbeddingRunner`]から共有してよい。
#[derive(Debug)]
pub struct PreparedProgram {
    source: Arc<str>,
    layout: HostLayout,
    type_schema: Arc<ProgramTypeSchema>,
    program: Program2,
}

impl PreparedProgram {
    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn layout(&self) -> &HostLayout {
        &self.layout
    }

    /// 値host slotのうち、読まれるもの。
    pub fn host_reads(&self) -> Vec<bool> {
        self.program.host_reads()
    }

    /// 値host slotのうち、書かれるもの。
    pub fn host_writes(&self) -> Vec<bool> {
        self.program.host_writes()
    }

    /// 値host slotのうち、読み書きのいずれかがあるもの。
    pub fn host_used(&self) -> Vec<bool> {
        self.program.host_used()
    }

    /// 値host slotが定数添字だけを触るなら、その添字を返す。
    /// `None`は丸ごとの値が要ることを表す。
    pub fn host_touched(&self, value_index: usize) -> Option<Vec<i128>> {
        self.program.host_touched(value_index)
    }

    /// 低水準経路へ渡す値を型・個数まで検査し、layout identityと結ぶ。
    ///
    /// 返したtokenは同じ[`HostLayout`]からprepareしたprogramでだけ走らせられる。
    /// 名前やlayout列をrunごとに再構成しない。
    pub fn host_values(&self, values: Vec<Value>) -> Result<HostValues, HostValuesError> {
        if let Err(error) = self.validate_host_values(&values, None) {
            drop_values_iteratively(values);
            return Err(error);
        }
        Ok(HostValues {
            layout: self.layout.clone(),
            type_schema: self.type_schema.clone(),
            values,
        })
    }

    fn validate_host_values(
        &self,
        values: &[Value],
        binding_state: Option<&VmBindingState>,
    ) -> Result<(), HostValuesError> {
        let expected_count = self.layout.value_count();
        if values.len() != expected_count {
            return Err(HostValuesError::WrongCount {
                expected: expected_count,
                actual: values.len(),
            });
        }
        for (index, (value, (name, ty))) in
            values.iter().zip(self.layout.value_entries()).enumerate()
        {
            if binding_state.is_some_and(|state| !state.materialized(index)) {
                continue;
            }
            let validation = match binding_state.and_then(|state| state.partial_indices(index)) {
                Some(indices) => validate_partial_array(value, ty, indices, &self.type_schema),
                None => validate_host_value(value, ty, &self.type_schema),
            };
            if let Err(error) = validation {
                return Err(match error {
                    ValueValidationError::WrongOuterType => HostValuesError::WrongType {
                        index,
                        name: name.to_string(),
                        expected: ty.clone(),
                        actual: host_value_kind(value),
                    },
                    ValueValidationError::Malformed(reason) => HostValuesError::MalformedValue {
                        index,
                        name: name.to_string(),
                        expected: ty.clone(),
                        reason,
                    },
                });
            }
        }
        Ok(())
    }

    /// 呼べるhost名を実体・順序・署名まで検査し、layout identityと結ぶ。
    ///
    /// 返したtoken自身が[`HostFns`]のindex dispatcherになる。無関係なdispatcherを
    /// 実行時に差し込む経路は公開しない。
    pub fn host_functions<'a>(
        &self,
        slots: Vec<HostFunctionSlot<'a>>,
    ) -> Result<HostFunctions<'a>, HostFunctionsError> {
        let expected_count = self.layout.function_count();
        if slots.len() != expected_count {
            return Err(HostFunctionsError::WrongCount {
                expected: expected_count,
                actual: slots.len(),
            });
        }
        for (index, (slot, (expected_name, expected_signature))) in
            slots.iter().zip(self.layout.function_entries()).enumerate()
        {
            if slot.name != expected_name {
                return Err(HostFunctionsError::WrongName {
                    index,
                    expected: expected_name.to_string(),
                    actual: slot.name.to_string(),
                });
            }
            let actual = slot.function.sig();
            if let Err(error) = validate_runtime_host_signature(expected_name, &actual) {
                drop_host_signature_iteratively(actual);
                return Err(HostFunctionsError::InvalidSignature {
                    index,
                    name: expected_name.to_string(),
                    error,
                });
            }
            if !host_signatures_equal(&actual, expected_signature) {
                return Err(HostFunctionsError::WrongSignature {
                    index,
                    name: expected_name.to_string(),
                    expected: expected_signature.clone(),
                    actual,
                });
            }
        }
        Ok(HostFunctions {
            layout: self.layout.clone(),
            type_schema: self.type_schema.clone(),
            functions: slots.into_iter().map(|slot| slot.function).collect(),
            signatures: self
                .layout
                .function_entries()
                .map(|(_, signature)| signature.clone())
                .collect(),
            contract_error: None,
        })
    }
}

/// 一度だけparse・check・type-check・VM compileする。
pub fn prepare(source: &str, layout: &HostLayout) -> Result<PreparedProgram, PrepareError> {
    let parsed = crate::parser::parse(source).map_err(|error| PrepareError {
        diagnostics: vec![PrepareDiagnostic {
            stage: PrepareStage::Parse,
            message: error.msg,
            span: error.span,
        }],
    })?;

    let mut diagnostics: Vec<PrepareDiagnostic> =
        crate::check::check_with_host(&parsed, layout.entries())
            .into_iter()
            .map(|error| PrepareDiagnostic {
                stage: PrepareStage::Check,
                message: error.msg,
                span: error.span,
            })
            .collect();
    diagnostics.extend(
        crate::types::check_types_with_host(&parsed, layout.entries())
            .into_iter()
            .map(|error| PrepareDiagnostic {
                stage: PrepareStage::TypeCheck,
                message: error.msg,
                span: error.span,
            }),
    );
    if !diagnostics.is_empty() {
        return Err(PrepareError { diagnostics });
    }

    let program =
        crate::vm::compile_with_host(&parsed, layout.entries()).map_err(|error| PrepareError {
            diagnostics: vec![PrepareDiagnostic {
                stage: PrepareStage::Compile,
                message: error.msg,
                span: error.span,
            }],
        })?;
    let type_schema = ProgramTypeSchema::for_layout(layout, &program).map_err(|error| {
        let message = match error {
            ProgramTypeSchemaError::TooDeep => format!(
                "host layoutから到達する名付き型schemaが深さ上限{MAX_HOST_TYPE_DESCRIPTOR_DEPTH}を越える"
            ),
            ProgramTypeSchemaError::TooLarge => format!(
                "host layoutから到達する名付き型schemaが要素数上限{MAX_HOST_TYPE_DESCRIPTOR_NODES}を越える"
            ),
            ProgramTypeSchemaError::UnsupportedF80 => {
                "VMのhost layoutから到達する名付き型schemaにはF80を置けない".into()
            }
        };
        PrepareError {
            diagnostics: vec![PrepareDiagnostic {
                stage: PrepareStage::Compile,
                message,
                span: Span::NONE,
            }],
        }
    })?;
    if let Some(name) = type_schema.unresolved.first() {
        return Err(PrepareError {
            diagnostics: vec![PrepareDiagnostic {
                stage: PrepareStage::Compile,
                message: format!(
                    "host layoutから到達する名付き型 `{name}` はprogram内に定義が無い"
                ),
                span: Span::NONE,
            }],
        });
    }
    let type_schema = intern_type_schema(type_schema);
    Ok(PreparedProgram {
        source: Arc::from(source),
        layout: layout.clone(),
        type_schema,
        program,
    })
}

/// 型・個数を検査済みのhost値。
///
/// `values`はrun後のwritebackで置き換わる。同じtokenを次のrunへそのまま渡せる。
#[derive(Clone, Debug)]
pub struct HostValues {
    layout: HostLayout,
    type_schema: Arc<ProgramTypeSchema>,
    values: Vec<Value>,
}

impl HostValues {
    pub fn values(&self) -> &[Value] {
        &self.values
    }

    pub fn into_values(mut self) -> Vec<Value> {
        std::mem::take(&mut self.values)
    }
}

impl Drop for HostValues {
    fn drop(&mut self) {
        drop_values_iteratively(std::mem::take(&mut self.values));
    }
}

/// 診断用の、再帰的な型記述子を含まないhost値の外形。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostValueKind {
    U1,
    U8,
    U16,
    U32,
    I32,
    I64,
    F32,
    F64,
    Str,
    Array,
    Map,
    Hash,
    Struct,
}

/// 低水準host値がprepare済みlayoutに合わない。
#[derive(Clone, Debug, PartialEq)]
pub enum HostValuesError {
    WrongCount {
        expected: usize,
        actual: usize,
    },
    WrongType {
        index: usize,
        name: String,
        expected: ValueType,
        actual: HostValueKind,
    },
    MalformedValue {
        index: usize,
        name: String,
        expected: ValueType,
        reason: String,
    },
}

impl std::fmt::Display for HostValuesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongCount { expected, actual } => {
                write!(f, "host値は{expected}個必要だが{actual}個だった")
            }
            Self::WrongType {
                index,
                name,
                expected,
                actual,
            } => write!(
                f,
                "host値{index} (`{name}`) の型が合わない（{expected:?} が必要、{actual:?}）"
            ),
            Self::MalformedValue {
                index,
                name,
                expected,
                reason,
            } => write!(
                f,
                "host値{index} (`{name}`) は{expected:?}として不正である（{reason}）"
            ),
        }
    }
}

impl std::error::Error for HostValuesError {}

/// Prepare時のfunction subsequenceへ結ぶ実物のhost関数。
pub struct HostFunctionSlot<'a> {
    name: &'a str,
    function: &'a mut dyn HostFn,
}

impl<'a> HostFunctionSlot<'a> {
    pub fn new(name: &'a str, function: &'a mut dyn HostFn) -> Self {
        Self { name, function }
    }
}

/// 検査済みのhost関数群。番号はprepare時のfunction subsequenceと同じ。
pub struct HostFunctions<'a> {
    layout: HostLayout,
    type_schema: Arc<ProgramTypeSchema>,
    functions: Vec<&'a mut dyn HostFn>,
    signatures: Vec<HostSig>,
    contract_error: Option<String>,
}

impl HostFns for HostFunctions<'_> {
    fn call(&mut self, index: u16, args: &[Value]) -> Option<Value> {
        let index = index as usize;
        let result = self.functions.get_mut(index)?.call(args);
        match checked_host_function_result(
            result,
            self.signatures.get(index)?.ret.as_ref(),
            &self.type_schema,
        ) {
            Ok(result) => result,
            Err(message) => {
                self.contract_error.get_or_insert(message);
                None
            }
        }
    }

    fn take_contract_error(&mut self) -> Option<String> {
        self.contract_error.take()
    }
}

/// Host関数の名前・順序・署名がprepare時layoutに合わない。
#[derive(Clone, Debug, PartialEq)]
pub enum HostFunctionsError {
    WrongCount {
        expected: usize,
        actual: usize,
    },
    WrongName {
        index: usize,
        expected: String,
        actual: String,
    },
    InvalidSignature {
        index: usize,
        name: String,
        error: HostLayoutError,
    },
    WrongSignature {
        index: usize,
        name: String,
        expected: HostSig,
        actual: HostSig,
    },
}

impl std::fmt::Display for HostFunctionsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongCount { expected, actual } => {
                write!(f, "host関数は{expected}個必要だが{actual}個だった")
            }
            Self::WrongName {
                index,
                expected,
                actual,
            } => write!(
                f,
                "host関数{index}の名前が違う（`{expected}` が必要、`{actual}`）"
            ),
            Self::InvalidSignature { index, name, error } => {
                write!(
                    f,
                    "host関数{index} (`{name}`) の署名が不正である（{error}）"
                )
            }
            Self::WrongSignature {
                index,
                name,
                expected,
                actual,
            } => write!(
                f,
                "host関数{index} (`{name}`) の署名が違う（{expected:?} が必要、{actual:?}）"
            ),
        }
    }
}

impl std::error::Error for HostFunctionsError {}

/// 高水準の実行時layoutを一列のまま表す。
///
/// Function slotは署名だけでなく、実際に呼ぶ[`HostFn`]を持つ。prepare時layoutとの
/// 照合に使った実物を、そのままfunction subsequenceのindexでdispatchする。
pub enum HostSlot<'a> {
    Value {
        name: &'a str,
        binding: &'a mut dyn HostBinding,
    },
    Function {
        name: &'a str,
        function: &'a mut dyn HostFn,
    },
}

impl<'a> HostSlot<'a> {
    pub fn value(name: &'a str, binding: &'a mut dyn HostBinding) -> Self {
        Self::Value { name, binding }
    }

    pub fn function(name: &'a str, function: &'a mut dyn HostFn) -> Self {
        Self::Function { name, function }
    }
}

/// 高水準runの前に見つかるlayout不一致、またはVM実行時エラー。
#[derive(Clone, Debug)]
pub enum EmbeddingRunError {
    InvalidLayout(HostLayoutError),
    LayoutMismatch {
        expected: HostLayout,
        actual: HostLayout,
    },
    HostValues(HostValuesError),
    FunctionsRequired {
        expected: usize,
    },
    Runtime(RtErr),
}

impl std::fmt::Display for EmbeddingRunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLayout(error) => error.fmt(f),
            Self::LayoutMismatch { .. } => {
                f.write_str("prepare時と実行時のhost layoutが一致しない")
            }
            Self::HostValues(error) => error.fmt(f),
            Self::FunctionsRequired { expected } => {
                write!(f, "このprogramにはhost関数が{expected}個必要である")
            }
            Self::Runtime(error) => error.msg.fmt(f),
        }
    }
}

impl std::error::Error for EmbeddingRunError {}

/// 低水準の検査済みtokenがprepare済みprogramに合わない。
#[derive(Clone, Debug, PartialEq)]
pub enum PreparedRunError {
    HostValuesLayoutMismatch,
    HostValuesTypeSchemaMismatch,
    HostFunctionsLayoutMismatch,
    HostFunctionsTypeSchemaMismatch,
    FunctionsRequired { expected: usize },
}

impl std::fmt::Display for PreparedRunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HostValuesLayoutMismatch => {
                f.write_str("host値tokenが別のhost layoutに属している")
            }
            Self::HostValuesTypeSchemaMismatch => {
                f.write_str("host値tokenの名付き型schemaがprogramと異なる")
            }
            Self::HostFunctionsLayoutMismatch => {
                f.write_str("host関数tokenが別のhost layoutに属している")
            }
            Self::HostFunctionsTypeSchemaMismatch => {
                f.write_str("host関数tokenの名付き型schemaがprogramと異なる")
            }
            Self::FunctionsRequired { expected } => {
                write!(f, "このprogramにはhost関数が{expected}個必要である")
            }
        }
    }
}

impl std::error::Error for PreparedRunError {}

/// 一度きりでない埋め込み実行のためのrunner。
#[derive(Default)]
pub struct EmbeddingRunner {
    runner: Runner,
}

impl EmbeddingRunner {
    pub fn new() -> Self {
        Self::default()
    }

    /// 型検査済みの値tokenを走らせる低水準経路。
    ///
    /// Host名や型をrunごとに組み直さない。runtime errorでも`host_values`には
    /// そこまでの変更が残る（C-2 / S-22）。
    pub fn run_values(
        &mut self,
        program: &PreparedProgram,
        host_values: &mut HostValues,
        host_functions: &mut HostFunctions<'_>,
    ) -> Result<Result<Eval, RtErr>, PreparedRunError> {
        if !program.layout.shares_identity(&host_values.layout) {
            return Err(PreparedRunError::HostValuesLayoutMismatch);
        }
        if !Arc::ptr_eq(&program.type_schema, &host_values.type_schema) {
            return Err(PreparedRunError::HostValuesTypeSchemaMismatch);
        }
        if !program.layout.shares_identity(&host_functions.layout) {
            return Err(PreparedRunError::HostFunctionsLayoutMismatch);
        }
        if !Arc::ptr_eq(&program.type_schema, &host_functions.type_schema) {
            return Err(PreparedRunError::HostFunctionsTypeSchemaMismatch);
        }
        let values = std::mem::take(&mut host_values.values);
        let (result, after) =
            self.runner
                .run_with_writeback(&program.program, values, host_functions);
        host_values.values = after;
        Ok(result)
    }

    pub fn run_values_without_functions(
        &mut self,
        program: &PreparedProgram,
        host_values: &mut HostValues,
    ) -> Result<Result<Eval, RtErr>, PreparedRunError> {
        let expected = program.layout.function_count();
        if expected != 0 {
            return Err(PreparedRunError::FunctionsRequired { expected });
        }
        let mut none = NoHostFns;
        if !program.layout.shares_identity(&host_values.layout) {
            return Err(PreparedRunError::HostValuesLayoutMismatch);
        }
        if !Arc::ptr_eq(&program.type_schema, &host_values.type_schema) {
            return Err(PreparedRunError::HostValuesTypeSchemaMismatch);
        }
        let values = std::mem::take(&mut host_values.values);
        let (result, after) = self
            .runner
            .run_with_writeback(&program.program, values, &mut none);
        host_values.values = after;
        Ok(result)
    }

    /// [`HostBinding`]を直接使う高水準経路。
    ///
    /// 実行時slot列をprepare時layoutと完全照合してから値を読む。layout不一致では
    /// `read`も`write`も呼ばない。S-15/S-22の部分読み書きとerror writebackは
    /// [`crate::host::Host`]のVM経路と同じ実装を使う。
    pub fn run(
        &mut self,
        program: &PreparedProgram,
        slots: &mut [HostSlot<'_>],
    ) -> Result<Eval, EmbeddingRunError> {
        let mut actual_entries: Vec<(String, HostItem)> = Vec::with_capacity(slots.len());
        let mut function_indices = Vec::with_capacity(program.layout.function_count());
        for (index, slot) in slots.iter().enumerate() {
            actual_entries.push(match slot {
                HostSlot::Value { name, binding } => {
                    ((*name).to_string(), HostItem::Value(binding.type_of()))
                }
                HostSlot::Function { name, function } => {
                    function_indices.push(index);
                    ((*name).to_string(), HostItem::Fn(function.sig()))
                }
            });
        }
        let actual = HostLayout::new(actual_entries).map_err(EmbeddingRunError::InvalidLayout)?;
        if actual != program.layout {
            return Err(EmbeddingRunError::LayoutMismatch {
                expected: program.layout.clone(),
                actual,
            });
        }

        let (values, mut state) = vm_binding_values_uncloned(
            &program.program,
            slots.iter().filter_map(|slot| match slot {
                HostSlot::Value { binding, .. } => Some(&**binding),
                HostSlot::Function { .. } => None,
            }),
        );
        if let Err(error) = program.validate_host_values(&values, Some(&state)) {
            drop_values_iteratively(values);
            return Err(EmbeddingRunError::HostValues(error));
        }
        state.capture_before(&values);
        let (result, after) = {
            let mut dispatch = SlotFunctionDispatch {
                slots,
                indices: function_indices,
                signatures: program
                    .layout
                    .function_entries()
                    .map(|(_, signature)| signature.clone())
                    .collect(),
                type_schema: &program.type_schema,
                contract_error: None,
            };
            self.runner
                .run_with_writeback(&program.program, values, &mut dispatch)
        };
        vm_binding_writeback(
            slots.iter_mut().filter_map(|slot| match slot {
                HostSlot::Value { binding, .. } => Some(&mut **binding),
                HostSlot::Function { .. } => None,
            }),
            after,
            state,
        );
        result.map_err(EmbeddingRunError::Runtime)
    }

    pub fn run_without_functions(
        &mut self,
        program: &PreparedProgram,
        slots: &mut [HostSlot<'_>],
    ) -> Result<Eval, EmbeddingRunError> {
        let expected = program.layout.function_count();
        if expected != 0 {
            return Err(EmbeddingRunError::FunctionsRequired { expected });
        }
        self.run(program, slots)
    }
}

struct SlotFunctionDispatch<'slots, 'binding, 'schema> {
    slots: &'slots mut [HostSlot<'binding>],
    indices: Vec<usize>,
    signatures: Vec<HostSig>,
    type_schema: &'schema ProgramTypeSchema,
    contract_error: Option<String>,
}

impl HostFns for SlotFunctionDispatch<'_, '_, '_> {
    fn call(&mut self, index: u16, args: &[Value]) -> Option<Value> {
        let index = index as usize;
        let slot = *self.indices.get(index)?;
        let HostSlot::Function { function, .. } = self.slots.get_mut(slot)? else {
            return None;
        };
        let result = function.call(args);
        match checked_host_function_result(
            result,
            self.signatures.get(index)?.ret.as_ref(),
            self.type_schema,
        ) {
            Ok(result) => result,
            Err(message) => {
                self.contract_error.get_or_insert(message);
                None
            }
        }
    }

    fn take_contract_error(&mut self) -> Option<String> {
        self.contract_error.take()
    }
}

fn host_value_kind(value: &Value) -> HostValueKind {
    match value {
        Value::U1(_) => HostValueKind::U1,
        Value::U8(_) => HostValueKind::U8,
        Value::U16(_) => HostValueKind::U16,
        Value::U32(_) => HostValueKind::U32,
        Value::I32(_) => HostValueKind::I32,
        Value::I64(_) => HostValueKind::I64,
        Value::F32(_) => HostValueKind::F32,
        Value::F64(_) => HostValueKind::F64,
        Value::Str(_) => HostValueKind::Str,
        Value::Array(_) => HostValueKind::Array,
        Value::Map(_) => HostValueKind::Map,
        Value::Hash(_) => HostValueKind::Hash,
        Value::Struct(_) => HostValueKind::Struct,
    }
}

fn value_matches_type(
    value: &Value,
    expected: &ValueType,
    descriptor_nodes: &mut usize,
) -> Result<bool, ValueValidationError> {
    match value {
        Value::Array(actual) => {
            validate_actual_type_descriptor(&actual.elem, descriptor_nodes)?;
        }
        Value::Map(actual) => {
            validate_actual_type_descriptor(&actual.key, descriptor_nodes)?;
            validate_actual_type_descriptor(&actual.val, descriptor_nodes)?;
        }
        Value::Hash(actual) => {
            validate_actual_type_descriptor(&actual.key, descriptor_nodes)?;
            validate_actual_type_descriptor(&actual.val, descriptor_nodes)?;
        }
        _ => {}
    }
    Ok(match (value, expected) {
        (Value::U1(_), ValueType::U1)
        | (Value::U8(_), ValueType::U8)
        | (Value::U16(_), ValueType::U16)
        | (Value::U32(_), ValueType::U32)
        | (Value::I32(_), ValueType::I32)
        | (Value::I64(_), ValueType::I64)
        | (Value::F32(_), ValueType::F32)
        | (Value::F64(_), ValueType::F64)
        | (Value::Str(_), ValueType::Str) => true,
        (Value::Array(actual), ValueType::Array(expected)) => {
            value_types_equal(&actual.elem, expected)
        }
        (Value::Map(actual), ValueType::Map(expected_key, expected_value)) => {
            value_types_equal(&actual.key, expected_key)
                && value_types_equal(&actual.val, expected_value)
        }
        (Value::Hash(actual), ValueType::Hash(expected_key, expected_value)) => {
            value_types_equal(&actual.key, expected_key)
                && value_types_equal(&actual.val, expected_value)
        }
        (Value::Struct(actual), ValueType::Named(expected)) => &actual.name == expected,
        _ => false,
    })
}

fn validate_actual_type_descriptor(
    ty: &ValueType,
    descriptor_nodes: &mut usize,
) -> Result<(), ValueValidationError> {
    validate_host_type_descriptor(ty, descriptor_nodes).map_err(|error| {
        let reason = match error {
            HostTypeDescriptorError::TooDeep => {
                format!("値が持つ型記述子の深さが検査上限{MAX_HOST_TYPE_DESCRIPTOR_DEPTH}を越える")
            }
            HostTypeDescriptorError::TooLarge => format!(
                "値が持つ型記述子の要素数が検査上限{MAX_HOST_TYPE_DESCRIPTOR_NODES}を越える"
            ),
            HostTypeDescriptorError::TooManyFunctionParameters { .. } => {
                "値の型記述子に関数署名は置けない".into()
            }
        };
        ValueValidationError::Malformed(reason)
    })
}

fn value_types_equal(left: &ValueType, right: &ValueType) -> bool {
    let mut pending = vec![(left, right)];
    while let Some((left, right)) = pending.pop() {
        match (left, right) {
            (ValueType::U1, ValueType::U1)
            | (ValueType::U8, ValueType::U8)
            | (ValueType::U16, ValueType::U16)
            | (ValueType::U32, ValueType::U32)
            | (ValueType::I32, ValueType::I32)
            | (ValueType::I64, ValueType::I64)
            | (ValueType::F32, ValueType::F32)
            | (ValueType::F64, ValueType::F64)
            | (ValueType::F80, ValueType::F80)
            | (ValueType::Str, ValueType::Str) => {}
            (ValueType::Named(left), ValueType::Named(right)) if left == right => {}
            (ValueType::Array(left), ValueType::Array(right)) => {
                pending.push((left, right));
            }
            (ValueType::Map(left_key, left_value), ValueType::Map(right_key, right_value))
            | (ValueType::Hash(left_key, left_value), ValueType::Hash(right_key, right_value)) => {
                pending.push((left_key, right_key));
                pending.push((left_value, right_value));
            }
            _ => return false,
        }
    }
    true
}

const MAX_HOST_VALUE_VALIDATION_DEPTH: usize = MAX_HOST_TYPE_DESCRIPTOR_DEPTH;
const MAX_HOST_VALUE_VALIDATION_NODES: usize = MAX_HOST_VALUE_NODES;

enum ValueValidationError {
    WrongOuterType,
    Malformed(String),
}

fn checked_host_function_result(
    result: Option<Value>,
    expected: Option<&ValueType>,
    schema: &ProgramTypeSchema,
) -> Result<Option<Value>, String> {
    let Some(value) = result else {
        // `None`は従来どおりparadox。retがSomeでも言語意味を変えない。
        return Ok(None);
    };
    let Some(expected) = expected else {
        drop_value_iteratively(value);
        return Err("host関数が返り値無しを宣言しながら値を返した".into());
    };
    match validate_host_value(&value, expected, schema) {
        Ok(()) => Ok(Some(value)),
        Err(ValueValidationError::WrongOuterType) => {
            let actual = host_value_kind(&value);
            drop_value_iteratively(value);
            Err(format!(
                "host関数の返値型が合わない（{expected:?} が必要、{actual:?}）"
            ))
        }
        Err(ValueValidationError::Malformed(reason)) => {
            drop_value_iteratively(value);
            Err(format!("host関数の返値が不正である（{reason}）"))
        }
    }
}

fn validate_host_value(
    value: &Value,
    expected: &ValueType,
    schema: &ProgramTypeSchema,
) -> Result<(), ValueValidationError> {
    let mut descriptor_nodes = 0;
    if !value_matches_type(value, expected, &mut descriptor_nodes)? {
        return Err(ValueValidationError::WrongOuterType);
    }
    validate_finite_scalar(value)?;
    if value_type_is_leaf(expected) {
        return Ok(());
    }
    validate_value_tasks(vec![(value, expected, 0, true)], schema, descriptor_nodes)
}

/// S-15の部分arrayは、hostから読んだ要素だけを検査する。
/// それ以外はdummyでありVMから観測されない。
fn validate_partial_array(
    value: &Value,
    expected: &ValueType,
    indices: &[i128],
    schema: &ProgramTypeSchema,
) -> Result<(), ValueValidationError> {
    let mut descriptor_nodes = 0;
    if !value_matches_type(value, expected, &mut descriptor_nodes)? {
        return Err(ValueValidationError::WrongOuterType);
    }
    let (Value::Array(actual), ValueType::Array(element)) = (value, expected) else {
        return Err(ValueValidationError::Malformed(
            "部分読みは配列にしか使えない".into(),
        ));
    };
    let mut tasks = Vec::with_capacity(indices.len().min(actual.items.len()));
    for &index in indices {
        let Ok(index) = usize::try_from(index) else {
            continue;
        };
        if let Some(value) = actual.items.get(index) {
            tasks.push((value, element.as_ref(), 1, false));
        }
    }
    validate_value_tasks(tasks, schema, descriptor_nodes)
}

fn validate_value_tasks<'value, 'schema>(
    mut tasks: Vec<(&'value Value, &'schema ValueType, usize, bool)>,
    schema: &'schema ProgramTypeSchema,
    mut descriptor_nodes: usize,
) -> Result<(), ValueValidationError> {
    let mut visited = 0usize;
    while let Some((value, expected, depth, type_already_checked)) = tasks.pop() {
        visited += 1;
        if visited > MAX_HOST_VALUE_VALIDATION_NODES {
            return Err(ValueValidationError::Malformed(format!(
                "値の要素数が検査上限{MAX_HOST_VALUE_VALIDATION_NODES}を越える"
            )));
        }
        if depth > MAX_HOST_VALUE_VALIDATION_DEPTH {
            return Err(ValueValidationError::Malformed(format!(
                "値の深さが検査上限{MAX_HOST_VALUE_VALIDATION_DEPTH}を越える"
            )));
        }
        if !type_already_checked && !value_matches_type(value, expected, &mut descriptor_nodes)? {
            return Err(ValueValidationError::Malformed(format!(
                "内部値の型が合わない（{expected:?} が必要、{:?}）",
                host_value_kind(value)
            )));
        }
        validate_finite_scalar(value)?;
        let child_depth = depth + 1;
        match (value, expected) {
            (Value::Array(actual), ValueType::Array(element)) => {
                ensure_validation_room(visited, tasks.len(), actual.items.len())?;
                tasks.extend(
                    actual
                        .items
                        .iter()
                        .map(|value| (value, element.as_ref(), child_depth, false)),
                );
            }
            (Value::Map(actual), ValueType::Map(key_type, value_type)) => {
                ensure_validation_room(visited, tasks.len(), actual.entries.len())?;
                for (key, value) in &actual.entries {
                    if !map_key_matches_type(key, key_type) {
                        return Err(ValueValidationError::Malformed(format!(
                            "mapの鍵{key:?}が宣言型{key_type:?}に合わない"
                        )));
                    }
                    tasks.push((value, value_type.as_ref(), child_depth, false));
                }
            }
            (Value::Hash(actual), ValueType::Hash(key_type, value_type)) => {
                let live = actual
                    .entries
                    .iter()
                    .filter(|entry| entry.is_some())
                    .count();
                if live != actual.index.len() {
                    return Err(ValueValidationError::Malformed(
                        "hashのentriesとindexの個数が合わない".into(),
                    ));
                }
                ensure_validation_room(visited, tasks.len(), live)?;
                for (position, entry) in actual.entries.iter().enumerate() {
                    let Some((key, value)) = entry else {
                        continue;
                    };
                    if actual.index.get(key) != Some(&position) {
                        return Err(ValueValidationError::Malformed(
                            "hashのentryからindexへの対応が壊れている".into(),
                        ));
                    }
                    if !map_key_matches_type(key, key_type) {
                        return Err(ValueValidationError::Malformed(format!(
                            "hashの鍵{key:?}が宣言型{key_type:?}に合わない"
                        )));
                    }
                    tasks.push((value, value_type.as_ref(), child_depth, false));
                }
                for (key, &position) in &actual.index {
                    if actual
                        .entries
                        .get(position)
                        .and_then(Option::as_ref)
                        .map(|(entry_key, _)| entry_key)
                        != Some(key)
                    {
                        return Err(ValueValidationError::Malformed(
                            "hashのindexからentryへの対応が壊れている".into(),
                        ));
                    }
                }
            }
            (Value::Struct(actual), ValueType::Named(name)) => {
                let struct_fields = schema.struct_fields(name).filter(|fields| {
                    fields.len() == actual.fields.len()
                        && fields
                            .iter()
                            .zip(&actual.fields)
                            .all(|(expected, actual)| expected.1 == actual.0)
                });
                let wrap_base = schema.wrap_base(name).filter(|_| {
                    actual.fields.len() == 1
                        && actual
                            .fields
                            .first()
                            .is_some_and(|field| field.0.is_empty())
                });
                if let Some(fields) = struct_fields {
                    ensure_validation_room(visited, tasks.len(), fields.len())?;
                    tasks.extend(
                        fields
                            .iter()
                            .zip(&actual.fields)
                            .map(|(expected, actual)| (&actual.1, &expected.2, child_depth, false)),
                    );
                } else if let Some(base) = wrap_base {
                    ensure_validation_room(visited, tasks.len(), 1)?;
                    tasks.push((&actual.fields[0].1, base, child_depth, false));
                } else {
                    return Err(ValueValidationError::Malformed(format!(
                        "名付き型`{name}`の欄名・順序・個数がschemaに合わない"
                    )));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn ensure_validation_room(
    visited: usize,
    pending: usize,
    additional: usize,
) -> Result<(), ValueValidationError> {
    if visited.saturating_add(pending).saturating_add(additional) > MAX_HOST_VALUE_VALIDATION_NODES
    {
        return Err(ValueValidationError::Malformed(format!(
            "値の要素数が検査上限{MAX_HOST_VALUE_VALIDATION_NODES}を越える"
        )));
    }
    Ok(())
}

fn value_type_is_leaf(ty: &ValueType) -> bool {
    matches!(
        ty,
        ValueType::U1
            | ValueType::U8
            | ValueType::U16
            | ValueType::U32
            | ValueType::I32
            | ValueType::I64
            | ValueType::F32
            | ValueType::F64
            | ValueType::Str
    )
}

fn validate_finite_scalar(value: &Value) -> Result<(), ValueValidationError> {
    match value {
        Value::F32(value) if !value.is_finite() => Err(ValueValidationError::Malformed(
            "f32のNaNと無限大はVaak値にできない".into(),
        )),
        Value::F64(value) if !value.is_finite() => Err(ValueValidationError::Malformed(
            "f64のNaNと無限大はVaak値にできない".into(),
        )),
        _ => Ok(()),
    }
}

fn map_key_matches_type(key: &MapKey, expected: &ValueType) -> bool {
    match (key, expected) {
        (MapKey::Int(value), ValueType::U1) => (0..=1).contains(value),
        (MapKey::Int(value), ValueType::U8) => (0..=u8::MAX as i128).contains(value),
        (MapKey::Int(value), ValueType::U16) => (0..=u16::MAX as i128).contains(value),
        (MapKey::Int(value), ValueType::U32) => (0..=u32::MAX as i128).contains(value),
        (MapKey::Int(value), ValueType::I32) => {
            (i32::MIN as i128..=i32::MAX as i128).contains(value)
        }
        (MapKey::Int(value), ValueType::I64) => {
            (i64::MIN as i128..=i64::MAX as i128).contains(value)
        }
        (MapKey::Bytes(_), ValueType::Str) => true,
        (MapKey::Float(key), ValueType::F32) => {
            let value = crate::value::float_from_key(*key);
            value.is_finite() && crate::value::float_key((value as f32) as f64) == *key
        }
        (MapKey::Float(key), ValueType::F64) => {
            let value = crate::value::float_from_key(*key);
            value.is_finite() && crate::value::float_key(value) == *key
        }
        _ => false,
    }
}

/// Host layoutの名付き型から到達できる定義だけを固定したABIの一部。
///
/// `HostLayout`が同じArcでも、別sourceが`struct P`の欄を変えていれば
/// そのprogramどうしでtokenを流用しない。関係ない局所型は共有を妨げない。
#[derive(Clone, Debug, PartialEq)]
struct ProgramTypeSchema {
    structs: Vec<(String, Vec<(BindKind, String, ValueType)>)>,
    wraps: Vec<(String, ValueType)>,
    unresolved: Vec<String>,
}

enum ProgramTypeSchemaError {
    TooDeep,
    TooLarge,
    UnsupportedF80,
}

impl ProgramTypeSchema {
    fn for_layout(layout: &HostLayout, program: &Program2) -> Result<Self, ProgramTypeSchemaError> {
        let (reachable, unresolved) = reachable_program_types(layout, program)?;
        let mut structs = Vec::new();
        let mut wraps = Vec::new();
        for name in reachable {
            if let Some(declaration) = program.structs.get(&name) {
                let fields = declaration
                    .fields
                    .iter()
                    .map(|field| (field.kind, field.name.clone(), field.ty.value.clone()))
                    .collect();
                structs.push((name.clone(), fields));
            }
            if let Some(base) = program.wraps.get(&name) {
                wraps.push((name.clone(), base.clone()));
            }
        }
        structs.sort_by(|left, right| left.0.cmp(&right.0));
        wraps.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(Self {
            structs,
            wraps,
            unresolved,
        })
    }

    fn struct_fields(&self, name: &str) -> Option<&[(BindKind, String, ValueType)]> {
        self.structs
            .binary_search_by(|candidate| candidate.0.as_str().cmp(name))
            .ok()
            .map(|index| self.structs[index].1.as_slice())
    }

    fn wrap_base(&self, name: &str) -> Option<&ValueType> {
        self.wraps
            .binary_search_by(|candidate| candidate.0.as_str().cmp(name))
            .ok()
            .map(|index| &self.wraps[index].1)
    }
}

/// Layoutの型を起点にstruct/wrapを展開し、実値validatorと同じ深さでABIを固定する。
fn reachable_program_types(
    layout: &HostLayout,
    program: &Program2,
) -> Result<(Vec<String>, Vec<String>), ProgramTypeSchemaError> {
    let mut pending = Vec::new();
    for (_, item) in layout.entries() {
        match item {
            HostItem::Value(ty) => pending.push((ty, 0usize)),
            HostItem::Fn(signature) => {
                pending.extend(signature.params.iter().map(|ty| (ty, 0usize)));
                if let Some(ty) = &signature.ret {
                    pending.push((ty, 0));
                }
            }
        }
    }

    let mut visited = 0usize;
    let mut expanded_depth = HashMap::<String, usize>::new();
    let mut reachable = HashSet::new();
    let mut unresolved = HashSet::new();
    while let Some((ty, depth)) = pending.pop() {
        if depth > MAX_HOST_TYPE_DESCRIPTOR_DEPTH {
            return Err(ProgramTypeSchemaError::TooDeep);
        }
        visited += 1;
        if visited > MAX_HOST_TYPE_DESCRIPTOR_NODES {
            return Err(ProgramTypeSchemaError::TooLarge);
        }
        let child_depth = depth + 1;
        match ty {
            ValueType::Array(element) => pending.push((element, child_depth)),
            ValueType::Map(key, value) | ValueType::Hash(key, value) => {
                pending.push((key, child_depth));
                pending.push((value, child_depth));
            }
            ValueType::Named(name) => {
                reachable.insert(name.clone());
                if expanded_depth.get(name).is_some_and(|old| *old >= depth) {
                    continue;
                }
                expanded_depth.insert(name.clone(), depth);
                let mut resolved = false;
                if let Some(declaration) = program.structs.get(name) {
                    resolved = true;
                    pending.extend(
                        declaration
                            .fields
                            .iter()
                            .map(|field| (&field.ty.value, child_depth)),
                    );
                }
                if let Some(base) = program.wraps.get(name) {
                    resolved = true;
                    pending.push((base, child_depth));
                }
                if !resolved {
                    unresolved.insert(name.clone());
                }
            }
            ValueType::F80 => return Err(ProgramTypeSchemaError::UnsupportedF80),
            _ => {}
        }
    }

    let mut reachable = reachable.into_iter().collect::<Vec<_>>();
    let mut unresolved = unresolved.into_iter().collect::<Vec<_>>();
    reachable.sort();
    unresolved.sort();
    Ok((reachable, unresolved))
}

/// Exact descriptorをprepare時に一度だけ照合し、runはArc identityだけを見る。
fn intern_type_schema(schema: ProgramTypeSchema) -> Arc<ProgramTypeSchema> {
    static SCHEMAS: OnceLock<Mutex<Vec<Weak<ProgramTypeSchema>>>> = OnceLock::new();
    let mut schemas = SCHEMAS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    schemas.retain(|candidate| candidate.strong_count() != 0);
    for candidate in schemas.iter().filter_map(Weak::upgrade) {
        if *candidate == schema {
            return candidate;
        }
    }
    let schema = Arc::new(schema);
    schemas.push(Arc::downgrade(&schema));
    schema
}
