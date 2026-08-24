# IRON VAAK for .NET

IRON VAAK exposes Vaak's prepared VM through a fixed C ABI and a `.NET Standard 2.1` facade.
The same C# sources are the runtime of the UPM package in
`packages/com.kiri-nakuni.iron-vaak`.

```csharp
using var engine = new IronVaakEngine();
using var program = engine.PrepareOrThrow(
    "score += 10; score",
    new[] { new HostBindingDefinition("score", WireValueType.I64, 1, 20) });
using var runner = program.CreateRunner();

ExecutionReport report = runner.Run(snapshot, runId, transactionId);
// Validate report.Patch against current revisions/capabilities, then commit it.
```

The core does not depend on a Lua implementation. `LuaScriptAdapter` directly connects a managed
Lua product through `ILuaPlanExecutor`, using the same immutable Snapshot and owned Patch bytes.
Vaak and Lua never synchronously call one another.

Build and smoke-test on Linux:

```bash
scripts/stage-iron-vaak-native.sh x86_64-unknown-linux-gnu
dotnet build dotnet/src/IronVaak/IronVaak.csproj -c Release
LD_LIBRARY_PATH=crates/iron-vaak-native/target/x86_64-unknown-linux-gnu/release \
  dotnet run --project dotnet/tests/IronVaak.Smoke/IronVaak.Smoke.csproj -c Release
```
