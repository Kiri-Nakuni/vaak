using System;
using System.Buffers.Binary;
using System.Collections.Generic;

namespace IronVaak
{
    /// <summary>Canonical little-endian codec shared by IRON VAAK and script adapters.</summary>
    public static class ExchangeCodec
    {
        private const int HeaderBytes = 32;
        private const int SectionBytes = 24;
        private const int SnapshotMetaBytes = 48;
        private const int PatchMetaBytes = 80;
        private const int RecordBytes = 48;
        private const int ValueNodeBytes = 32;
        private const int MaxSections = 32;
        private const int MaxRecords = ushort.MaxValue;
        private const int MaxWireBytes = 16 * 1024 * 1024;
        private const ushort MessageSnapshot = 2;
        private const ushort MessagePatch = 3;
        private const ushort SectionMeta = 1;
        private const ushort SectionRecords = 2;
        private const ushort SectionValueNodes = 3;
        private const ushort SectionPayload = 4;
        private const ushort SectionRequired = 1;

        public static byte[] EncodeSnapshot(SettingsSnapshot snapshot)
        {
            if (snapshot == null) throw new ArgumentNullException(nameof(snapshot));
            return Encode(
                MessageSnapshot,
                SnapshotMetaBytes,
                snapshot.Entries.Count,
                snapshot.Entries,
                entry => entry.Value,
                (buffer, offset) =>
                {
                    snapshot.SchemaId.WriteTo(buffer.AsSpan(offset, 16));
                    snapshot.SessionId.WriteTo(buffer.AsSpan(offset + 16, 16));
                    WriteU64(buffer, offset + 32, snapshot.SnapshotRevision);
                    WriteU32(buffer, offset + 40, checked((uint)snapshot.Entries.Count));
                    WriteU32(buffer, offset + 44, 0);
                },
                (buffer, offset, entry, index) => WriteSnapshotRecord(buffer, offset, entry, index));
        }

        public static SettingsSnapshot DecodeSnapshot(ReadOnlySpan<byte> bytes)
        {
            DecodedEnvelope envelope = DecodeEnvelope(bytes, MessageSnapshot, SnapshotMetaBytes);
            ReadOnlySpan<byte> meta = Slice(bytes, envelope.Meta);
            WireId128 schemaId = WireId128.FromBytes(meta.Slice(0, 16));
            WireId128 sessionId = WireId128.FromBytes(meta.Slice(16, 16));
            ulong revision = ReadU64(meta, 32);
            if (ReadU32(meta, 40) != envelope.RecordCount || ReadU32(meta, 44) != 0)
                throw Malformed("Snapshot meta is not canonical.");

            var entries = new List<SnapshotEntry>(checked((int)envelope.RecordCount));
            PropertyKey? previous = null;
            for (int index = 0; index < envelope.RecordCount; index++)
            {
                int recordOffset = checked(envelope.Records.Offset + index * RecordBytes);
                ulong entityId = ReadU64(bytes, recordOffset);
                uint propertyId = ReadU32(bytes, recordOffset + 8);
                uint typeTag = ReadU32(bytes, recordOffset + 12);
                ulong propertyRevision = ReadU64(bytes, recordOffset + 16);
                if (ReadU64(bytes, recordOffset + 24) != (ulong)index ||
                    ReadU64(bytes, recordOffset + 32) != 0 ||
                    ReadU32(bytes, recordOffset + 40) != 0 ||
                    ReadU32(bytes, recordOffset + 44) != 0)
                    throw Malformed("Snapshot record is not canonical.");
                ValidateKey(ref previous, new PropertyKey(entityId, propertyId), "snapshot");
                WireValue value = ReadValue(bytes, envelope, index, typeTag);
                entries.Add(new SnapshotEntry(entityId, propertyId, propertyRevision, value));
            }
            return new SettingsSnapshot(schemaId, sessionId, revision, entries);
        }

        public static byte[] EncodePatch(SettingsPatch patch)
        {
            if (patch == null) throw new ArgumentNullException(nameof(patch));
            return Encode(
                MessagePatch,
                PatchMetaBytes,
                patch.Entries.Count,
                patch.Entries,
                entry => entry.Value,
                (buffer, offset) =>
                {
                    patch.SchemaId.WriteTo(buffer.AsSpan(offset, 16));
                    patch.SessionId.WriteTo(buffer.AsSpan(offset + 16, 16));
                    patch.RunId.WriteTo(buffer.AsSpan(offset + 32, 16));
                    patch.TransactionId.WriteTo(buffer.AsSpan(offset + 48, 16));
                    WriteU64(buffer, offset + 64, patch.BaseSnapshotRevision);
                    WriteU32(buffer, offset + 72, checked((uint)patch.Entries.Count));
                    WriteU32(buffer, offset + 76, 0);
                },
                (buffer, offset, entry, index) => WritePatchRecord(buffer, offset, entry, index));
        }

        public static SettingsPatch DecodePatch(ReadOnlySpan<byte> bytes)
        {
            DecodedEnvelope envelope = DecodeEnvelope(bytes, MessagePatch, PatchMetaBytes);
            ReadOnlySpan<byte> meta = Slice(bytes, envelope.Meta);
            WireId128 schemaId = WireId128.FromBytes(meta.Slice(0, 16));
            WireId128 sessionId = WireId128.FromBytes(meta.Slice(16, 16));
            WireId128 runId = WireId128.FromBytes(meta.Slice(32, 16));
            WireId128 transactionId = WireId128.FromBytes(meta.Slice(48, 16));
            ulong revision = ReadU64(meta, 64);
            if (ReadU32(meta, 72) != envelope.RecordCount || ReadU32(meta, 76) != 0)
                throw Malformed("Patch meta is not canonical.");

            var entries = new List<PatchEntry>(checked((int)envelope.RecordCount));
            PropertyKey? previous = null;
            for (int index = 0; index < envelope.RecordCount; index++)
            {
                int recordOffset = checked(envelope.Records.Offset + index * RecordBytes);
                ulong entityId = ReadU64(bytes, recordOffset);
                uint propertyId = ReadU32(bytes, recordOffset + 8);
                if (ReadU16(bytes, recordOffset + 12) != 1 ||
                    ReadU16(bytes, recordOffset + 14) != 0 ||
                    ReadU64(bytes, recordOffset + 32) != (ulong)index ||
                    ReadU64(bytes, recordOffset + 40) != 0)
                    throw Malformed("Patch record is not canonical.");
                ulong expectedRevision = ReadU64(bytes, recordOffset + 16);
                uint typeTag = ReadU32(bytes, recordOffset + 24);
                uint capability = ReadU32(bytes, recordOffset + 28);
                ValidateKey(ref previous, new PropertyKey(entityId, propertyId), "patch");
                WireValue value = ReadValue(bytes, envelope, index, typeTag);
                entries.Add(new PatchEntry(entityId, propertyId, expectedRevision, capability, value));
            }
            return new SettingsPatch(schemaId, sessionId, runId, transactionId, revision, entries);
        }

        private static byte[] Encode<T>(
            ushort messageKind,
            int metaBytes,
            int count,
            IReadOnlyList<T> entries,
            Func<T, WireValue> value,
            Action<byte[], int> writeMeta,
            Action<byte[], int, T, int> writeRecord)
        {
            if (count > MaxRecords) throw new WireFormatException("The record count exceeds the v0 limit.");
            int payloadBytes = 0;
            for (int index = 0; index < count; index++)
            {
                WireValue item = value(entries[index]);
                if (item.Type == WireValueType.Utf8 || item.Type == WireValueType.Bytes)
                    payloadBytes = checked(payloadBytes + item.Payload.Length);
            }
            Layout layout = EncodingLayout(metaBytes, count, payloadBytes);
            byte[] output = new byte[layout.Total];
            WriteHeader(output, messageKind, count, layout, metaBytes);
            writeMeta(output, layout.MetaOffset);
            int payloadCursor = layout.PayloadOffset;
            for (int index = 0; index < count; index++)
            {
                writeRecord(output, checked(layout.RecordsOffset + index * RecordBytes), entries[index], index);
                WriteValue(output, checked(layout.NodesOffset + index * ValueNodeBytes), value(entries[index]), ref payloadCursor);
            }
            if (payloadCursor != layout.Total) throw new InvalidOperationException("Internal wire layout mismatch.");
            return output;
        }

        private static void WriteSnapshotRecord(byte[] output, int offset, SnapshotEntry entry, int index)
        {
            WriteU64(output, offset, entry.EntityId);
            WriteU32(output, offset + 8, entry.PropertyId);
            WriteU32(output, offset + 12, (uint)entry.Value.Type);
            WriteU64(output, offset + 16, entry.PropertyRevision);
            WriteU64(output, offset + 24, (ulong)index);
        }

        private static void WritePatchRecord(byte[] output, int offset, PatchEntry entry, int index)
        {
            WriteU64(output, offset, entry.EntityId);
            WriteU32(output, offset + 8, entry.PropertyId);
            WriteU16(output, offset + 12, 1);
            WriteU64(output, offset + 16, entry.ExpectedPropertyRevision);
            WriteU32(output, offset + 24, (uint)entry.Value.Type);
            WriteU32(output, offset + 28, entry.CapabilityTableIndex);
            WriteU64(output, offset + 32, (ulong)index);
        }

        private static void WriteValue(byte[] output, int offset, WireValue value, ref int payloadCursor)
        {
            WriteU32(output, offset, (uint)value.Type);
            if (value.Type == WireValueType.Utf8 || value.Type == WireValueType.Bytes)
            {
                ReadOnlySpan<byte> payload = value.Payload.Span;
                WriteU64(output, offset + 8, (ulong)payloadCursor);
                WriteU64(output, offset + 16, (ulong)payload.Length);
                payload.CopyTo(output.AsSpan(payloadCursor, payload.Length));
                payloadCursor = checked(payloadCursor + payload.Length);
            }
            else
            {
                WriteU64(output, offset + 8, value.ScalarBits);
            }
        }

        private static WireValue ReadValue(ReadOnlySpan<byte> bytes, DecodedEnvelope envelope, int index, uint recordType)
        {
            int offset = checked(envelope.Nodes.Offset + index * ValueNodeBytes);
            uint type = ReadU32(bytes, offset);
            uint flags = ReadU32(bytes, offset + 4);
            ulong bits = ReadU64(bytes, offset + 8);
            ulong auxiliary = ReadU64(bytes, offset + 16);
            ulong reserved = ReadU64(bytes, offset + 24);
            if (recordType != type || flags != 0 || reserved != 0)
                throw Malformed("Value node type, flags, or reserved field is invalid.");
            WireValueType valueType = (WireValueType)type;
            if (valueType == WireValueType.Utf8 || valueType == WireValueType.Bytes)
            {
                if (!envelope.Payload.HasValue) throw Malformed("The payload section is missing.");
                Section payload = envelope.Payload.Value;
                int start = CheckedInt(bits, "payload offset");
                int length = CheckedInt(auxiliary, "payload length");
                int end = CheckedRangeEnd(start, length, "value payload");
                if (start < payload.Offset || end > CheckedRangeEnd(payload.Offset, payload.Length, "payload section"))
                    throw Malformed("A value points outside the payload section.");
                return WireValue.FromEncoded(valueType, 0, bytes.Slice(start, length));
            }
            if (auxiliary != 0) throw Malformed("A scalar value has a non-zero auxiliary field.");
            return WireValue.FromEncoded(valueType, bits, ReadOnlySpan<byte>.Empty);
        }

        private static Layout EncodingLayout(int metaBytes, int count, int payloadBytes)
        {
            int prefix = checked(HeaderBytes + 4 * SectionBytes);
            int meta = Align8(prefix);
            int records = Align8(checked(meta + metaBytes));
            int nodes = Align8(checked(records + count * RecordBytes));
            int payload = Align8(checked(nodes + count * ValueNodeBytes));
            int total = checked(payload + payloadBytes);
            if (total > MaxWireBytes) throw new WireFormatException("The message exceeds the v0 wire limit.");
            return new Layout(meta, records, nodes, payload, total);
        }

        private static int Align8(int value) => checked(value + 7) & ~7;

        private static void WriteHeader(byte[] output, ushort kind, int count, Layout layout, int metaBytes)
        {
            output[0] = (byte)'I'; output[1] = (byte)'V'; output[2] = (byte)'X'; output[3] = (byte)'0';
            WriteU16(output, 4, 0);
            WriteU16(output, 8, kind);
            WriteU32(output, 12, HeaderBytes);
            WriteU64(output, 16, (ulong)layout.Total);
            WriteU32(output, 24, checked((uint)count));
            WriteU32(output, 28, 4);
            WriteSection(output, 0, SectionMeta, SectionRequired, 0, layout.MetaOffset, metaBytes);
            WriteSection(output, 1, SectionRecords, SectionRequired, RecordBytes, layout.RecordsOffset, count * RecordBytes);
            WriteSection(output, 2, SectionValueNodes, SectionRequired, ValueNodeBytes, layout.NodesOffset, count * ValueNodeBytes);
            WriteSection(output, 3, SectionPayload, 0, 0, layout.PayloadOffset, layout.Total - layout.PayloadOffset);
        }

        private static void WriteSection(byte[] output, int index, ushort kind, ushort flags, int width, int offset, int length)
        {
            int start = HeaderBytes + index * SectionBytes;
            WriteU16(output, start, kind);
            WriteU16(output, start + 2, flags);
            WriteU32(output, start + 4, checked((uint)width));
            WriteU64(output, start + 8, checked((ulong)offset));
            WriteU64(output, start + 16, checked((ulong)length));
        }

        private static DecodedEnvelope DecodeEnvelope(ReadOnlySpan<byte> bytes, ushort expectedKind, int expectedMetaBytes)
        {
            if (bytes.Length > MaxWireBytes) throw new WireFormatException("The message exceeds the v0 wire limit.");
            if (bytes.Length < HeaderBytes || bytes[0] != 'I' || bytes[1] != 'V' || bytes[2] != 'X' || bytes[3] != '0')
                throw Malformed("The common header or magic is invalid.");
            if (ReadU16(bytes, 4) != 0 || ReadU16(bytes, 6) != 0)
                throw Malformed("The schema version is unsupported.");
            if (ReadU16(bytes, 8) != expectedKind || ReadU16(bytes, 10) != 0 || ReadU32(bytes, 12) != HeaderBytes)
                throw Malformed("The message kind, flags, or header size is invalid.");
            if (ReadU64(bytes, 16) != (ulong)bytes.Length)
                throw Malformed("The total byte count does not match the input.");
            uint recordCount = ReadU32(bytes, 24);
            if (recordCount > MaxRecords) throw Malformed("The record count exceeds the v0 limit.");
            int sectionCount = CheckedInt(ReadU32(bytes, 28), "section count");
            if (sectionCount > MaxSections) throw Malformed("The section count exceeds the v0 limit.");
            int prefixEnd = checked(HeaderBytes + sectionCount * SectionBytes);
            if (prefixEnd > bytes.Length) throw Malformed("The section table is truncated.");

            var sections = new List<Section>(sectionCount);
            for (int index = 0; index < sectionCount; index++)
            {
                int offset = HeaderBytes + index * SectionBytes;
                var section = new Section(
                    ReadU16(bytes, offset),
                    ReadU16(bytes, offset + 2),
                    CheckedInt(ReadU32(bytes, offset + 4), "record width"),
                    CheckedInt(ReadU64(bytes, offset + 8), "section offset"),
                    CheckedInt(ReadU64(bytes, offset + 16), "section length"));
                if ((section.Flags & ~SectionRequired) != 0) throw Malformed("A section contains unknown flag bits.");
                if ((section.Offset & 7) != 0) throw Malformed("A section is not 8-byte aligned.");
                int end = CheckedRangeEnd(section.Offset, section.Length, "section");
                if (section.Offset < prefixEnd || end > bytes.Length) throw Malformed("A section lies outside the message.");
                if (section.RecordWidth != 0 && section.Length % section.RecordWidth != 0)
                    throw Malformed("A section length is not a record-width multiple.");
                sections.Add(section);
            }
            var ranges = new List<(int Start, int End)>(sections.Count);
            foreach (Section section in sections)
                ranges.Add((section.Offset, CheckedRangeEnd(section.Offset, section.Length, "section")));
            ranges.Sort((left, right) => left.Start != right.Start ? left.Start.CompareTo(right.Start) : left.End.CompareTo(right.End));
            for (int index = 1; index < ranges.Count; index++)
            {
                var before = ranges[index - 1];
                var current = ranges[index];
                if (before.End > current.Start && before.Start != before.End && current.Start != current.End)
                    throw Malformed("Section ranges overlap.");
            }

            Section? meta = null, records = null, nodes = null, payload = null;
            foreach (Section section in sections)
            {
                switch (section.Kind)
                {
                    case SectionMeta: Assign(ref meta, section); break;
                    case SectionRecords: Assign(ref records, section); break;
                    case SectionValueNodes: Assign(ref nodes, section); break;
                    case SectionPayload: Assign(ref payload, section); break;
                    default:
                        if ((section.Flags & SectionRequired) != 0) throw Malformed("An unknown required section is present.");
                        break;
                }
            }
            if (!meta.HasValue || !records.HasValue || !nodes.HasValue) throw Malformed("A required section is missing.");
            if (meta.Value.Length != expectedMetaBytes || meta.Value.RecordWidth != 0)
                throw Malformed("The meta section width is invalid.");
            if (records.Value.RecordWidth != RecordBytes || records.Value.Length != checked((int)recordCount * RecordBytes))
                throw Malformed("The record section width or count is invalid.");
            if (nodes.Value.RecordWidth != ValueNodeBytes || nodes.Value.Length != checked((int)recordCount * ValueNodeBytes))
                throw Malformed("The value-node section width or count is invalid.");
            if (payload.HasValue && payload.Value.RecordWidth != 0) throw Malformed("The payload section has a record width.");
            return new DecodedEnvelope(recordCount, meta.Value, records.Value, nodes.Value, payload);
        }

        private static void Assign(ref Section? destination, Section section)
        {
            if (destination.HasValue) throw Malformed("A singleton section is duplicated.");
            destination = section;
        }

        private static void ValidateKey(ref PropertyKey? previous, PropertyKey current, string kind)
        {
            if (previous.HasValue && previous.Value.CompareTo(current) >= 0)
                throw Malformed($"The {kind} records are not in strict canonical order.");
            previous = current;
        }

        private static ReadOnlySpan<byte> Slice(ReadOnlySpan<byte> bytes, Section section) =>
            bytes.Slice(section.Offset, section.Length);

        private static int CheckedInt(ulong value, string field)
        {
            if (value > int.MaxValue) throw Malformed($"The {field} does not fit the managed wire limit.");
            return (int)value;
        }

        private static int CheckedInt(uint value, string field) => CheckedInt((ulong)value, field);

        private static int CheckedRangeEnd(int offset, int length, string field)
        {
            long end = (long)offset + length;
            if (end > int.MaxValue) throw Malformed($"The {field} range exceeds the managed wire limit.");
            return (int)end;
        }

        private static WireFormatException Malformed(string message) => new WireFormatException(message);

        private static ushort ReadU16(ReadOnlySpan<byte> bytes, int offset)
        {
            RequireRange(bytes, offset, 2);
            return BinaryPrimitives.ReadUInt16LittleEndian(bytes.Slice(offset, 2));
        }

        private static uint ReadU32(ReadOnlySpan<byte> bytes, int offset)
        {
            RequireRange(bytes, offset, 4);
            return BinaryPrimitives.ReadUInt32LittleEndian(bytes.Slice(offset, 4));
        }

        private static ulong ReadU64(ReadOnlySpan<byte> bytes, int offset)
        {
            RequireRange(bytes, offset, 8);
            return BinaryPrimitives.ReadUInt64LittleEndian(bytes.Slice(offset, 8));
        }

        private static void RequireRange(ReadOnlySpan<byte> bytes, int offset, int length)
        {
            if (offset < 0 || length < 0 || offset > bytes.Length - length)
                throw Malformed("A fixed-width field is truncated.");
        }

        private static void WriteU16(byte[] bytes, int offset, ushort value) =>
            BinaryPrimitives.WriteUInt16LittleEndian(bytes.AsSpan(offset, 2), value);
        private static void WriteU32(byte[] bytes, int offset, int value) => WriteU32(bytes, offset, checked((uint)value));
        private static void WriteU32(byte[] bytes, int offset, uint value) =>
            BinaryPrimitives.WriteUInt32LittleEndian(bytes.AsSpan(offset, 4), value);
        private static void WriteU64(byte[] bytes, int offset, ulong value) =>
            BinaryPrimitives.WriteUInt64LittleEndian(bytes.AsSpan(offset, 8), value);

        private readonly struct Layout
        {
            internal Layout(int metaOffset, int recordsOffset, int nodesOffset, int payloadOffset, int total)
            {
                MetaOffset = metaOffset;
                RecordsOffset = recordsOffset;
                NodesOffset = nodesOffset;
                PayloadOffset = payloadOffset;
                Total = total;
            }
            internal int MetaOffset { get; }
            internal int RecordsOffset { get; }
            internal int NodesOffset { get; }
            internal int PayloadOffset { get; }
            internal int Total { get; }
        }

        private readonly struct Section
        {
            internal Section(ushort kind, ushort flags, int recordWidth, int offset, int length)
            {
                Kind = kind;
                Flags = flags;
                RecordWidth = recordWidth;
                Offset = offset;
                Length = length;
            }
            internal ushort Kind { get; }
            internal ushort Flags { get; }
            internal int RecordWidth { get; }
            internal int Offset { get; }
            internal int Length { get; }
        }

        private readonly struct DecodedEnvelope
        {
            internal DecodedEnvelope(uint recordCount, Section meta, Section records, Section nodes, Section? payload)
            {
                RecordCount = recordCount;
                Meta = meta;
                Records = records;
                Nodes = nodes;
                Payload = payload;
            }
            internal uint RecordCount { get; }
            internal Section Meta { get; }
            internal Section Records { get; }
            internal Section Nodes { get; }
            internal Section? Payload { get; }
        }

        private readonly struct PropertyKey : IComparable<PropertyKey>
        {
            internal PropertyKey(ulong entity, uint property)
            {
                Entity = entity;
                Property = property;
            }
            private ulong Entity { get; }
            private uint Property { get; }
            public int CompareTo(PropertyKey other)
            {
                int entity = Entity.CompareTo(other.Entity);
                return entity != 0 ? entity : Property.CompareTo(other.Property);
            }
        }
    }
}
