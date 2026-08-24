using System;
using System.Buffers.Binary;
using System.Collections.Generic;
using System.Linq;
using System.Text;

namespace IronVaak
{
    public enum WireValueType : uint
    {
        U1 = 1,
        U8 = 2,
        U16 = 3,
        U32 = 4,
        I32 = 5,
        I64 = 6,
        F32 = 7,
        F64 = 8,
        Utf8 = 9,
        Bytes = 10,
    }

    public readonly struct WireId128 : IEquatable<WireId128>
    {
        private readonly ulong _first;
        private readonly ulong _second;

        public WireId128(ulong first, ulong second)
        {
            _first = first;
            _second = second;
        }

        public static WireId128 Zero => default;

        public static WireId128 FromBytes(ReadOnlySpan<byte> bytes)
        {
            if (bytes.Length != 16)
            {
                throw new ArgumentException("A wire ID is exactly 16 bytes.", nameof(bytes));
            }
            return new WireId128(
                BinaryPrimitives.ReadUInt64LittleEndian(bytes.Slice(0, 8)),
                BinaryPrimitives.ReadUInt64LittleEndian(bytes.Slice(8, 8)));
        }

        public static WireId128 FromGuid(Guid value) => FromBytes(value.ToByteArray());

        public void WriteTo(Span<byte> destination)
        {
            if (destination.Length < 16)
            {
                throw new ArgumentException("Destination must contain at least 16 bytes.", nameof(destination));
            }
            BinaryPrimitives.WriteUInt64LittleEndian(destination.Slice(0, 8), _first);
            BinaryPrimitives.WriteUInt64LittleEndian(destination.Slice(8, 8), _second);
        }

        public byte[] ToByteArray()
        {
            byte[] bytes = new byte[16];
            WriteTo(bytes);
            return bytes;
        }

        public Guid ToGuid() => new Guid(ToByteArray());

        public bool Equals(WireId128 other) => _first == other._first && _second == other._second;
        public override bool Equals(object? obj) => obj is WireId128 other && Equals(other);
        public override int GetHashCode() => unchecked((_first.GetHashCode() * 397) ^ _second.GetHashCode());
        public static bool operator ==(WireId128 left, WireId128 right) => left.Equals(right);
        public static bool operator !=(WireId128 left, WireId128 right) => !left.Equals(right);
        public override string ToString() => ToGuid().ToString("D");
    }

    public sealed class WireValue : IEquatable<WireValue>
    {
        private static readonly UTF8Encoding StrictUtf8 = new UTF8Encoding(false, true);
        private readonly ulong _bits;
        private readonly byte[] _bytes;

        private WireValue(WireValueType type, ulong bits, byte[]? bytes = null)
        {
            Type = type;
            _bits = bits;
            _bytes = bytes ?? Array.Empty<byte>();
        }

        public WireValueType Type { get; }
        public ReadOnlyMemory<byte> Payload => _bytes;
        internal ulong ScalarBits => _bits;

        public static WireValue U1(bool value) => new WireValue(WireValueType.U1, value ? 1UL : 0UL);
        public static WireValue U8(byte value) => new WireValue(WireValueType.U8, value);
        public static WireValue U16(ushort value) => new WireValue(WireValueType.U16, value);
        public static WireValue U32(uint value) => new WireValue(WireValueType.U32, value);
        public static WireValue I32(int value) => new WireValue(WireValueType.I32, unchecked((uint)value));
        public static WireValue I64(long value) => new WireValue(WireValueType.I64, unchecked((ulong)value));

        public static WireValue F32(float value)
        {
            RequireFinite(value, nameof(value));
            return new WireValue(WireValueType.F32, unchecked((uint)BitConverter.SingleToInt32Bits(value)));
        }

        public static WireValue F64(double value)
        {
            RequireFinite(value, nameof(value));
            return new WireValue(WireValueType.F64, unchecked((ulong)BitConverter.DoubleToInt64Bits(value)));
        }

        public static WireValue Utf8(string value)
        {
            if (value == null) throw new ArgumentNullException(nameof(value));
            return new WireValue(WireValueType.Utf8, 0, StrictUtf8.GetBytes(value));
        }

        public static WireValue Bytes(ReadOnlySpan<byte> value) =>
            new WireValue(WireValueType.Bytes, 0, value.ToArray());

        internal static WireValue FromEncoded(WireValueType type, ulong bits, ReadOnlySpan<byte> bytes)
        {
            switch (type)
            {
                case WireValueType.U1:
                    if (bits > 1 || bytes.Length != 0) throw new WireFormatException("U1 is not canonical.");
                    return U1(bits != 0);
                case WireValueType.U8:
                    if (bits > byte.MaxValue || bytes.Length != 0) throw new WireFormatException("U8 is not canonical.");
                    return U8((byte)bits);
                case WireValueType.U16:
                    if (bits > ushort.MaxValue || bytes.Length != 0) throw new WireFormatException("U16 is not canonical.");
                    return U16((ushort)bits);
                case WireValueType.U32:
                    if (bits > uint.MaxValue || bytes.Length != 0) throw new WireFormatException("U32 is not canonical.");
                    return U32((uint)bits);
                case WireValueType.I32:
                    if (bits > uint.MaxValue || bytes.Length != 0) throw new WireFormatException("I32 is not canonical.");
                    return I32(unchecked((int)(uint)bits));
                case WireValueType.I64:
                    if (bytes.Length != 0) throw new WireFormatException("I64 is not canonical.");
                    return I64(unchecked((long)bits));
                case WireValueType.F32:
                    if (bits > uint.MaxValue || bytes.Length != 0) throw new WireFormatException("F32 is not canonical.");
                    return F32(BitConverter.Int32BitsToSingle(unchecked((int)(uint)bits)));
                case WireValueType.F64:
                    if (bytes.Length != 0) throw new WireFormatException("F64 is not canonical.");
                    return F64(BitConverter.Int64BitsToDouble(unchecked((long)bits)));
                case WireValueType.Utf8:
                    try
                    {
                        string value = StrictUtf8.GetString(bytes.ToArray());
                        return new WireValue(WireValueType.Utf8, 0, StrictUtf8.GetBytes(value));
                    }
                    catch (DecoderFallbackException error)
                    {
                        throw new WireFormatException("UTF-8 payload is invalid.", error);
                    }
                case WireValueType.Bytes:
                    return Bytes(bytes);
                default:
                    throw new WireFormatException("The value type tag is unsupported.");
            }
        }

        public bool AsU1() => Require(WireValueType.U1, _bits != 0);
        public byte AsU8() => Require(WireValueType.U8, (byte)_bits);
        public ushort AsU16() => Require(WireValueType.U16, (ushort)_bits);
        public uint AsU32() => Require(WireValueType.U32, (uint)_bits);
        public int AsI32() => Require(WireValueType.I32, unchecked((int)(uint)_bits));
        public long AsI64() => Require(WireValueType.I64, unchecked((long)_bits));
        public float AsF32() => Require(WireValueType.F32, BitConverter.Int32BitsToSingle(unchecked((int)(uint)_bits)));
        public double AsF64() => Require(WireValueType.F64, BitConverter.Int64BitsToDouble(unchecked((long)_bits)));

        public string AsUtf8()
        {
            if (Type != WireValueType.Utf8) throw WrongType(WireValueType.Utf8);
            return StrictUtf8.GetString(_bytes);
        }

        public byte[] AsBytes()
        {
            if (Type != WireValueType.Bytes) throw WrongType(WireValueType.Bytes);
            return (byte[])_bytes.Clone();
        }

        private T Require<T>(WireValueType expected, T value)
        {
            if (Type != expected) throw WrongType(expected);
            return value;
        }

        private InvalidOperationException WrongType(WireValueType expected) =>
            new InvalidOperationException($"Expected {expected}, but the value is {Type}.");

        private static void RequireFinite(float value, string name)
        {
            if (float.IsNaN(value) || float.IsInfinity(value))
                throw new ArgumentOutOfRangeException(name, "NaN and infinity are not wire values.");
        }

        private static void RequireFinite(double value, string name)
        {
            if (double.IsNaN(value) || double.IsInfinity(value))
                throw new ArgumentOutOfRangeException(name, "NaN and infinity are not wire values.");
        }

        public bool Equals(WireValue? other)
        {
            return other != null && Type == other.Type && _bits == other._bits && _bytes.SequenceEqual(other._bytes);
        }

        public override bool Equals(object? obj) => Equals(obj as WireValue);
        public override int GetHashCode()
        {
            int hash = unchecked(((int)Type * 397) ^ _bits.GetHashCode());
            foreach (byte value in _bytes) hash = unchecked(hash * 31 + value);
            return hash;
        }
    }

    public sealed class SnapshotEntry
    {
        public SnapshotEntry(ulong entityId, uint propertyId, ulong propertyRevision, WireValue value)
        {
            EntityId = entityId;
            PropertyId = propertyId;
            PropertyRevision = propertyRevision;
            Value = value ?? throw new ArgumentNullException(nameof(value));
        }

        public ulong EntityId { get; }
        public uint PropertyId { get; }
        public ulong PropertyRevision { get; }
        public WireValue Value { get; }
    }

    public sealed class SettingsSnapshot
    {
        public SettingsSnapshot(WireId128 schemaId, WireId128 sessionId, ulong snapshotRevision, IEnumerable<SnapshotEntry> entries)
        {
            SchemaId = schemaId;
            SessionId = sessionId;
            SnapshotRevision = snapshotRevision;
            Entries = Canonical(entries, entry => (entry.EntityId, entry.PropertyId), "snapshot");
        }

        public WireId128 SchemaId { get; }
        public WireId128 SessionId { get; }
        public ulong SnapshotRevision { get; }
        public IReadOnlyList<SnapshotEntry> Entries { get; }

        internal static IReadOnlyList<T> Canonical<T>(IEnumerable<T> entries, Func<T, (ulong, uint)> key, string kind)
        {
            if (entries == null) throw new ArgumentNullException(nameof(entries));
            List<T> copy = entries.ToList();
            copy.Sort((left, right) =>
            {
                (ulong entity, uint property) a = key(left);
                (ulong entity, uint property) b = key(right);
                int entity = a.entity.CompareTo(b.entity);
                return entity != 0 ? entity : a.property.CompareTo(b.property);
            });
            for (int index = 1; index < copy.Count; index++)
            {
                if (key(copy[index - 1]) == key(copy[index]))
                    throw new ArgumentException($"The {kind} contains a duplicate property key.", nameof(entries));
            }
            return copy.AsReadOnly();
        }
    }

    public sealed class PatchEntry
    {
        public PatchEntry(
            ulong entityId,
            uint propertyId,
            ulong expectedPropertyRevision,
            uint capabilityTableIndex,
            WireValue value)
        {
            EntityId = entityId;
            PropertyId = propertyId;
            ExpectedPropertyRevision = expectedPropertyRevision;
            CapabilityTableIndex = capabilityTableIndex;
            Value = value ?? throw new ArgumentNullException(nameof(value));
        }

        public ulong EntityId { get; }
        public uint PropertyId { get; }
        public ulong ExpectedPropertyRevision { get; }
        public uint CapabilityTableIndex { get; }
        public WireValue Value { get; }
    }

    public sealed class SettingsPatch
    {
        public SettingsPatch(
            WireId128 schemaId,
            WireId128 sessionId,
            WireId128 runId,
            WireId128 transactionId,
            ulong baseSnapshotRevision,
            IEnumerable<PatchEntry> entries)
        {
            SchemaId = schemaId;
            SessionId = sessionId;
            RunId = runId;
            TransactionId = transactionId;
            BaseSnapshotRevision = baseSnapshotRevision;
            Entries = SettingsSnapshot.Canonical(entries, entry => (entry.EntityId, entry.PropertyId), "patch");
        }

        public WireId128 SchemaId { get; }
        public WireId128 SessionId { get; }
        public WireId128 RunId { get; }
        public WireId128 TransactionId { get; }
        public ulong BaseSnapshotRevision { get; }
        public IReadOnlyList<PatchEntry> Entries { get; }
    }

    public sealed class HostBindingDefinition
    {
        public HostBindingDefinition(
            string name,
            WireValueType valueType,
            ulong entityId,
            uint propertyId,
            uint capabilityTableIndex = 0)
        {
            Name = name ?? throw new ArgumentNullException(nameof(name));
            if (name.Length == 0) throw new ArgumentException("Host binding name cannot be empty.", nameof(name));
            ValueType = valueType;
            EntityId = entityId;
            PropertyId = propertyId;
            CapabilityTableIndex = capabilityTableIndex;
        }

        public string Name { get; }
        public WireValueType ValueType { get; }
        public ulong EntityId { get; }
        public uint PropertyId { get; }
        public uint CapabilityTableIndex { get; }
    }

    public readonly struct SourceSpan
    {
        public SourceSpan(ulong start, ulong length)
        {
            Start = start;
            Length = length;
        }

        public ulong Start { get; }
        public ulong Length { get; }
    }

    public sealed class IronVaakDiagnostic
    {
        internal IronVaakDiagnostic(ushort severity, uint origin, uint stableCode, SourceSpan span, ulong incidentId, string message)
        {
            Severity = severity;
            Origin = origin;
            StableCode = stableCode;
            Span = span;
            IncidentId = incidentId;
            Message = message;
        }

        public ushort Severity { get; }
        public uint Origin { get; }
        public uint StableCode { get; }
        public SourceSpan Span { get; }
        public ulong IncidentId { get; }
        public string Message { get; }
    }

    public sealed class TopLevelOutcome
    {
        internal TopLevelOutcome(Interop.TopLevelKind kind, WireValue? value, SourceSpan span)
        {
            Kind = kind;
            Value = value;
            Span = span;
        }

        public Interop.TopLevelKind Kind { get; }
        public WireValue? Value { get; }
        public SourceSpan Span { get; }
    }

    public sealed class ExecutionReport
    {
        internal ExecutionReport(
            Interop.ProgramStatus status,
            TopLevelOutcome topLevel,
            SettingsPatch patch,
            byte[] patchWire,
            IReadOnlyList<IronVaakDiagnostic> diagnostics)
        {
            Status = status;
            TopLevel = topLevel;
            Patch = patch;
            PatchWire = patchWire;
            Diagnostics = diagnostics;
        }

        public Interop.ProgramStatus Status { get; }
        public TopLevelOutcome TopLevel { get; }
        public SettingsPatch Patch { get; }
        public ReadOnlyMemory<byte> PatchWire { get; }
        public IReadOnlyList<IronVaakDiagnostic> Diagnostics { get; }
    }

    public class IronVaakException : Exception
    {
        public IronVaakException(string message) : base(message) { }
        public IronVaakException(string message, Exception inner) : base(message, inner) { }
    }

    public sealed class IronVaakTransportException : IronVaakException
    {
        internal IronVaakTransportException(Interop.TransportStatus status, IReadOnlyList<IronVaakDiagnostic> diagnostics)
            : base(diagnostics.Count == 0 ? $"IRON VAAK transport failed with {status}." : diagnostics[0].Message)
        {
            Status = status;
            Diagnostics = diagnostics;
        }

        public Interop.TransportStatus Status { get; }
        public IReadOnlyList<IronVaakDiagnostic> Diagnostics { get; }
    }

    public sealed class WireFormatException : IronVaakException
    {
        public WireFormatException(string message) : base(message) { }
        public WireFormatException(string message, Exception inner) : base(message, inner) { }
    }
}
