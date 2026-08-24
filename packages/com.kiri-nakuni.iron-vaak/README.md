# IRON VAAK

UPM source package for Vaak calculation jobs in Unity games. The runtime assembly has no
`UnityEngine` dependency and is shared with the general .NET/NuGet facade.

1. Stage the platform native plugin with `scripts/stage-iron-vaak-native.sh`.
2. Prepare a program once and keep an `IronVaakRunner` for repeated jobs.
3. Build an immutable `SettingsSnapshot` on a safe host checkpoint.
4. Run Vaak and optional Lua adapters to produce plans.
5. Validate every revision/capability in shadow state, then commit on Unity's main thread.

`UnityPlanScheduler` posts validation/commit to the captured main-thread `SynchronizationContext`.
It never invokes Vaak or Lua while applying a plan.
