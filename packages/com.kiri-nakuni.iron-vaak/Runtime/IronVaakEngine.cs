using IronVaak.Interop;
using System;
using System.Collections.Generic;
using System.Linq;
using System.Runtime.InteropServices;
using System.Text;

namespace IronVaak
{
    public sealed class PreparationResult
    {
        internal PreparationResult(IronVaakProgram? program, IReadOnlyList<IronVaakDiagnostic> diagnostics)
        {
            Program = program;
            Diagnostics = diagnostics;
        }

        public IronVaakProgram? Program { get; }
        public IReadOnlyList<IronVaakDiagnostic> Diagnostics { get; }
        public bool Succeeded => Program != null;
    }

    public sealed class IronVaakCompilationException : IronVaakException
    {
        internal IronVaakCompilationException(IReadOnlyList<IronVaakDiagnostic> diagnostics)
            : base(diagnostics.Count == 0 ? "Vaak preparation failed." : diagnostics[0].Message)
        {
            Diagnostics = diagnostics;
        }

        public IReadOnlyList<IronVaakDiagnostic> Diagnostics { get; }
    }

    /// <summary>Owns one native handle domain. Programs and runners retain it through SafeHandle references.</summary>
    public sealed class IronVaakEngine : IDisposable
    {
        private static readonly UTF8Encoding StrictUtf8 = new UTF8Encoding(false, true);
        private readonly ContextSafeHandle _context;
        private readonly object _gate = new object();
        private bool _disposed;

        public IronVaakEngine()
        {
            VerifyManagedLayout();
            uint status = NativeMethods.iron_vaak_v0_abi_info(out NativeAbiInfo nativeAbi, out NativeCallEnvelope envelope);
            if (status != (uint)TransportStatus.Ok || envelope.Status != TransportStatus.Ok)
                throw new IronVaakTransportException((TransportStatus)status, Array.Empty<IronVaakDiagnostic>());
            Abi = new AbiInfo(nativeAbi);
            NativeFeature required = NativeFeature.PrepareRunMany |
                                     NativeFeature.SnapshotPatch |
                                     NativeFeature.DiagnosticCopy |
                                     NativeFeature.ThreadSafeHandles;
            if (Abi.Major != 0 || (Abi.Features & required) != required || Abi.RequiredAlignment != 8)
                throw new IronVaakException("The loaded IRON VAAK native ABI is incompatible with this managed facade.");

            status = NativeMethods.iron_vaak_v0_context_create(out ulong context, out envelope);
            if (status != (uint)TransportStatus.Ok || context == 0)
                throw new IronVaakTransportException((TransportStatus)status, Array.Empty<IronVaakDiagnostic>());
            _context = new ContextSafeHandle(context);
        }

        public AbiInfo Abi { get; }

        public unsafe PreparationResult Prepare(string source, IReadOnlyList<HostBindingDefinition> bindings)
        {
            lock (_gate)
            {
                ThrowIfDisposed();
                if (source == null) throw new ArgumentNullException(nameof(source));
                if (bindings == null) throw new ArgumentNullException(nameof(bindings));
                byte[] sourceBytes = StrictUtf8.GetBytes(source);
                BuildLayout(bindings, out NativeHostLayoutEntry[] entries, out byte[] names);

                fixed (byte* sourcePointer = sourceBytes)
                fixed (NativeHostLayoutEntry* entryPointer = entries)
                fixed (byte* namePointer = names)
                {
                    uint status = NativeMethods.iron_vaak_v0_prepare(
                        _context.Token,
                        sourcePointer,
                        (ulong)sourceBytes.Length,
                        entryPointer,
                        (ulong)entries.Length,
                        namePointer,
                        (ulong)names.Length,
                        out ulong prepared,
                        out NativeCallEnvelope envelope);
                    PreparedSafeHandle? preparedHandle = prepared == 0 ? null : new PreparedSafeHandle(_context, prepared);
                    try
                    {
                        IReadOnlyList<IronVaakDiagnostic> diagnostics = ReadOwnedDiagnostics(_context, envelope.DiagnosticId);
                        if (status != (uint)TransportStatus.Ok || envelope.Status != TransportStatus.Ok)
                            throw new IronVaakTransportException(envelope.Status, diagnostics);
                        IronVaakProgram? program = preparedHandle == null
                            ? null
                            : new IronVaakProgram(_context, preparedHandle);
                        preparedHandle = null;
                        GC.KeepAlive(_context);
                        return new PreparationResult(program, diagnostics);
                    }
                    finally
                    {
                        preparedHandle?.Dispose();
                    }
                }
            }
        }

        public IronVaakProgram PrepareOrThrow(string source, IReadOnlyList<HostBindingDefinition> bindings)
        {
            PreparationResult result = Prepare(source, bindings);
            return result.Program ?? throw new IronVaakCompilationException(result.Diagnostics);
        }

        public void Dispose()
        {
            lock (_gate)
            {
                if (_disposed) return;
                _disposed = true;
                _context.Dispose();
            }
        }

        private void ThrowIfDisposed()
        {
            if (_disposed) throw new ObjectDisposedException(nameof(IronVaakEngine));
        }

        private static void VerifyManagedLayout()
        {
            if (IntPtr.Size != 8) throw new PlatformNotSupportedException("IRON VAAK v0 requires a 64-bit process.");
            if (!BitConverter.IsLittleEndian) throw new PlatformNotSupportedException("IRON VAAK v0 requires a little-endian process.");
            if (Marshal.SizeOf<NativeAbiInfo>() != 64 ||
                Marshal.SizeOf<NativeCallEnvelope>() != 64 ||
                Marshal.SizeOf<NativeDiagnosticEnvelope>() != 64 ||
                Marshal.SizeOf<NativeHostLayoutEntry>() != 56 ||
                Marshal.SizeOf<NativeRunnerReportInfo>() != 64)
                throw new IronVaakException("Managed and native fixed-width records do not agree.");
        }

        private static void BuildLayout(
            IReadOnlyList<HostBindingDefinition> bindings,
            out NativeHostLayoutEntry[] entries,
            out byte[] names)
        {
            if (bindings.Count > ushort.MaxValue) throw new ArgumentException("Too many host bindings.", nameof(bindings));
            var seenNames = new HashSet<string>(StringComparer.Ordinal);
            var seenProperties = new HashSet<(ulong, uint)>();
            var nameBytes = new List<byte>();
            entries = new NativeHostLayoutEntry[bindings.Count];
            uint width = checked((uint)Marshal.SizeOf<NativeHostLayoutEntry>());
            for (int index = 0; index < bindings.Count; index++)
            {
                HostBindingDefinition binding = bindings[index] ?? throw new ArgumentException("A host binding is null.", nameof(bindings));
                if (!seenNames.Add(binding.Name)) throw new ArgumentException("Host binding names must be unique.", nameof(bindings));
                if (!seenProperties.Add((binding.EntityId, binding.PropertyId)))
                    throw new ArgumentException("Host binding property keys must be unique.", nameof(bindings));
                if ((uint)binding.ValueType < (uint)WireValueType.U1 || (uint)binding.ValueType > (uint)WireValueType.Bytes)
                    throw new ArgumentException("A host binding uses an unsupported v0 value type.", nameof(bindings));
                byte[] encodedName = StrictUtf8.GetBytes(binding.Name);
                ulong offset = (ulong)nameBytes.Count;
                nameBytes.AddRange(encodedName);
                entries[index] = new NativeHostLayoutEntry
                {
                    StructSize = width,
                    SlotIndex = checked((uint)index),
                    ValueType = (uint)binding.ValueType,
                    EntityId = binding.EntityId,
                    PropertyId = binding.PropertyId,
                    CapabilityTableIndex = binding.CapabilityTableIndex,
                    NameOffset = offset,
                    NameLength = (ulong)encodedName.Length,
                };
            }
            names = nameBytes.ToArray();
        }

        internal static unsafe IReadOnlyList<IronVaakDiagnostic> ReadOwnedDiagnostics(ContextSafeHandle context, ulong diagnosticId)
        {
            if (diagnosticId == 0) return Array.Empty<IronVaakDiagnostic>();
            try
            {
                uint status = NativeMethods.iron_vaak_v0_diagnostics_count(context.Token, diagnosticId, out ulong count, out NativeCallEnvelope envelope);
                if (status != (uint)TransportStatus.Ok) throw new IronVaakTransportException(envelope.Status, Array.Empty<IronVaakDiagnostic>());
                if (count > ushort.MaxValue) throw new IronVaakException("Native diagnostic count exceeds the v0 limit.");
                var diagnostics = new List<IronVaakDiagnostic>((int)count);
                for (ulong index = 0; index < count; index++)
                {
                    status = NativeMethods.iron_vaak_v0_diagnostic_copy(
                        context.Token, diagnosticId, index, out NativeDiagnosticEnvelope native, null, 0, out envelope);
                    byte[] message;
                    if (status == (uint)TransportStatus.BufferTooSmall)
                    {
                        message = NewBuffer(envelope.RequiredBytes);
                        fixed (byte* messagePointer = message)
                        {
                            status = NativeMethods.iron_vaak_v0_diagnostic_copy(
                                context.Token, diagnosticId, index, out native,
                                messagePointer, (ulong)message.Length, out envelope);
                        }
                    }
                    else
                    {
                        message = Array.Empty<byte>();
                    }
                    if (status != (uint)TransportStatus.Ok) throw new IronVaakTransportException(envelope.Status, diagnostics);
                    diagnostics.Add(ToDiagnostic(native, message));
                }
                return diagnostics.AsReadOnly();
            }
            finally
            {
                NativeMethods.iron_vaak_v0_diagnostics_destroy(context.Token, diagnosticId, out _);
                GC.KeepAlive(context);
            }
        }

        internal static IronVaakDiagnostic ToDiagnostic(NativeDiagnosticEnvelope native, byte[] message)
        {
            string text;
            try { text = StrictUtf8.GetString(message); }
            catch (DecoderFallbackException error) { throw new IronVaakException("Native diagnostic is not UTF-8.", error); }
            return new IronVaakDiagnostic(
                native.Severity,
                native.Origin,
                native.StableCode,
                new SourceSpan(native.SpanStart, native.SpanLength),
                native.IncidentId,
                text);
        }

        internal static byte[] NewBuffer(ulong length)
        {
            if (length > int.MaxValue) throw new IronVaakException("Native output exceeds the managed buffer limit.");
            return new byte[(int)length];
        }
    }

    public sealed class IronVaakProgram : IDisposable
    {
        private readonly ContextSafeHandle _context;
        private readonly PreparedSafeHandle _prepared;
        private readonly object _gate = new object();
        private bool _disposed;

        internal IronVaakProgram(ContextSafeHandle context, PreparedSafeHandle prepared)
        {
            _context = context;
            _prepared = prepared;
        }

        public IronVaakRunner CreateRunner()
        {
            lock (_gate)
            {
                if (_disposed) throw new ObjectDisposedException(nameof(IronVaakProgram));
                uint status = NativeMethods.iron_vaak_v0_runner_create(
                    _context.Token, _prepared.Token, out ulong runner, out NativeCallEnvelope envelope);
                RunnerSafeHandle? runnerHandle = runner == 0 ? null : new RunnerSafeHandle(_context, runner);
                try
                {
                    IReadOnlyList<IronVaakDiagnostic> diagnostics = IronVaakEngine.ReadOwnedDiagnostics(_context, envelope.DiagnosticId);
                    if (status != (uint)TransportStatus.Ok || runnerHandle == null)
                        throw new IronVaakTransportException(envelope.Status, diagnostics);
                    IronVaakRunner result = new IronVaakRunner(_context, runnerHandle);
                    runnerHandle = null;
                    GC.KeepAlive(_prepared);
                    return result;
                }
                finally
                {
                    runnerHandle?.Dispose();
                }
            }
        }

        public void Dispose()
        {
            lock (_gate)
            {
                if (_disposed) return;
                _disposed = true;
                _prepared.Dispose();
            }
        }
    }

    public sealed class IronVaakRunner : IDisposable
    {
        private readonly ContextSafeHandle _context;
        private readonly RunnerSafeHandle _runner;
        private readonly object _gate = new object();
        private bool _disposed;

        internal IronVaakRunner(ContextSafeHandle context, RunnerSafeHandle runner)
        {
            _context = context;
            _runner = runner;
        }

        public unsafe ExecutionReport Run(SettingsSnapshot snapshot, WireId128 runId, WireId128 transactionId)
        {
            if (snapshot == null) throw new ArgumentNullException(nameof(snapshot));
            lock (_gate)
            {
                if (_disposed) throw new ObjectDisposedException(nameof(IronVaakRunner));
                byte[] snapshotWire = ExchangeCodec.EncodeSnapshot(snapshot);
                byte[] runBytes = runId.ToByteArray();
                byte[] transactionBytes = transactionId.ToByteArray();
                fixed (byte* snapshotPointer = snapshotWire)
                fixed (byte* runPointer = runBytes)
                fixed (byte* transactionPointer = transactionBytes)
                {
                    uint status = NativeMethods.iron_vaak_v0_runner_run(
                        _context.Token, _runner.Token,
                        snapshotPointer, (ulong)snapshotWire.Length,
                        runPointer, transactionPointer,
                        out NativeCallEnvelope envelope);
                    IReadOnlyList<IronVaakDiagnostic> diagnostics = IronVaakEngine.ReadOwnedDiagnostics(_context, envelope.DiagnosticId);
                    if (status != (uint)TransportStatus.Ok)
                        throw new IronVaakTransportException(envelope.Status, diagnostics);
                }

                try
                {
                    return CopyReport(snapshot, runId, transactionId);
                }
                finally
                {
                    uint clear = NativeMethods.iron_vaak_v0_runner_clear_report(_context.Token, _runner.Token, out NativeCallEnvelope envelope);
                    if (clear != (uint)TransportStatus.Ok)
                    {
                        IReadOnlyList<IronVaakDiagnostic> diagnostics = IronVaakEngine.ReadOwnedDiagnostics(_context, envelope.DiagnosticId);
                        throw new IronVaakTransportException(envelope.Status, diagnostics);
                    }
                    GC.KeepAlive(_runner);
                }
            }
        }

        private unsafe ExecutionReport CopyReport(SettingsSnapshot snapshot, WireId128 runId, WireId128 transactionId)
        {
            uint status = NativeMethods.iron_vaak_v0_runner_report_info(
                _context.Token, _runner.Token, out NativeRunnerReportInfo info, out NativeCallEnvelope envelope);
            if (status != (uint)TransportStatus.Ok) ThrowTransport(envelope);
            byte[] patchWire = CopyPatch(info.PatchBytes);
            SettingsPatch patch = ExchangeCodec.DecodePatch(patchWire);
            if (patch.SchemaId != snapshot.SchemaId || patch.SessionId != snapshot.SessionId ||
                patch.RunId != runId || patch.TransactionId != transactionId ||
                patch.BaseSnapshotRevision != snapshot.SnapshotRevision)
                throw new IronVaakException("Native report identity does not match the run request.");
            byte[] topBytes = CopyTopLevel(info.TopLevelBytes);
            TopLevelOutcome top = DecodeTopLevel(info, topBytes);
            IReadOnlyList<IronVaakDiagnostic> diagnostics = CopyReportDiagnostics(info.DiagnosticCount);
            return new ExecutionReport((ProgramStatus)info.ProgramStatus, top, patch, patchWire, diagnostics);
        }

        private unsafe byte[] CopyPatch(ulong length)
        {
            byte[] output = IronVaakEngine.NewBuffer(length);
            fixed (byte* pointer = output)
            {
                uint status = NativeMethods.iron_vaak_v0_runner_report_patch_copy(
                    _context.Token, _runner.Token, pointer, (ulong)output.Length, out NativeCallEnvelope envelope);
                if (status != (uint)TransportStatus.Ok || envelope.WrittenBytes != (ulong)output.Length) ThrowTransport(envelope);
            }
            return output;
        }

        private unsafe byte[] CopyTopLevel(ulong length)
        {
            byte[] output = IronVaakEngine.NewBuffer(length);
            fixed (byte* pointer = output)
            {
                uint status = NativeMethods.iron_vaak_v0_runner_report_top_level_copy(
                    _context.Token, _runner.Token, pointer, (ulong)output.Length, out NativeCallEnvelope envelope);
                if (status != (uint)TransportStatus.Ok || envelope.WrittenBytes != (ulong)output.Length) ThrowTransport(envelope);
            }
            return output;
        }

        private unsafe IReadOnlyList<IronVaakDiagnostic> CopyReportDiagnostics(ulong count)
        {
            if (count > ushort.MaxValue) throw new IronVaakException("Native diagnostic count exceeds the v0 limit.");
            var diagnostics = new List<IronVaakDiagnostic>((int)count);
            for (ulong index = 0; index < count; index++)
            {
                uint status = NativeMethods.iron_vaak_v0_runner_report_diagnostic_copy(
                    _context.Token, _runner.Token, index, out NativeDiagnosticEnvelope native, null, 0, out NativeCallEnvelope envelope);
                byte[] message;
                if (status == (uint)TransportStatus.BufferTooSmall)
                {
                    message = IronVaakEngine.NewBuffer(envelope.RequiredBytes);
                    fixed (byte* pointer = message)
                    {
                        status = NativeMethods.iron_vaak_v0_runner_report_diagnostic_copy(
                            _context.Token, _runner.Token, index, out native,
                            pointer, (ulong)message.Length, out envelope);
                    }
                }
                else
                {
                    message = Array.Empty<byte>();
                }
                if (status != (uint)TransportStatus.Ok) ThrowTransport(envelope);
                diagnostics.Add(IronVaakEngine.ToDiagnostic(native, message));
            }
            return diagnostics.AsReadOnly();
        }

        private static TopLevelOutcome DecodeTopLevel(NativeRunnerReportInfo info, byte[] bytes)
        {
            TopLevelKind kind = (TopLevelKind)info.TopLevelKind;
            WireValue? value = null;
            if (kind == TopLevelKind.Value)
            {
                WireValueType type = (WireValueType)info.TopLevelValueType;
                value = WireValue.FromEncoded(type, info.TopLevelScalarBits, bytes);
            }
            return new TopLevelOutcome(kind, value, new SourceSpan(info.TopLevelSpanStart, info.TopLevelSpanLength));
        }

        private void ThrowTransport(NativeCallEnvelope envelope)
        {
            IReadOnlyList<IronVaakDiagnostic> diagnostics = IronVaakEngine.ReadOwnedDiagnostics(_context, envelope.DiagnosticId);
            throw new IronVaakTransportException(envelope.Status, diagnostics);
        }

        public void Dispose()
        {
            lock (_gate)
            {
                if (_disposed) return;
                _disposed = true;
                _runner.Dispose();
            }
        }
    }
}
