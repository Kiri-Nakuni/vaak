//! `SettingsSnapshotV0` / `SettingsPatchV0`の一括byte codec。
//!
//! wireは常にlittle-endianで読み書きし、`repr(C)` recordをbyte列へcastしない。
//! これによりC#、IL2CPP、Lua adapterのmemory layoutをVaakへ持ち込まない。

use crate::abi::{value_tag, PatchRecordV0, SnapshotRecordV0, ValueNodeV0, ABI_MAJOR};
use crate::abi::{MAX_RECORDS, MAX_WIRE_BYTES};
use std::fmt;

const MAGIC: &[u8; 4] = b"IVX0";
const HEADER_BYTES: usize = 32;
const SECTION_BYTES: usize = 24;
const MESSAGE_SNAPSHOT: u16 = 2;
const MESSAGE_PATCH: u16 = 3;
const SECTION_META: u16 = 1;
const SECTION_RECORDS: u16 = 2;
const SECTION_VALUE_NODES: u16 = 3;
const SECTION_PAYLOAD: u16 = 4;
const SECTION_REQUIRED: u16 = 1;
const SNAPSHOT_META_BYTES: usize = 48;
const PATCH_META_BYTES: usize = 80;
const SNAPSHOT_RECORD_BYTES: usize = 48;
const PATCH_RECORD_BYTES: usize = 48;
const VALUE_NODE_BYTES: usize = 32;
const MAX_SECTIONS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodecErrorKind {
    Malformed,
    LimitExceeded,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodecError {
    pub kind: CodecErrorKind,
    pub message: String,
}

impl CodecError {
    fn malformed(message: impl Into<String>) -> Self {
        Self {
            kind: CodecErrorKind::Malformed,
            message: message.into(),
        }
    }

    fn limit(message: impl Into<String>) -> Self {
        Self {
            kind: CodecErrorKind::LimitExceeded,
            message: message.into(),
        }
    }
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(f)
    }
}

impl std::error::Error for CodecError {}

#[derive(Clone, Debug, PartialEq)]
pub enum WireValueV0 {
    U1(bool),
    U8(u8),
    U16(u16),
    U32(u32),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    Utf8(String),
    Bytes(Vec<u8>),
}

impl WireValueV0 {
    pub fn type_tag(&self) -> u32 {
        match self {
            Self::U1(_) => value_tag::U1,
            Self::U8(_) => value_tag::U8,
            Self::U16(_) => value_tag::U16,
            Self::U32(_) => value_tag::U32,
            Self::I32(_) => value_tag::I32,
            Self::I64(_) => value_tag::I64,
            Self::F32(_) => value_tag::F32,
            Self::F64(_) => value_tag::F64,
            Self::Utf8(_) => value_tag::UTF8,
            Self::Bytes(_) => value_tag::BYTES,
        }
    }

    fn validate(&self) -> Result<(), CodecError> {
        match self {
            Self::F32(value) if !value.is_finite() => {
                Err(CodecError::malformed("F32にNaNまたは無限大は置けない"))
            }
            Self::F64(value) if !value.is_finite() => {
                Err(CodecError::malformed("F64にNaNまたは無限大は置けない"))
            }
            _ => Ok(()),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SnapshotEntryV0 {
    pub entity_id: u64,
    pub property_id: u32,
    pub property_revision: u64,
    pub value: WireValueV0,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SnapshotBatchV0 {
    pub schema_id: [u8; 16],
    pub session_id: [u8; 16],
    pub snapshot_revision: u64,
    pub entries: Vec<SnapshotEntryV0>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PatchEntryV0 {
    pub entity_id: u64,
    pub property_id: u32,
    pub expected_property_revision: u64,
    pub capability_table_index: u32,
    pub value: WireValueV0,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PatchBatchV0 {
    pub schema_id: [u8; 16],
    pub session_id: [u8; 16],
    pub run_id: [u8; 16],
    pub transaction_id: [u8; 16],
    pub base_snapshot_revision: u64,
    pub entries: Vec<PatchEntryV0>,
}

#[derive(Clone, Copy, Debug)]
struct Section {
    kind: u16,
    flags: u16,
    record_width: u32,
    offset: usize,
    length: usize,
}

#[derive(Clone, Copy, Debug)]
struct Layout {
    meta_offset: usize,
    records_offset: usize,
    nodes_offset: usize,
    payload_offset: usize,
    total: usize,
}

pub fn encode_snapshot(batch: &SnapshotBatchV0) -> Result<Vec<u8>, CodecError> {
    validate_entry_count(batch.entries.len())?;
    validate_snapshot_order(&batch.entries)?;
    for entry in &batch.entries {
        entry.value.validate()?;
    }
    let payload_length = payload_length(batch.entries.iter().map(|entry| &entry.value))?;
    let layout = encoding_layout(SNAPSHOT_META_BYTES, batch.entries.len(), payload_length)?;
    let mut out = vec![0; layout.total];
    write_header(
        &mut out,
        MESSAGE_SNAPSHOT,
        batch.entries.len(),
        layout,
        SNAPSHOT_META_BYTES,
        SNAPSHOT_RECORD_BYTES,
    );
    let mut meta = layout.meta_offset;
    write_bytes(&mut out, &mut meta, &batch.schema_id);
    write_bytes(&mut out, &mut meta, &batch.session_id);
    write_u64_at(&mut out, meta, batch.snapshot_revision);
    meta += 8;
    write_u32_at(&mut out, meta, batch.entries.len() as u32);
    write_u32_at(&mut out, meta + 4, 0);

    let mut payload_cursor = layout.payload_offset;
    for (index, entry) in batch.entries.iter().enumerate() {
        let record = SnapshotRecordV0 {
            entity_id: entry.entity_id,
            property_id: entry.property_id,
            type_tag: entry.value.type_tag(),
            property_revision: entry.property_revision,
            value_node_index_or_scalar: index as u64,
            payload_aux: 0,
            flags: 0,
            reserved: 0,
        };
        write_snapshot_record(
            &mut out,
            layout.records_offset + index * SNAPSHOT_RECORD_BYTES,
            record,
        );
        let node = encode_value_node(&mut out, &entry.value, &mut payload_cursor)?;
        write_value_node(
            &mut out,
            layout.nodes_offset + index * VALUE_NODE_BYTES,
            node,
        );
    }
    debug_assert_eq!(payload_cursor, layout.total);
    Ok(out)
}

pub fn decode_snapshot(bytes: &[u8]) -> Result<SnapshotBatchV0, CodecError> {
    let decoded = decode_envelope(bytes, MESSAGE_SNAPSHOT, SNAPSHOT_META_BYTES)?;
    let meta = section_bytes(bytes, decoded.meta);
    let mut schema_id = [0; 16];
    schema_id.copy_from_slice(&meta[0..16]);
    let mut session_id = [0; 16];
    session_id.copy_from_slice(&meta[16..32]);
    let snapshot_revision = read_u64(meta, 32)?;
    let meta_count = read_u32(meta, 40)?;
    if meta_count != decoded.record_count || read_u32(meta, 44)? != 0 {
        return Err(CodecError::malformed(
            "snapshot metaの件数またはreservedが不正である",
        ));
    }
    let records = section_bytes(bytes, decoded.records);
    let nodes = section_bytes(bytes, decoded.nodes);
    let payload = decoded.payload;
    let mut entries = Vec::with_capacity(decoded.record_count as usize);
    let mut previous = None;
    for index in 0..decoded.record_count as usize {
        let record = read_snapshot_record(records, index * SNAPSHOT_RECORD_BYTES)?;
        validate_snapshot_record(record, index, &mut previous)?;
        let node = read_value_node(nodes, index * VALUE_NODE_BYTES)?;
        if record.type_tag != node.type_tag {
            return Err(CodecError::malformed(
                "snapshot recordとvalue nodeの型tagが一致しない",
            ));
        }
        let value = decode_value_node(bytes, payload, node)?;
        entries.push(SnapshotEntryV0 {
            entity_id: record.entity_id,
            property_id: record.property_id,
            property_revision: record.property_revision,
            value,
        });
    }
    Ok(SnapshotBatchV0 {
        schema_id,
        session_id,
        snapshot_revision,
        entries,
    })
}

pub fn encode_patch(batch: &PatchBatchV0) -> Result<Vec<u8>, CodecError> {
    validate_entry_count(batch.entries.len())?;
    validate_patch_order(&batch.entries)?;
    for entry in &batch.entries {
        entry.value.validate()?;
    }
    let payload_length = payload_length(batch.entries.iter().map(|entry| &entry.value))?;
    let layout = encoding_layout(PATCH_META_BYTES, batch.entries.len(), payload_length)?;
    let mut out = vec![0; layout.total];
    write_header(
        &mut out,
        MESSAGE_PATCH,
        batch.entries.len(),
        layout,
        PATCH_META_BYTES,
        PATCH_RECORD_BYTES,
    );
    let mut meta = layout.meta_offset;
    write_bytes(&mut out, &mut meta, &batch.schema_id);
    write_bytes(&mut out, &mut meta, &batch.session_id);
    write_bytes(&mut out, &mut meta, &batch.run_id);
    write_bytes(&mut out, &mut meta, &batch.transaction_id);
    write_u64_at(&mut out, meta, batch.base_snapshot_revision);
    meta += 8;
    write_u32_at(&mut out, meta, batch.entries.len() as u32);
    write_u32_at(&mut out, meta + 4, 0);

    let mut payload_cursor = layout.payload_offset;
    for (index, entry) in batch.entries.iter().enumerate() {
        let record = PatchRecordV0 {
            entity_id: entry.entity_id,
            property_id: entry.property_id,
            operation: 1,
            flags: 0,
            expected_property_revision: entry.expected_property_revision,
            value_type_tag: entry.value.type_tag(),
            capability_table_index: entry.capability_table_index,
            value_node_index_or_payload_offset: index as u64,
            value_aux_or_length: 0,
        };
        write_patch_record(
            &mut out,
            layout.records_offset + index * PATCH_RECORD_BYTES,
            record,
        );
        let node = encode_value_node(&mut out, &entry.value, &mut payload_cursor)?;
        write_value_node(
            &mut out,
            layout.nodes_offset + index * VALUE_NODE_BYTES,
            node,
        );
    }
    debug_assert_eq!(payload_cursor, layout.total);
    Ok(out)
}

pub fn decode_patch(bytes: &[u8]) -> Result<PatchBatchV0, CodecError> {
    let decoded = decode_envelope(bytes, MESSAGE_PATCH, PATCH_META_BYTES)?;
    let meta = section_bytes(bytes, decoded.meta);
    let mut schema_id = [0; 16];
    schema_id.copy_from_slice(&meta[0..16]);
    let mut session_id = [0; 16];
    session_id.copy_from_slice(&meta[16..32]);
    let mut run_id = [0; 16];
    run_id.copy_from_slice(&meta[32..48]);
    let mut transaction_id = [0; 16];
    transaction_id.copy_from_slice(&meta[48..64]);
    let base_snapshot_revision = read_u64(meta, 64)?;
    let meta_count = read_u32(meta, 72)?;
    if meta_count != decoded.record_count || read_u32(meta, 76)? != 0 {
        return Err(CodecError::malformed(
            "patch metaの件数またはreservedが不正である",
        ));
    }
    let records = section_bytes(bytes, decoded.records);
    let nodes = section_bytes(bytes, decoded.nodes);
    let payload = decoded.payload;
    let mut entries = Vec::with_capacity(decoded.record_count as usize);
    let mut previous = None;
    for index in 0..decoded.record_count as usize {
        let record = read_patch_record(records, index * PATCH_RECORD_BYTES)?;
        validate_patch_record(record, index, &mut previous)?;
        let node = read_value_node(nodes, index * VALUE_NODE_BYTES)?;
        if record.value_type_tag != node.type_tag {
            return Err(CodecError::malformed(
                "patch recordとvalue nodeの型tagが一致しない",
            ));
        }
        let value = decode_value_node(bytes, payload, node)?;
        entries.push(PatchEntryV0 {
            entity_id: record.entity_id,
            property_id: record.property_id,
            expected_property_revision: record.expected_property_revision,
            capability_table_index: record.capability_table_index,
            value,
        });
    }
    Ok(PatchBatchV0 {
        schema_id,
        session_id,
        run_id,
        transaction_id,
        base_snapshot_revision,
        entries,
    })
}

fn validate_entry_count(count: usize) -> Result<(), CodecError> {
    if count > MAX_RECORDS as usize {
        return Err(CodecError::limit(format!(
            "recordが{count}件あり、上限{MAX_RECORDS}を越える"
        )));
    }
    Ok(())
}

fn validate_snapshot_order(entries: &[SnapshotEntryV0]) -> Result<(), CodecError> {
    let mut previous = None;
    for entry in entries {
        let key = (entry.entity_id, entry.property_id);
        if previous.is_some_and(|old| old >= key) {
            return Err(CodecError::malformed(
                "snapshot propertyは(entity_id, property_id)のstrict昇順でなければならない",
            ));
        }
        previous = Some(key);
    }
    Ok(())
}

fn validate_patch_order(entries: &[PatchEntryV0]) -> Result<(), CodecError> {
    let mut previous = None;
    for entry in entries {
        let key = (entry.entity_id, entry.property_id);
        if previous.is_some_and(|old| old >= key) {
            return Err(CodecError::malformed(
                "patch propertyは(entity_id, property_id)のstrict昇順でなければならない",
            ));
        }
        previous = Some(key);
    }
    Ok(())
}

fn payload_length<'a>(values: impl Iterator<Item = &'a WireValueV0>) -> Result<usize, CodecError> {
    let mut length = 0usize;
    for value in values {
        let add = match value {
            WireValueV0::Utf8(value) => value.len(),
            WireValueV0::Bytes(value) => value.len(),
            _ => 0,
        };
        length = length
            .checked_add(add)
            .ok_or_else(|| CodecError::limit("payload長がusizeを越える"))?;
    }
    Ok(length)
}

fn encoding_layout(
    meta_bytes: usize,
    record_count: usize,
    payload_bytes: usize,
) -> Result<Layout, CodecError> {
    let prefix = HEADER_BYTES
        .checked_add(4 * SECTION_BYTES)
        .ok_or_else(|| CodecError::limit("section table長がoverflowした"))?;
    let meta_offset = align8(prefix)?;
    let records_offset = align8(
        meta_offset
            .checked_add(meta_bytes)
            .ok_or_else(|| CodecError::limit("meta末尾がoverflowした"))?,
    )?;
    let record_bytes = record_count
        .checked_mul(SNAPSHOT_RECORD_BYTES)
        .ok_or_else(|| CodecError::limit("record table長がoverflowした"))?;
    let nodes_offset = align8(
        records_offset
            .checked_add(record_bytes)
            .ok_or_else(|| CodecError::limit("record table末尾がoverflowした"))?,
    )?;
    let node_bytes = record_count
        .checked_mul(VALUE_NODE_BYTES)
        .ok_or_else(|| CodecError::limit("value node table長がoverflowした"))?;
    let payload_offset = align8(
        nodes_offset
            .checked_add(node_bytes)
            .ok_or_else(|| CodecError::limit("value node table末尾がoverflowした"))?,
    )?;
    let total = payload_offset
        .checked_add(payload_bytes)
        .ok_or_else(|| CodecError::limit("message長がoverflowした"))?;
    if total as u64 > MAX_WIRE_BYTES {
        return Err(CodecError::limit(format!(
            "messageが{total} byteあり、上限{MAX_WIRE_BYTES}を越える"
        )));
    }
    Ok(Layout {
        meta_offset,
        records_offset,
        nodes_offset,
        payload_offset,
        total,
    })
}

fn align8(value: usize) -> Result<usize, CodecError> {
    value
        .checked_add(7)
        .map(|value| value & !7)
        .ok_or_else(|| CodecError::limit("8-byte alignment計算がoverflowした"))
}

fn write_header(
    out: &mut [u8],
    message_kind: u16,
    record_count: usize,
    layout: Layout,
    meta_bytes: usize,
    record_bytes: usize,
) {
    out[0..4].copy_from_slice(MAGIC);
    write_u16_at(out, 4, ABI_MAJOR);
    write_u16_at(out, 6, 0);
    write_u16_at(out, 8, message_kind);
    write_u16_at(out, 10, 0);
    write_u32_at(out, 12, HEADER_BYTES as u32);
    write_u64_at(out, 16, layout.total as u64);
    write_u32_at(out, 24, record_count as u32);
    write_u32_at(out, 28, 4);
    let sections = [
        Section {
            kind: SECTION_META,
            flags: SECTION_REQUIRED,
            record_width: 0,
            offset: layout.meta_offset,
            length: meta_bytes,
        },
        Section {
            kind: SECTION_RECORDS,
            flags: SECTION_REQUIRED,
            record_width: record_bytes as u32,
            offset: layout.records_offset,
            length: record_count * record_bytes,
        },
        Section {
            kind: SECTION_VALUE_NODES,
            flags: SECTION_REQUIRED,
            record_width: VALUE_NODE_BYTES as u32,
            offset: layout.nodes_offset,
            length: record_count * VALUE_NODE_BYTES,
        },
        Section {
            kind: SECTION_PAYLOAD,
            flags: 0,
            record_width: 0,
            offset: layout.payload_offset,
            length: layout.total - layout.payload_offset,
        },
    ];
    for (index, section) in sections.into_iter().enumerate() {
        let offset = HEADER_BYTES + index * SECTION_BYTES;
        write_u16_at(out, offset, section.kind);
        write_u16_at(out, offset + 2, section.flags);
        write_u32_at(out, offset + 4, section.record_width);
        write_u64_at(out, offset + 8, section.offset as u64);
        write_u64_at(out, offset + 16, section.length as u64);
    }
}

struct DecodedEnvelope {
    record_count: u32,
    meta: Section,
    records: Section,
    nodes: Section,
    payload: Option<Section>,
}

fn decode_envelope(
    bytes: &[u8],
    expected_kind: u16,
    expected_meta_bytes: usize,
) -> Result<DecodedEnvelope, CodecError> {
    if bytes.len() as u64 > MAX_WIRE_BYTES {
        return Err(CodecError::limit(format!(
            "messageが{} byteあり、上限{MAX_WIRE_BYTES}を越える",
            bytes.len()
        )));
    }
    if bytes.len() < HEADER_BYTES || &bytes[0..4] != MAGIC {
        return Err(CodecError::malformed("共通headerまたはmagicが不正である"));
    }
    if read_u16(bytes, 4)? != ABI_MAJOR || read_u16(bytes, 6)? != 0 {
        return Err(CodecError::malformed("schema major/minorが未対応である"));
    }
    if read_u16(bytes, 8)? != expected_kind
        || read_u16(bytes, 10)? != 0
        || read_u32(bytes, 12)? as usize != HEADER_BYTES
    {
        return Err(CodecError::malformed(
            "message kind、flags、header bytesのいずれかが不正である",
        ));
    }
    let total = usize_from_u64(read_u64(bytes, 16)?)?;
    if total != bytes.len() {
        return Err(CodecError::malformed(
            "headerのtotal bytesが入力slice長と一致しない",
        ));
    }
    let record_count = read_u32(bytes, 24)?;
    validate_entry_count(record_count as usize)?;
    let section_count = read_u32(bytes, 28)? as usize;
    if section_count > MAX_SECTIONS {
        return Err(CodecError::limit("section数が上限を越える"));
    }
    let prefix_end = HEADER_BYTES
        .checked_add(
            section_count
                .checked_mul(SECTION_BYTES)
                .ok_or_else(|| CodecError::malformed("section table長がoverflowした"))?,
        )
        .ok_or_else(|| CodecError::malformed("section table末尾がoverflowした"))?;
    if prefix_end > bytes.len() {
        return Err(CodecError::malformed("section tableが入力範囲を越える"));
    }
    let mut sections = Vec::with_capacity(section_count);
    for index in 0..section_count {
        let offset = HEADER_BYTES + index * SECTION_BYTES;
        let section = Section {
            kind: read_u16(bytes, offset)?,
            flags: read_u16(bytes, offset + 2)?,
            record_width: read_u32(bytes, offset + 4)?,
            offset: usize_from_u64(read_u64(bytes, offset + 8)?)?,
            length: usize_from_u64(read_u64(bytes, offset + 16)?)?,
        };
        if section.flags & !SECTION_REQUIRED != 0 {
            return Err(CodecError::malformed("section flagsに未知bitがある"));
        }
        if !section.offset.is_multiple_of(8) {
            return Err(CodecError::malformed("section startが8-byte alignedでない"));
        }
        let end = section
            .offset
            .checked_add(section.length)
            .ok_or_else(|| CodecError::malformed("section範囲がoverflowした"))?;
        if section.offset < prefix_end || end > bytes.len() {
            return Err(CodecError::malformed("sectionがmessage範囲外である"));
        }
        if section.record_width != 0
            && !section.length.is_multiple_of(section.record_width as usize)
        {
            return Err(CodecError::malformed(
                "section lengthがrecord widthの倍数でない",
            ));
        }
        sections.push(section);
    }
    let mut ranges: Vec<_> = sections
        .iter()
        .map(|section| (section.offset, section.offset + section.length))
        .collect();
    ranges.sort_unstable();
    for pair in ranges.windows(2) {
        if pair[0].1 > pair[1].0 && pair[0].0 != pair[0].1 && pair[1].0 != pair[1].1 {
            return Err(CodecError::malformed("section範囲が重複する"));
        }
    }

    let mut meta = None;
    let mut records = None;
    let mut nodes = None;
    let mut payload = None;
    for section in sections {
        let destination = match section.kind {
            SECTION_META => &mut meta,
            SECTION_RECORDS => &mut records,
            SECTION_VALUE_NODES => &mut nodes,
            SECTION_PAYLOAD => &mut payload,
            _ if section.flags & SECTION_REQUIRED != 0 => {
                return Err(CodecError::malformed("未知の必須sectionがある"));
            }
            _ => continue,
        };
        if destination.replace(section).is_some() {
            return Err(CodecError::malformed("singleton sectionが重複する"));
        }
    }
    let meta = meta.ok_or_else(|| CodecError::malformed("meta sectionが無い"))?;
    let records = records.ok_or_else(|| CodecError::malformed("record sectionが無い"))?;
    let nodes = nodes.ok_or_else(|| CodecError::malformed("value node sectionが無い"))?;
    if meta.length != expected_meta_bytes || meta.record_width != 0 {
        return Err(CodecError::malformed("meta sectionの幅が不正である"));
    }
    let expected_record_width = if expected_kind == MESSAGE_SNAPSHOT {
        SNAPSHOT_RECORD_BYTES
    } else {
        PATCH_RECORD_BYTES
    };
    if records.record_width as usize != expected_record_width
        || records.length != record_count as usize * expected_record_width
    {
        return Err(CodecError::malformed(
            "record sectionの幅または件数が不正である",
        ));
    }
    if nodes.record_width as usize != VALUE_NODE_BYTES
        || nodes.length != record_count as usize * VALUE_NODE_BYTES
    {
        return Err(CodecError::malformed(
            "value node sectionの幅または件数が不正である",
        ));
    }
    if payload.is_some_and(|section| section.record_width != 0) {
        return Err(CodecError::malformed("payload sectionにrecord widthがある"));
    }
    Ok(DecodedEnvelope {
        record_count,
        meta,
        records,
        nodes,
        payload,
    })
}

fn encode_value_node(
    out: &mut [u8],
    value: &WireValueV0,
    payload_cursor: &mut usize,
) -> Result<ValueNodeV0, CodecError> {
    value.validate()?;
    let mut node = ValueNodeV0 {
        type_tag: value.type_tag(),
        ..ValueNodeV0::default()
    };
    node.scalar_bits_or_payload_offset = match value {
        WireValueV0::U1(value) => u64::from(*value),
        WireValueV0::U8(value) => u64::from(*value),
        WireValueV0::U16(value) => u64::from(*value),
        WireValueV0::U32(value) => u64::from(*value),
        WireValueV0::I32(value) => u64::from(u32::from_le_bytes(value.to_le_bytes())),
        WireValueV0::I64(value) => u64::from_le_bytes(value.to_le_bytes()),
        WireValueV0::F32(value) => u64::from(value.to_bits()),
        WireValueV0::F64(value) => value.to_bits(),
        WireValueV0::Utf8(value) => {
            let start = *payload_cursor;
            let end = start
                .checked_add(value.len())
                .ok_or_else(|| CodecError::limit("UTF-8 payload末尾がoverflowした"))?;
            out.get_mut(start..end)
                .ok_or_else(|| CodecError::limit("UTF-8 payloadが出力範囲を越える"))?
                .copy_from_slice(value.as_bytes());
            *payload_cursor = end;
            node.payload_length_or_first_child = value.len() as u64;
            start as u64
        }
        WireValueV0::Bytes(value) => {
            let start = *payload_cursor;
            let end = start
                .checked_add(value.len())
                .ok_or_else(|| CodecError::limit("bytes payload末尾がoverflowした"))?;
            out.get_mut(start..end)
                .ok_or_else(|| CodecError::limit("bytes payloadが出力範囲を越える"))?
                .copy_from_slice(value);
            *payload_cursor = end;
            node.payload_length_or_first_child = value.len() as u64;
            start as u64
        }
    };
    Ok(node)
}

fn decode_value_node(
    all_bytes: &[u8],
    payload: Option<Section>,
    node: ValueNodeV0,
) -> Result<WireValueV0, CodecError> {
    if node.type_id_or_flags != 0 || node.child_count_or_reserved != 0 {
        return Err(CodecError::malformed(
            "value nodeのflags/reservedが非零である",
        ));
    }
    let bits = node.scalar_bits_or_payload_offset;
    let aux = node.payload_length_or_first_child;
    let scalar = |value| {
        if aux != 0 {
            Err(CodecError::malformed("scalar value nodeのauxが非零である"))
        } else {
            Ok(value)
        }
    };
    match node.type_tag {
        value_tag::U1 if bits <= 1 => scalar(WireValueV0::U1(bits != 0)),
        value_tag::U1 => Err(CodecError::malformed("U1のbit値が0/1でない")),
        value_tag::U8 if bits <= u8::MAX as u64 => scalar(WireValueV0::U8(bits as u8)),
        value_tag::U16 if bits <= u16::MAX as u64 => scalar(WireValueV0::U16(bits as u16)),
        value_tag::U32 if bits <= u32::MAX as u64 => scalar(WireValueV0::U32(bits as u32)),
        value_tag::I32 if bits <= u32::MAX as u64 => scalar(WireValueV0::I32(i32::from_le_bytes(
            (bits as u32).to_le_bytes(),
        ))),
        value_tag::I64 => scalar(WireValueV0::I64(i64::from_le_bytes(bits.to_le_bytes()))),
        value_tag::F32 if bits <= u32::MAX as u64 => {
            let value = f32::from_bits(bits as u32);
            if value.is_finite() {
                scalar(WireValueV0::F32(value))
            } else {
                Err(CodecError::malformed("F32にNaNまたは無限大がある"))
            }
        }
        value_tag::F64 => {
            let value = f64::from_bits(bits);
            if value.is_finite() {
                scalar(WireValueV0::F64(value))
            } else {
                Err(CodecError::malformed("F64にNaNまたは無限大がある"))
            }
        }
        value_tag::UTF8 | value_tag::BYTES => {
            let payload = payload.ok_or_else(|| CodecError::malformed("payload sectionが無い"))?;
            let start = usize_from_u64(bits)?;
            let length = usize_from_u64(aux)?;
            let end = start
                .checked_add(length)
                .ok_or_else(|| CodecError::malformed("payload参照がoverflowした"))?;
            let payload_start = payload.offset;
            let payload_end = payload.offset + payload.length;
            if start < payload_start || end > payload_end {
                return Err(CodecError::malformed(
                    "value nodeのpayload参照がpayload section外である",
                ));
            }
            let bytes = all_bytes[start..end].to_vec();
            if node.type_tag == value_tag::UTF8 {
                let value = String::from_utf8(bytes)
                    .map_err(|_| CodecError::malformed("UTF8 valueが正しいUTF-8でない"))?;
                Ok(WireValueV0::Utf8(value))
            } else {
                Ok(WireValueV0::Bytes(bytes))
            }
        }
        _ => Err(CodecError::malformed(
            "value nodeのtype tagまたはscalar幅が不正である",
        )),
    }
}

fn validate_snapshot_record(
    record: SnapshotRecordV0,
    index: usize,
    previous: &mut Option<(u64, u32)>,
) -> Result<(), CodecError> {
    if record.value_node_index_or_scalar != index as u64
        || record.payload_aux != 0
        || record.flags != 0
        || record.reserved != 0
    {
        return Err(CodecError::malformed(
            "snapshot recordのnode index、flags、reservedが不正である",
        ));
    }
    let key = (record.entity_id, record.property_id);
    if previous.is_some_and(|old| old >= key) {
        return Err(CodecError::malformed(
            "snapshot recordがstrict昇順でないか重複している",
        ));
    }
    *previous = Some(key);
    Ok(())
}

fn validate_patch_record(
    record: PatchRecordV0,
    index: usize,
    previous: &mut Option<(u64, u32)>,
) -> Result<(), CodecError> {
    if record.operation != 1
        || record.flags != 0
        || record.value_node_index_or_payload_offset != index as u64
        || record.value_aux_or_length != 0
    {
        return Err(CodecError::malformed(
            "patch recordのoperation、node index、flags、reservedが不正である",
        ));
    }
    let key = (record.entity_id, record.property_id);
    if previous.is_some_and(|old| old >= key) {
        return Err(CodecError::malformed(
            "patch recordがstrict昇順でないか重複している",
        ));
    }
    *previous = Some(key);
    Ok(())
}

fn write_snapshot_record(out: &mut [u8], offset: usize, record: SnapshotRecordV0) {
    write_u64_at(out, offset, record.entity_id);
    write_u32_at(out, offset + 8, record.property_id);
    write_u32_at(out, offset + 12, record.type_tag);
    write_u64_at(out, offset + 16, record.property_revision);
    write_u64_at(out, offset + 24, record.value_node_index_or_scalar);
    write_u64_at(out, offset + 32, record.payload_aux);
    write_u32_at(out, offset + 40, record.flags);
    write_u32_at(out, offset + 44, record.reserved);
}

fn read_snapshot_record(bytes: &[u8], offset: usize) -> Result<SnapshotRecordV0, CodecError> {
    Ok(SnapshotRecordV0 {
        entity_id: read_u64(bytes, offset)?,
        property_id: read_u32(bytes, offset + 8)?,
        type_tag: read_u32(bytes, offset + 12)?,
        property_revision: read_u64(bytes, offset + 16)?,
        value_node_index_or_scalar: read_u64(bytes, offset + 24)?,
        payload_aux: read_u64(bytes, offset + 32)?,
        flags: read_u32(bytes, offset + 40)?,
        reserved: read_u32(bytes, offset + 44)?,
    })
}

fn write_patch_record(out: &mut [u8], offset: usize, record: PatchRecordV0) {
    write_u64_at(out, offset, record.entity_id);
    write_u32_at(out, offset + 8, record.property_id);
    write_u16_at(out, offset + 12, record.operation);
    write_u16_at(out, offset + 14, record.flags);
    write_u64_at(out, offset + 16, record.expected_property_revision);
    write_u32_at(out, offset + 24, record.value_type_tag);
    write_u32_at(out, offset + 28, record.capability_table_index);
    write_u64_at(out, offset + 32, record.value_node_index_or_payload_offset);
    write_u64_at(out, offset + 40, record.value_aux_or_length);
}

fn read_patch_record(bytes: &[u8], offset: usize) -> Result<PatchRecordV0, CodecError> {
    Ok(PatchRecordV0 {
        entity_id: read_u64(bytes, offset)?,
        property_id: read_u32(bytes, offset + 8)?,
        operation: read_u16(bytes, offset + 12)?,
        flags: read_u16(bytes, offset + 14)?,
        expected_property_revision: read_u64(bytes, offset + 16)?,
        value_type_tag: read_u32(bytes, offset + 24)?,
        capability_table_index: read_u32(bytes, offset + 28)?,
        value_node_index_or_payload_offset: read_u64(bytes, offset + 32)?,
        value_aux_or_length: read_u64(bytes, offset + 40)?,
    })
}

fn write_value_node(out: &mut [u8], offset: usize, node: ValueNodeV0) {
    write_u32_at(out, offset, node.type_tag);
    write_u32_at(out, offset + 4, node.type_id_or_flags);
    write_u64_at(out, offset + 8, node.scalar_bits_or_payload_offset);
    write_u64_at(out, offset + 16, node.payload_length_or_first_child);
    write_u64_at(out, offset + 24, node.child_count_or_reserved);
}

fn read_value_node(bytes: &[u8], offset: usize) -> Result<ValueNodeV0, CodecError> {
    Ok(ValueNodeV0 {
        type_tag: read_u32(bytes, offset)?,
        type_id_or_flags: read_u32(bytes, offset + 4)?,
        scalar_bits_or_payload_offset: read_u64(bytes, offset + 8)?,
        payload_length_or_first_child: read_u64(bytes, offset + 16)?,
        child_count_or_reserved: read_u64(bytes, offset + 24)?,
    })
}

fn section_bytes(bytes: &[u8], section: Section) -> &[u8] {
    &bytes[section.offset..section.offset + section.length]
}

fn usize_from_u64(value: u64) -> Result<usize, CodecError> {
    usize::try_from(value).map_err(|_| CodecError::malformed("u64値をnative usizeへ変換できない"))
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, CodecError> {
    let raw = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| CodecError::malformed("u16 fieldが入力範囲を越える"))?;
    Ok(u16::from_le_bytes([raw[0], raw[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, CodecError> {
    let raw = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| CodecError::malformed("u32 fieldが入力範囲を越える"))?;
    Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, CodecError> {
    let raw = bytes
        .get(offset..offset + 8)
        .ok_or_else(|| CodecError::malformed("u64 fieldが入力範囲を越える"))?;
    Ok(u64::from_le_bytes([
        raw[0], raw[1], raw[2], raw[3], raw[4], raw[5], raw[6], raw[7],
    ]))
}

fn write_u16_at(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_u32_at(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_u64_at(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn write_bytes(out: &mut [u8], cursor: &mut usize, bytes: &[u8]) {
    let end = *cursor + bytes.len();
    out[*cursor..end].copy_from_slice(bytes);
    *cursor = end;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot_fixture() -> SnapshotBatchV0 {
        SnapshotBatchV0 {
            schema_id: [1; 16],
            session_id: [2; 16],
            snapshot_revision: 7,
            entries: vec![
                SnapshotEntryV0 {
                    entity_id: 0,
                    property_id: 10,
                    property_revision: 3,
                    value: WireValueV0::I64(-42),
                },
                SnapshotEntryV0 {
                    entity_id: 0,
                    property_id: 11,
                    property_revision: 4,
                    value: WireValueV0::Utf8("鉄の雨☂".into()),
                },
            ],
        }
    }

    #[test]
    fn snapshotを一括encodeして同じ値へdecodeする() {
        let fixture = snapshot_fixture();
        let bytes = encode_snapshot(&fixture).expect("encode");
        assert_eq!(decode_snapshot(&bytes).expect("decode"), fixture);
    }

    #[test]
    fn patchを一括encodeして同じ値へdecodeする() {
        let fixture = PatchBatchV0 {
            schema_id: [1; 16],
            session_id: [2; 16],
            run_id: [3; 16],
            transaction_id: [4; 16],
            base_snapshot_revision: 7,
            entries: vec![PatchEntryV0 {
                entity_id: 5,
                property_id: 6,
                expected_property_revision: 8,
                capability_table_index: 9,
                value: WireValueV0::F32(1.25),
            }],
        };
        let bytes = encode_patch(&fixture).expect("encode");
        assert_eq!(decode_patch(&bytes).expect("decode"), fixture);
    }

    #[test]
    fn 非canonicalなproperty順と重複を拒む() {
        let mut fixture = snapshot_fixture();
        fixture.entries.swap(0, 1);
        assert_eq!(
            encode_snapshot(&fixture).expect_err("順序違反").kind,
            CodecErrorKind::Malformed
        );
        fixture.entries[0].property_id = fixture.entries[1].property_id;
        assert!(encode_snapshot(&fixture).is_err());
    }

    #[test]
    fn 不正utf8とpayload範囲外を全体として拒む() {
        let fixture = snapshot_fixture();
        let mut bytes = encode_snapshot(&fixture).expect("encode");
        let payload_offset = read_u64(&bytes, HEADER_BYTES + 3 * SECTION_BYTES + 8)
            .expect("payload offset") as usize;
        bytes[payload_offset] = 0xff;
        assert_eq!(
            decode_snapshot(&bytes).expect_err("UTF-8違反").kind,
            CodecErrorKind::Malformed
        );

        let mut bytes = encode_snapshot(&fixture).expect("encode");
        let nodes_offset =
            read_u64(&bytes, HEADER_BYTES + 2 * SECTION_BYTES + 8).expect("nodes offset") as usize;
        write_u64_at(&mut bytes, nodes_offset + VALUE_NODE_BYTES + 8, u64::MAX);
        assert!(decode_snapshot(&bytes).is_err());
    }

    #[test]
    fn 未知optional_sectionはskipし未知requiredは拒む() {
        let fixture = SnapshotBatchV0 {
            schema_id: [1; 16],
            session_id: [2; 16],
            snapshot_revision: 7,
            entries: vec![SnapshotEntryV0 {
                entity_id: 0,
                property_id: 10,
                property_revision: 3,
                value: WireValueV0::I64(-42),
            }],
        };
        let mut bytes = encode_snapshot(&fixture).expect("encode");
        write_u16_at(&mut bytes, HEADER_BYTES + 3 * SECTION_BYTES, 999);
        assert_eq!(decode_snapshot(&bytes).expect("optionalをskip"), fixture);
        write_u16_at(
            &mut bytes,
            HEADER_BYTES + 3 * SECTION_BYTES + 2,
            SECTION_REQUIRED,
        );
        assert!(decode_snapshot(&bytes).is_err());
    }

    #[test]
    fn nanと無限大をwireへ入れない() {
        let fixture = SnapshotBatchV0 {
            schema_id: [0; 16],
            session_id: [0; 16],
            snapshot_revision: 0,
            entries: vec![SnapshotEntryV0 {
                entity_id: 0,
                property_id: 1,
                property_revision: 0,
                value: WireValueV0::F64(f64::NAN),
            }],
        };
        assert!(encode_snapshot(&fixture).is_err());
    }
}
