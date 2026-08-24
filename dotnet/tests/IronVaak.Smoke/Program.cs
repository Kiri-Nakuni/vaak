using IronVaak;
using IronVaak.Interop;
using IronVaak.Scripting;
using System;
using System.Buffers.Binary;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;

internal static class Program
{
    private static int Main()
    {
        try
        {
            CodecRoundTrip();
            MalformedWireRangeIsRejected();
            PrepareRunMany();
            NegativeI32TopLevel();
            ByteStringTopLevel();
            PrepareDiagnostics();
            RuntimeErrorKeepsPatch();
            HandlesSurviveParentDispose();
            PublicRuntimeContract();
            LuaComposition();
            LuaConflictIsRejected();
            Console.WriteLine("IRON VAAK managed smoke: 11 passed");
            return 0;
        }
        catch (Exception error)
        {
            Console.Error.WriteLine(error);
            return 1;
        }
    }

    private static SettingsSnapshot Snapshot(long value, uint property = 10)
    {
        return new SettingsSnapshot(
            new WireId128(1, 2),
            new WireId128(3, 4),
            5,
            new[] { new SnapshotEntry(0, property, 6, WireValue.I64(value)) });
    }

    private static IReadOnlyList<HostBindingDefinition> Layout(uint property = 10)
    {
        return new[] { new HostBindingDefinition("n", WireValueType.I64, 0, property, 23) };
    }

    private static void CodecRoundTrip()
    {
        var snapshot = new SettingsSnapshot(
            new WireId128(1, 2),
            new WireId128(3, 4),
            7,
            new[]
            {
                new SnapshotEntry(0, 10, 8, WireValue.I64(-42)),
                new SnapshotEntry(0, 11, 9, WireValue.Utf8("鉄の雨☂")),
            });
        SettingsSnapshot decoded = ExchangeCodec.DecodeSnapshot(ExchangeCodec.EncodeSnapshot(snapshot));
        Equal(2, decoded.Entries.Count, "snapshot entry count");
        Equal(-42L, decoded.Entries[0].Value.AsI64(), "snapshot i64");
        Equal("鉄の雨☂", decoded.Entries[1].Value.AsUtf8(), "snapshot utf8");

        var patch = new SettingsPatch(
            snapshot.SchemaId, snapshot.SessionId, new WireId128(5, 6), new WireId128(7, 8), 7,
            new[] { new PatchEntry(0, 10, 8, 23, WireValue.F32(1.25f)) });
        SettingsPatch decodedPatch = ExchangeCodec.DecodePatch(ExchangeCodec.EncodePatch(patch));
        Equal(1.25f, decodedPatch.Entries[0].Value.AsF32(), "patch f32");
    }

    private static void MalformedWireRangeIsRejected()
    {
        byte[] malformed = ExchangeCodec.EncodeSnapshot(Snapshot(1));
        BinaryPrimitives.WriteUInt64LittleEndian(malformed.AsSpan(40, 8), 0x7fff_fff8UL);
        BinaryPrimitives.WriteUInt64LittleEndian(malformed.AsSpan(48, 8), 16UL);
        bool rejected = false;
        try
        {
            ExchangeCodec.DecodeSnapshot(malformed);
        }
        catch (WireFormatException)
        {
            rejected = true;
        }
        True(rejected, "overflowing wire section range");
    }

    private static void PrepareRunMany()
    {
        using var engine = new IronVaakEngine();
        True((engine.Abi.Features & NativeFeature.ThreadSafeHandles) != 0, "thread-safe handle feature");
        using IronVaakProgram program = engine.PrepareOrThrow("n += 1; n", Layout());
        using IronVaakRunner runner = program.CreateRunner();
        foreach (long value in new[] { 1L, 10L, 100L })
        {
            ExecutionReport report = runner.Run(Snapshot(value), new WireId128(7, 7), new WireId128(8, 8));
            Equal(ProgramStatus.Completed, report.Status, "program status");
            Equal(value + 1, report.TopLevel.Value!.AsI64(), "top-level result");
            Equal(value + 1, report.Patch.Entries[0].Value.AsI64(), "patch result");
            Equal(23U, report.Patch.Entries[0].CapabilityTableIndex, "capability index");
        }
    }

    private static void NegativeI32TopLevel()
    {
        using var engine = new IronVaakEngine();
        var layout = new[] { new HostBindingDefinition("n", WireValueType.I32, 0, 10, 23) };
        var snapshot = new SettingsSnapshot(
            new WireId128(1, 2),
            new WireId128(3, 4),
            5,
            new[] { new SnapshotEntry(0, 10, 6, WireValue.I32(-42)) });
        using IronVaakProgram program = engine.PrepareOrThrow("n", layout);
        using IronVaakRunner runner = program.CreateRunner();
        ExecutionReport report = runner.Run(snapshot, new WireId128(13, 13), new WireId128(14, 14));
        Equal(-42, report.TopLevel.Value!.AsI32(), "negative i32 top-level result");
    }

    private static void ByteStringTopLevel()
    {
        using var engine = new IronVaakEngine();
        var layout = new[] { new HostBindingDefinition("label", WireValueType.Utf8, 0, 12) };
        var snapshot = new SettingsSnapshot(
            new WireId128(1, 2),
            new WireId128(3, 4),
            5,
            new[] { new SnapshotEntry(0, 12, 6, WireValue.Utf8("鉄の雨☂")) });
        using IronVaakProgram program = engine.PrepareOrThrow("label", layout);
        using IronVaakRunner runner = program.CreateRunner();
        ExecutionReport report = runner.Run(snapshot, new WireId128(15, 15), new WireId128(16, 16));
        byte[] expected = WireValue.Utf8("鉄の雨☂").Payload.ToArray();
        True(report.TopLevel.Value!.AsBytes().SequenceEqual(expected), "byte-string top-level result");
    }

    private static void PrepareDiagnostics()
    {
        using var engine = new IronVaakEngine();
        PreparationResult result = engine.Prepare("(", Array.Empty<HostBindingDefinition>());
        True(!result.Succeeded, "invalid source must not prepare");
        True(result.Diagnostics.Count > 0, "prepare diagnostics");
    }

    private static void RuntimeErrorKeepsPatch()
    {
        using var engine = new IronVaakEngine();
        using IronVaakProgram program = engine.PrepareOrThrow("n := 42; n := n / 0; 0", Layout());
        using IronVaakRunner runner = program.CreateRunner();
        ExecutionReport report = runner.Run(Snapshot(1), new WireId128(9, 9), new WireId128(10, 10));
        Equal(ProgramStatus.ProgramError, report.Status, "runtime status");
        Equal(42L, report.Patch.Entries[0].Value.AsI64(), "error writeback");
        True(report.Diagnostics.Count > 0, "runtime diagnostic");
    }

    private static void HandlesSurviveParentDispose()
    {
        var engine = new IronVaakEngine();
        IronVaakProgram program = engine.PrepareOrThrow("n += 1; n", Layout());
        IronVaakRunner runner = program.CreateRunner();
        program.Dispose();
        engine.Dispose();
        ExecutionReport report = Task.Run(() =>
            runner.Run(Snapshot(4), new WireId128(11, 11), new WireId128(12, 12))).GetAwaiter().GetResult();
        Equal(5L, report.TopLevel.Value!.AsI64(), "runner retained native context and prepared program");
        runner.Dispose();
    }

    private static void PublicRuntimeContract()
    {
        var coordinator = new ScriptPlanCoordinator(new IScriptPlanRuntime[] { new EmptyRuntime() });
        SettingsSnapshot snapshot = Snapshot(1);
        ScriptCompositionResult result = coordinator.ExecuteAsync(
            snapshot, new WireId128(17, 17), new WireId128(18, 18)).AsTask().GetAwaiter().GetResult();
        Equal(0, result.Patch.Entries.Count, "external runtime plan");
        Equal(1, result.Plans.Count, "external runtime result");
    }

    private static SettingsSnapshot SharedSnapshot()
    {
        return new SettingsSnapshot(
            new WireId128(21, 22),
            new WireId128(23, 24),
            25,
            new[]
            {
                new SnapshotEntry(0, 10, 26, WireValue.I64(4)),
                new SnapshotEntry(0, 11, 27, WireValue.I64(8)),
            });
    }

    private static void LuaComposition()
    {
        using var engine = new IronVaakEngine();
        using IronVaakProgram program = engine.PrepareOrThrow("n += 1; n", Layout());
        using var vaak = new IronVaakScriptAdapter("vaak", program.CreateRunner());
        var lua = new LuaScriptAdapter("lua", new FakeLuaExecutor(11, 3));
        var coordinator = new ScriptPlanCoordinator(new IScriptPlanRuntime[] { vaak, lua });
        SettingsSnapshot snapshot = SharedSnapshot();
        ScriptCompositionResult result = coordinator.ExecuteAsync(
            snapshot, new WireId128(28, 29), new WireId128(30, 31)).AsTask().GetAwaiter().GetResult();
        Equal(2, result.Patch.Entries.Count, "Vaak + Lua patch count");
        Equal(5L, result.Patch.Entries.Single(entry => entry.PropertyId == 10).Value.AsI64(), "Vaak plan");
        Equal(11L, result.Patch.Entries.Single(entry => entry.PropertyId == 11).Value.AsI64(), "Lua plan");
    }

    private static void LuaConflictIsRejected()
    {
        using var engine = new IronVaakEngine();
        using IronVaakProgram program = engine.PrepareOrThrow("n += 1; n", Layout());
        using var vaak = new IronVaakScriptAdapter("vaak", program.CreateRunner());
        var lua = new LuaScriptAdapter("lua", new FakeLuaExecutor(10, 2));
        var coordinator = new ScriptPlanCoordinator(new IScriptPlanRuntime[] { vaak, lua });
        bool rejected = false;
        try
        {
            coordinator.ExecuteAsync(
                SharedSnapshot(), new WireId128(32, 33), new WireId128(34, 35)).AsTask().GetAwaiter().GetResult();
        }
        catch (ScriptPatchConflictException)
        {
            rejected = true;
        }
        True(rejected, "Vaak/Lua write conflict");
    }

    private sealed class FakeLuaExecutor : ILuaPlanExecutor
    {
        private readonly uint _property;
        private readonly long _add;

        internal FakeLuaExecutor(uint property, long add)
        {
            _property = property;
            _add = add;
        }

        public ValueTask<ReadOnlyMemory<byte>> ExecutePlanAsync(ScriptInvocation invocation, CancellationToken cancellationToken)
        {
            cancellationToken.ThrowIfCancellationRequested();
            SettingsSnapshot copiedSnapshot = ExchangeCodec.DecodeSnapshot(invocation.SnapshotWire.Span);
            SnapshotEntry before = copiedSnapshot.Entries.Single(entry => entry.PropertyId == _property);
            var patch = new SettingsPatch(
                copiedSnapshot.SchemaId,
                copiedSnapshot.SessionId,
                invocation.RunId,
                invocation.TransactionId,
                copiedSnapshot.SnapshotRevision,
                new[]
                {
                    new PatchEntry(
                        before.EntityId,
                        before.PropertyId,
                        before.PropertyRevision,
                        0,
                        WireValue.I64(before.Value.AsI64() + _add)),
                });
            return new ValueTask<ReadOnlyMemory<byte>>(ExchangeCodec.EncodePatch(patch));
        }
    }

    private sealed class EmptyRuntime : IScriptPlanRuntime
    {
        public string RuntimeId => "external-runtime";

        public ValueTask<ScriptPlan> ExecuteAsync(ScriptInvocation invocation, CancellationToken cancellationToken)
        {
            cancellationToken.ThrowIfCancellationRequested();
            var patch = new SettingsPatch(
                invocation.Snapshot.SchemaId,
                invocation.Snapshot.SessionId,
                invocation.RunId,
                invocation.TransactionId,
                invocation.Snapshot.SnapshotRevision,
                Array.Empty<PatchEntry>());
            return new ValueTask<ScriptPlan>(new ScriptPlan(RuntimeId, true, patch));
        }
    }

    private static void True(bool value, string name)
    {
        if (!value) throw new InvalidOperationException($"Assertion failed: {name}");
    }

    private static void Equal<T>(T expected, T actual, string name)
    {
        if (!EqualityComparer<T>.Default.Equals(expected, actual))
            throw new InvalidOperationException($"Assertion failed: {name}; expected {expected}, actual {actual}");
    }
}
