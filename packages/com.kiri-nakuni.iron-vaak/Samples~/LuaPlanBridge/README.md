# Lua plan bridge sample shape

Bind an installed Lua product without adding it as an IRON VAAK dependency:

```csharp
var lua = new LuaScriptAdapter(
    "gameplay-lua",
    new DelegateLuaPlanExecutor(async (invocation, cancellation) =>
    {
        // Copy invocation.SnapshotWire into the Lua runtime's protected entry.
        // Lua returns canonical SettingsPatchV0 bytes; do not expose C# or Unity callbacks.
        return await myLuaRuntime.RunPlanAsync(invocation.SnapshotWire, cancellation);
    }));

var coordinator = new ScriptPlanCoordinator(new IScriptPlanRuntime[] { vaak, lua });
ScriptCompositionResult result = await coordinator.ExecuteAsync(snapshot, runId, transactionId);
```

The adapter rejects nested/concurrent entry, wrong snapshot/session/run identity, malformed bytes,
and non-canonical records before the Patch reaches Unity validation.
