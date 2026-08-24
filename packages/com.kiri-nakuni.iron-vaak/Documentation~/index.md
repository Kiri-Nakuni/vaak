# IRON VAAK Unity integration

## Runtime shape

`IronVaakEngine` owns the native handle domain. `IronVaakProgram` is immutable after preparation;
`IronVaakRunner` is reusable and serializes its own calls. Programs/runners retain their parent
context with `SafeHandle`, so explicit disposal order is forgiving, but deterministic disposal is
still recommended.

The runtime reads and writes only versioned Snapshot/Patch bytes. Never put a Unity object address,
`GCHandle`, fake-null, Lua state, registry reference, or stack index in the exchange schema. Use
stable entity/property IDs and re-resolve them at commit time.

## Lua

Implement `ILuaPlanExecutor` (or use `DelegateLuaPlanExecutor`) for the installed managed Lua
product. The Lua entry receives `ScriptInvocation.SnapshotWire` and must return one canonical Patch.
`LuaScriptAdapter` copies and validates that Patch before composition.

`ScriptPlanCoordinator` starts Vaak and Lua from the same immutable snapshot. Its v0 merge policy is
strict: if two runtimes write the same `(entityId, propertyId)`, nothing is applied and
`ScriptPatchConflictException` is raised.

## IL2CPP

The facade uses explicit `DllImport`, fixed-width blittable records, no reflection, no dynamic code
generation, and no reverse managed delegate. Editor Mono smoke is not a release gate: build and run
an IL2CPP Player for every target architecture, including an actual iOS/Android device build.
