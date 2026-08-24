//! IRON VAAK の言語中立なnative境界の中核。
//!
//! このcrateは、Vaakの意味論を追加せず、既存の
//! [`vaak::embedding::PreparedProgram`] と
//! [`vaak::embedding::EmbeddingRunner`] だけを包む。
//! v0 checkpointではraw pointerを受けるC shimをまだ公開せず、FFIの可変長入力を
//! safe sliceへ変換した後の中核契約を固定する。Unity型、managed object、Lua state、
//! Lua stack index、reverse callbackはここへ入れない。

#![forbid(unsafe_code)]

pub mod abi;
pub mod api;
pub mod codec;

pub use abi::{
    status, AbiInfoV0, CallEnvelopeV0, DiagnosticEnvelopeV0, HostLayoutEntryV0, PatchRecordV0,
    PreparedHandleV0, RunnerHandleV0, SnapshotRecordV0, ValueNodeV0,
};
pub use api::{
    ApiResult, DiagnosticV0, NativeApi, PrepareReplyV0, ProgramStatusV0, RunnerReportV0,
    TopLevelOutcomeV0,
};
pub use codec::{
    decode_patch, decode_snapshot, encode_patch, encode_snapshot, PatchBatchV0, PatchEntryV0,
    SnapshotBatchV0, SnapshotEntryV0, WireValueV0,
};
