using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;

namespace IronVaak.Scripting
{
    public sealed class ScriptInvocation
    {
        private readonly byte[] _snapshotWire;

        internal ScriptInvocation(SettingsSnapshot snapshot, byte[] snapshotWire, WireId128 runId, WireId128 transactionId)
        {
            Snapshot = snapshot;
            _snapshotWire = snapshotWire;
            RunId = runId;
            TransactionId = transactionId;
        }

        public SettingsSnapshot Snapshot { get; }
        public ReadOnlyMemory<byte> SnapshotWire => _snapshotWire;
        public WireId128 RunId { get; }
        public WireId128 TransactionId { get; }
    }

    public sealed class ScriptPlan
    {
        private readonly byte[] _patchWire;

        public ScriptPlan(string runtimeId, bool succeeded, SettingsPatch patch)
            : this(runtimeId, succeeded, patch, ExchangeCodec.EncodePatch(patch), null)
        {
        }

        internal ScriptPlan(string runtimeId, bool succeeded, SettingsPatch patch, ReadOnlyMemory<byte> patchWire, ExecutionReport? vaakReport)
        {
            RuntimeId = string.IsNullOrWhiteSpace(runtimeId)
                ? throw new ArgumentException("Runtime ID is required.", nameof(runtimeId))
                : runtimeId;
            Succeeded = succeeded;
            Patch = patch ?? throw new ArgumentNullException(nameof(patch));
            _patchWire = patchWire.ToArray();
            VaakReport = vaakReport;
        }

        public string RuntimeId { get; }
        public bool Succeeded { get; }
        public SettingsPatch Patch { get; }
        public ReadOnlyMemory<byte> PatchWire => _patchWire;
        public ExecutionReport? VaakReport { get; }
    }

    public interface IScriptPlanRuntime
    {
        string RuntimeId { get; }
        ValueTask<ScriptPlan> ExecuteAsync(ScriptInvocation invocation, CancellationToken cancellationToken);
    }

    /// <summary>Vaak runnerをgeneric script-plan contractへ接続する。callbackは公開しない。</summary>
    public sealed class IronVaakScriptAdapter : IScriptPlanRuntime, IDisposable
    {
        private readonly IronVaakRunner _runner;
        private int _running;
        private bool _disposed;

        public IronVaakScriptAdapter(string runtimeId, IronVaakRunner runner)
        {
            RuntimeId = string.IsNullOrWhiteSpace(runtimeId) ? throw new ArgumentException("Runtime ID is required.", nameof(runtimeId)) : runtimeId;
            _runner = runner ?? throw new ArgumentNullException(nameof(runner));
        }

        public string RuntimeId { get; }

        public ValueTask<ScriptPlan> ExecuteAsync(ScriptInvocation invocation, CancellationToken cancellationToken)
        {
            if (invocation == null) throw new ArgumentNullException(nameof(invocation));
            if (_disposed) throw new ObjectDisposedException(nameof(IronVaakScriptAdapter));
            cancellationToken.ThrowIfCancellationRequested();
            Enter();
            try
            {
                ExecutionReport report = _runner.Run(invocation.Snapshot, invocation.RunId, invocation.TransactionId);
                cancellationToken.ThrowIfCancellationRequested();
                var plan = new ScriptPlan(
                    RuntimeId,
                    report.Status == Interop.ProgramStatus.Completed,
                    report.Patch,
                    report.PatchWire,
                    report);
                return new ValueTask<ScriptPlan>(plan);
            }
            finally
            {
                Volatile.Write(ref _running, 0);
            }
        }

        private void Enter()
        {
            if (Interlocked.CompareExchange(ref _running, 1, 0) != 0)
                throw new ScriptReentrancyException(RuntimeId);
        }

        public void Dispose()
        {
            if (_disposed) return;
            if (Volatile.Read(ref _running) != 0) throw new InvalidOperationException("Cannot dispose a running script adapter.");
            _disposed = true;
            _runner.Dispose();
        }
    }

    /// <summary>
    /// Product-neutral direct managed-Lua boundary. Implementations run a protected Lua entry and return one Patch wire.
    /// They must not call Vaak or Unity synchronously from Lua callbacks.
    /// </summary>
    public interface ILuaPlanExecutor
    {
        ValueTask<ReadOnlyMemory<byte>> ExecutePlanAsync(ScriptInvocation invocation, CancellationToken cancellationToken);
    }

    /// <summary>Small binding point for MoonSharp, xLua, NLua, or an application-owned Lua service.</summary>
    public sealed class DelegateLuaPlanExecutor : ILuaPlanExecutor
    {
        private readonly Func<ScriptInvocation, CancellationToken, ValueTask<ReadOnlyMemory<byte>>> _execute;

        public DelegateLuaPlanExecutor(
            Func<ScriptInvocation, CancellationToken, ValueTask<ReadOnlyMemory<byte>>> execute)
        {
            _execute = execute ?? throw new ArgumentNullException(nameof(execute));
        }

        public ValueTask<ReadOnlyMemory<byte>> ExecutePlanAsync(
            ScriptInvocation invocation,
            CancellationToken cancellationToken) => _execute(invocation, cancellationToken);
    }

    public sealed class LuaScriptAdapter : IScriptPlanRuntime
    {
        private readonly ILuaPlanExecutor _executor;
        private int _running;

        public LuaScriptAdapter(string runtimeId, ILuaPlanExecutor executor)
        {
            RuntimeId = string.IsNullOrWhiteSpace(runtimeId) ? throw new ArgumentException("Runtime ID is required.", nameof(runtimeId)) : runtimeId;
            _executor = executor ?? throw new ArgumentNullException(nameof(executor));
        }

        public string RuntimeId { get; }

        public async ValueTask<ScriptPlan> ExecuteAsync(ScriptInvocation invocation, CancellationToken cancellationToken)
        {
            if (invocation == null) throw new ArgumentNullException(nameof(invocation));
            if (Interlocked.CompareExchange(ref _running, 1, 0) != 0)
                throw new ScriptReentrancyException(RuntimeId);
            try
            {
                cancellationToken.ThrowIfCancellationRequested();
                ReadOnlyMemory<byte> returned = await _executor.ExecutePlanAsync(invocation, cancellationToken).ConfigureAwait(false);
                byte[] ownedWire = returned.ToArray();
                SettingsPatch patch = ExchangeCodec.DecodePatch(ownedWire);
                ValidateIdentity(invocation, patch);
                cancellationToken.ThrowIfCancellationRequested();
                return new ScriptPlan(RuntimeId, true, patch, ownedWire, null);
            }
            finally
            {
                Volatile.Write(ref _running, 0);
            }
        }

        private static void ValidateIdentity(ScriptInvocation invocation, SettingsPatch patch)
        {
            SettingsSnapshot snapshot = invocation.Snapshot;
            if (patch.SchemaId != snapshot.SchemaId || patch.SessionId != snapshot.SessionId ||
                patch.RunId != invocation.RunId || patch.TransactionId != invocation.TransactionId ||
                patch.BaseSnapshotRevision != snapshot.SnapshotRevision)
                throw new ScriptPlanException("Lua returned a Patch for a different snapshot or invocation.");
        }
    }

    public sealed class ScriptCompositionResult
    {
        internal ScriptCompositionResult(SettingsPatch patch, IReadOnlyList<ScriptPlan> plans)
        {
            Patch = patch;
            Plans = plans;
        }

        public SettingsPatch Patch { get; }
        public IReadOnlyList<ScriptPlan> Plans { get; }
    }

    /// <summary>Runs independent runtimes from one immutable snapshot, then merges with strict write-conflict rejection.</summary>
    public sealed class ScriptPlanCoordinator
    {
        private readonly IReadOnlyList<IScriptPlanRuntime> _runtimes;
        private int _running;

        public ScriptPlanCoordinator(IEnumerable<IScriptPlanRuntime> runtimes)
        {
            if (runtimes == null) throw new ArgumentNullException(nameof(runtimes));
            List<IScriptPlanRuntime> copy = runtimes.ToList();
            if (copy.Count == 0) throw new ArgumentException("At least one runtime is required.", nameof(runtimes));
            if (copy.Any(runtime => runtime == null)) throw new ArgumentException("A runtime is null.", nameof(runtimes));
            if (copy.Any(runtime => string.IsNullOrWhiteSpace(runtime.RuntimeId)))
                throw new ArgumentException("Every runtime needs a non-empty ID.", nameof(runtimes));
            if (copy.Select(runtime => runtime.RuntimeId).Distinct(StringComparer.Ordinal).Count() != copy.Count)
                throw new ArgumentException("Runtime IDs must be unique.", nameof(runtimes));
            _runtimes = copy.AsReadOnly();
        }

        public async ValueTask<ScriptCompositionResult> ExecuteAsync(
            SettingsSnapshot snapshot,
            WireId128 runId,
            WireId128 transactionId,
            CancellationToken cancellationToken = default)
        {
            if (snapshot == null) throw new ArgumentNullException(nameof(snapshot));
            if (Interlocked.CompareExchange(ref _running, 1, 0) != 0)
                throw new ScriptReentrancyException("coordinator");
            try
            {
                cancellationToken.ThrowIfCancellationRequested();
                byte[] snapshotWire = ExchangeCodec.EncodeSnapshot(snapshot);
                var invocation = new ScriptInvocation(snapshot, snapshotWire, runId, transactionId);
                var tasks = new Task<ScriptPlan>[_runtimes.Count];
                for (int index = 0; index < _runtimes.Count; index++)
                {
                    try
                    {
                        tasks[index] = _runtimes[index].ExecuteAsync(invocation, cancellationToken).AsTask();
                    }
                    catch (Exception error)
                    {
                        tasks[index] = Task.FromException<ScriptPlan>(error);
                    }
                }
                ScriptPlan[] plans = await Task.WhenAll(tasks).ConfigureAwait(false);
                cancellationToken.ThrowIfCancellationRequested();
                for (int index = 0; index < plans.Length; index++)
                {
                    ScriptPlan plan = plans[index] ??
                        throw new ScriptPlanException($"Runtime '{_runtimes[index].RuntimeId}' returned a null plan.");
                    if (!StringComparer.Ordinal.Equals(plan.RuntimeId, _runtimes[index].RuntimeId))
                        throw new ScriptPlanException($"Runtime '{_runtimes[index].RuntimeId}' returned a plan owned by '{plan.RuntimeId}'.");
                    if (!plan.Succeeded) throw new ScriptPlanRejectedException(plan.RuntimeId);
                }
                SettingsPatch merged = MergeStrict(snapshot, runId, transactionId, plans);
                return new ScriptCompositionResult(merged, Array.AsReadOnly(plans));
            }
            finally
            {
                Volatile.Write(ref _running, 0);
            }
        }

        private static SettingsPatch MergeStrict(
            SettingsSnapshot snapshot,
            WireId128 runId,
            WireId128 transactionId,
            IReadOnlyList<ScriptPlan> plans)
        {
            var entries = new List<PatchEntry>();
            var owners = new Dictionary<(ulong, uint), string>();
            foreach (ScriptPlan plan in plans)
            {
                SettingsPatch patch = plan.Patch;
                if (patch.SchemaId != snapshot.SchemaId || patch.SessionId != snapshot.SessionId ||
                    patch.BaseSnapshotRevision != snapshot.SnapshotRevision || patch.RunId != runId || patch.TransactionId != transactionId)
                    throw new ScriptPlanException($"Runtime '{plan.RuntimeId}' returned a Patch with the wrong identity.");
                foreach (PatchEntry entry in patch.Entries)
                {
                    var key = (entry.EntityId, entry.PropertyId);
                    if (owners.TryGetValue(key, out string? first))
                        throw new ScriptPatchConflictException(key.Item1, key.Item2, first, plan.RuntimeId);
                    owners.Add(key, plan.RuntimeId);
                    entries.Add(entry);
                }
            }
            return new SettingsPatch(snapshot.SchemaId, snapshot.SessionId, runId, transactionId, snapshot.SnapshotRevision, entries);
        }
    }

    public class ScriptPlanException : IronVaakException
    {
        public ScriptPlanException(string message) : base(message) { }
    }

    public sealed class ScriptReentrancyException : ScriptPlanException
    {
        public ScriptReentrancyException(string runtimeId) : base($"Runtime '{runtimeId}' rejected concurrent or nested entry.") { }
    }

    public sealed class ScriptPlanRejectedException : ScriptPlanException
    {
        public ScriptPlanRejectedException(string runtimeId) : base($"Runtime '{runtimeId}' did not complete successfully; its plan was not composed.") { }
    }

    public sealed class ScriptPatchConflictException : ScriptPlanException
    {
        public ScriptPatchConflictException(ulong entityId, uint propertyId, string firstRuntime, string secondRuntime)
            : base($"Runtimes '{firstRuntime}' and '{secondRuntime}' both wrote property ({entityId}, {propertyId}).")
        {
            EntityId = entityId;
            PropertyId = propertyId;
            FirstRuntime = firstRuntime;
            SecondRuntime = secondRuntime;
        }

        public ulong EntityId { get; }
        public uint PropertyId { get; }
        public string FirstRuntime { get; }
        public string SecondRuntime { get; }
    }
}
