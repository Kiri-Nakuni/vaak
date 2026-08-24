//! P/Invoke/IL2CPP側で同じ幅を宣言できる、pointerを含まない固定幅record。

use std::mem::{align_of, size_of};

pub const ABI_MAJOR: u16 = 0;
pub const ABI_MINOR: u16 = 0;
pub const MAX_SOURCE_BYTES: u64 = 4 * 1024 * 1024;
pub const MAX_NAME_BYTES: u64 = 4 * 1024 * 1024;
pub const MAX_WIRE_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_RECORDS: u32 = u16::MAX as u32;

/// C関数の戻り値に対応するtransport status。
///
/// C enumの幅には依存せず、公開面では常に`u32`として扱う。
pub mod status {
    pub const OK: u32 = 0;
    pub const INVALID_ARGUMENT: u32 = 1;
    pub const ABI_MISMATCH: u32 = 2;
    pub const MALFORMED_WIRE: u32 = 3;
    pub const LIMIT_EXCEEDED: u32 = 4;
    pub const STALE_HANDLE: u32 = 5;
    pub const BUSY: u32 = 6;
    pub const POISONED: u32 = 7;
    pub const BUFFER_TOO_SMALL: u32 = 8;
    pub const INTERNAL_PANIC: u32 = 9;
}

pub mod diagnostic_origin {
    pub const FFI_DECODE: u32 = 1;
    pub const PREPARE_PARSE: u32 = 2;
    pub const PREPARE_CHECK: u32 = 3;
    pub const PREPARE_TYPE_CHECK: u32 = 4;
    pub const PREPARE_COMPILE: u32 = 5;
    pub const VAAK_RUNTIME: u32 = 6;
    pub const HOST_CONTRACT: u32 = 7;
    pub const INTERNAL_PANIC: u32 = 8;
}

pub mod value_tag {
    pub const U1: u32 = 1;
    pub const U8: u32 = 2;
    pub const U16: u32 = 3;
    pub const U32: u32 = 4;
    pub const I32: u32 = 5;
    pub const I64: u32 = 6;
    pub const F32: u32 = 7;
    pub const F64: u32 = 8;
    pub const UTF8: u32 = 9;
    pub const BYTES: u32 = 10;
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbiInfoV0 {
    pub struct_size: u32,
    pub abi_major: u16,
    pub abi_minor: u16,
    pub supported_features: u64,
    pub required_alignment: u64,
    pub max_wire_bytes: u64,
    pub reserved: [u64; 4],
}

impl Default for AbiInfoV0 {
    fn default() -> Self {
        Self {
            struct_size: size_of::<Self>() as u32,
            abi_major: ABI_MAJOR,
            abi_minor: ABI_MINOR,
            supported_features: 0,
            required_alignment: 8,
            max_wire_bytes: MAX_WIRE_BYTES,
            reserved: [0; 4],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallEnvelopeV0 {
    pub struct_size: u32,
    pub transport_status: u32,
    pub detail_code: u32,
    pub flags: u32,
    pub required_bytes: u64,
    pub written_bytes: u64,
    pub diagnostic_id: u64,
    pub reserved: [u64; 3],
}

impl CallEnvelopeV0 {
    pub const fn with_status(transport_status: u32) -> Self {
        Self {
            struct_size: size_of::<Self>() as u32,
            transport_status,
            detail_code: 0,
            flags: 0,
            required_bytes: 0,
            written_bytes: 0,
            diagnostic_id: 0,
            reserved: [0; 3],
        }
    }
}

impl Default for CallEnvelopeV0 {
    fn default() -> Self {
        Self::with_status(status::OK)
    }
}

/// 表示message自体は隣接byte列に置き、この固定幅envelopeはoffset/lengthだけを持つ。
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiagnosticEnvelopeV0 {
    pub struct_size: u32,
    pub severity: u16,
    pub reserved0: u16,
    pub origin: u32,
    pub stable_code: u32,
    pub span_start: u64,
    pub span_length: u64,
    pub message_offset: u64,
    pub message_length: u64,
    pub incident_id: u64,
    pub reserved1: u64,
}

impl DiagnosticEnvelopeV0 {
    pub fn error(origin: u32, span_start: u64, span_length: u64, message_length: u64) -> Self {
        Self {
            struct_size: size_of::<Self>() as u32,
            severity: 3,
            reserved0: 0,
            origin,
            stable_code: 0,
            span_start,
            span_length,
            message_offset: 0,
            message_length,
            incident_id: 0,
            reserved1: 0,
        }
    }
}

/// prepare時に固定する一host value slot。
///
/// `name_offset/name_length`は同じcallのUTF-8 name blobを指す。
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HostLayoutEntryV0 {
    pub struct_size: u32,
    pub flags: u32,
    pub slot_index: u32,
    pub value_type: u32,
    pub entity_id: u64,
    pub property_id: u32,
    /// host-owned capability tableのindex。authorityそのものではない。
    pub capability_table_index: u32,
    pub name_offset: u64,
    pub name_length: u64,
    pub reserved1: u64,
}

impl HostLayoutEntryV0 {
    pub fn new(
        slot_index: u32,
        value_type: u32,
        entity_id: u64,
        property_id: u32,
        capability_table_index: u32,
        name_offset: u64,
        name_length: u64,
    ) -> Self {
        Self {
            struct_size: size_of::<Self>() as u32,
            flags: 0,
            slot_index,
            value_type,
            entity_id,
            property_id,
            capability_table_index,
            name_offset,
            name_length,
            reserved1: 0,
        }
    }
}

/// language-neutral exchange schemaの32-byte value node。
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ValueNodeV0 {
    pub type_tag: u32,
    pub type_id_or_flags: u32,
    pub scalar_bits_or_payload_offset: u64,
    pub payload_length_or_first_child: u64,
    pub child_count_or_reserved: u64,
}

/// SettingsSnapshotV0の48-byte record。
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SnapshotRecordV0 {
    pub entity_id: u64,
    pub property_id: u32,
    pub type_tag: u32,
    pub property_revision: u64,
    pub value_node_index_or_scalar: u64,
    pub payload_aux: u64,
    pub flags: u32,
    pub reserved: u32,
}

/// SettingsPatchV0の48-byte record。
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PatchRecordV0 {
    pub entity_id: u64,
    pub property_id: u32,
    pub operation: u16,
    pub flags: u16,
    pub expected_property_revision: u64,
    pub value_type_tag: u32,
    pub capability_table_index: u32,
    pub value_node_index_or_payload_offset: u64,
    pub value_aux_or_length: u64,
}

/// C側から中身をdereferenceしない世代付きprepared token。
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PreparedHandleV0(pub u64);

impl PreparedHandleV0 {
    pub const INVALID: Self = Self(0);
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }
    pub const fn into_raw(self) -> u64 {
        self.0
    }
}

/// C側から中身をdereferenceしない世代付きrunner token。
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct RunnerHandleV0(pub u64);

impl RunnerHandleV0 {
    pub const INVALID: Self = Self(0);
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }
    pub const fn into_raw(self) -> u64 {
        self.0
    }
}

const _: [(); 64] = [(); size_of::<AbiInfoV0>()];
const _: [(); 64] = [(); size_of::<CallEnvelopeV0>()];
const _: [(); 64] = [(); size_of::<DiagnosticEnvelopeV0>()];
const _: [(); 56] = [(); size_of::<HostLayoutEntryV0>()];
const _: [(); 32] = [(); size_of::<ValueNodeV0>()];
const _: [(); 48] = [(); size_of::<SnapshotRecordV0>()];
const _: [(); 48] = [(); size_of::<PatchRecordV0>()];
const _: [(); 8] = [(); size_of::<PreparedHandleV0>()];
const _: [(); 8] = [(); size_of::<RunnerHandleV0>()];
const _: [(); 8] = [(); align_of::<AbiInfoV0>()];

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{align_of, offset_of, size_of};

    #[test]
    fn 固定幅recordの大きさと主要offsetを固定する() {
        assert_eq!(size_of::<AbiInfoV0>(), 64);
        assert_eq!(size_of::<CallEnvelopeV0>(), 64);
        assert_eq!(size_of::<DiagnosticEnvelopeV0>(), 64);
        assert_eq!(size_of::<HostLayoutEntryV0>(), 56);
        assert_eq!(size_of::<ValueNodeV0>(), 32);
        assert_eq!(size_of::<SnapshotRecordV0>(), 48);
        assert_eq!(size_of::<PatchRecordV0>(), 48);
        assert_eq!(align_of::<SnapshotRecordV0>(), 8);
        assert_eq!(offset_of!(HostLayoutEntryV0, entity_id), 16);
        assert_eq!(offset_of!(HostLayoutEntryV0, name_offset), 32);
        assert_eq!(offset_of!(SnapshotRecordV0, property_revision), 16);
        assert_eq!(offset_of!(PatchRecordV0, expected_property_revision), 16);
    }

    #[test]
    fn abi情報はreservedを零にする() {
        let info = AbiInfoV0::default();
        assert_eq!(info.abi_major, 0);
        assert_eq!(info.required_alignment, 8);
        assert_eq!(info.reserved, [0; 4]);
    }
}
