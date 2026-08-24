using System;
using System.Runtime.InteropServices;

namespace IronVaak.Interop
{
    public enum TransportStatus : uint
    {
        Ok = 0,
        InvalidArgument = 1,
        AbiMismatch = 2,
        MalformedWire = 3,
        LimitExceeded = 4,
        StaleHandle = 5,
        Busy = 6,
        Poisoned = 7,
        BufferTooSmall = 8,
        InternalPanic = 9,
    }

    [Flags]
    public enum NativeFeature : ulong
    {
        PrepareRunMany = 1UL << 0,
        SnapshotPatch = 1UL << 1,
        DiagnosticCopy = 1UL << 2,
        ThreadSafeHandles = 1UL << 3,
    }

    public enum ProgramStatus : ushort
    {
        Completed = 1,
        ProgramError = 2,
        HostContractError = 3,
        InternalPanic = 4,
    }

    public enum TopLevelKind : ushort
    {
        None = 0,
        Akasha = 1,
        Value = 2,
        Paradox = 3,
        Escape = 4,
        UnsupportedAggregate = 5,
    }

    [StructLayout(LayoutKind.Sequential, Pack = 8)]
    internal struct NativeAbiInfo
    {
        internal uint StructSize;
        internal ushort AbiMajor;
        internal ushort AbiMinor;
        internal ulong SupportedFeatures;
        internal ulong RequiredAlignment;
        internal ulong MaxWireBytes;
        internal ulong Reserved0;
        internal ulong Reserved1;
        internal ulong Reserved2;
        internal ulong Reserved3;
    }

    [StructLayout(LayoutKind.Sequential, Pack = 8)]
    internal struct NativeCallEnvelope
    {
        internal uint StructSize;
        internal uint TransportStatus;
        internal uint DetailCode;
        internal uint Flags;
        internal ulong RequiredBytes;
        internal ulong WrittenBytes;
        internal ulong DiagnosticId;
        internal ulong Reserved0;
        internal ulong Reserved1;
        internal ulong Reserved2;

        internal TransportStatus Status => (TransportStatus)TransportStatus;
    }

    [StructLayout(LayoutKind.Sequential, Pack = 8)]
    internal struct NativeDiagnosticEnvelope
    {
        internal uint StructSize;
        internal ushort Severity;
        internal ushort Reserved0;
        internal uint Origin;
        internal uint StableCode;
        internal ulong SpanStart;
        internal ulong SpanLength;
        internal ulong MessageOffset;
        internal ulong MessageLength;
        internal ulong IncidentId;
        internal ulong Reserved1;
    }

    [StructLayout(LayoutKind.Sequential, Pack = 8)]
    internal struct NativeHostLayoutEntry
    {
        internal uint StructSize;
        internal uint Flags;
        internal uint SlotIndex;
        internal uint ValueType;
        internal ulong EntityId;
        internal uint PropertyId;
        internal uint CapabilityTableIndex;
        internal ulong NameOffset;
        internal ulong NameLength;
        internal ulong Reserved1;
    }

    [StructLayout(LayoutKind.Sequential, Pack = 8)]
    internal struct NativeRunnerReportInfo
    {
        internal uint StructSize;
        internal ushort ProgramStatus;
        internal ushort TopLevelKind;
        internal uint TopLevelValueType;
        internal uint Flags;
        internal ulong TopLevelScalarBits;
        internal ulong TopLevelSpanStart;
        internal ulong TopLevelSpanLength;
        internal ulong PatchBytes;
        internal ulong TopLevelBytes;
        internal ulong DiagnosticCount;
    }

    public readonly struct AbiInfo
    {
        internal AbiInfo(NativeAbiInfo native)
        {
            Major = native.AbiMajor;
            Minor = native.AbiMinor;
            Features = (NativeFeature)native.SupportedFeatures;
            RequiredAlignment = native.RequiredAlignment;
            MaxWireBytes = native.MaxWireBytes;
        }

        public ushort Major { get; }
        public ushort Minor { get; }
        public NativeFeature Features { get; }
        public ulong RequiredAlignment { get; }
        public ulong MaxWireBytes { get; }
    }
}
