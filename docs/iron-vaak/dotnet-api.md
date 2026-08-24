# IRON VAAK .NET / Unity implementation checkpoint

更新日: 2026-08-24

この文書は非規範的な実装記録である。Vaakの意味論、参照実装、C-n/S-nを変更しない。

## 実装した縦切り

- `iron-vaak-native`: raw pointerを扱う唯一のC ABI shim。全exportでpanicを捕捉する。
- `IronVaak` (`netstandard2.1;net8.0`): `SafeHandle`、prepare once、runner reuse、診断とreport copy。
- UPM source package: 同じC# sourceをUnityが直接buildする。`UnityEngine`依存は無い。
- managed Snapshot/Patch codec: little-endian、16 MiB/65535 record上限、canonical順、UTF-8、finite floatを検査する。
- `ScriptPlanCoordinator`: Vaak/Lua等を同一immutable snapshotから独立実行し、Patchをstrict mergeする。
- `LuaScriptAdapter`: 特定Lua製品に依存せず、`ILuaPlanExecutor`のowned wireだけを受ける。
- `UnityPlanScheduler`: 計算終了後、validation/commitだけをmain-thread contextへpostする。

root `vaak`のportable C exportはdefault featureで従来どおり維持する一方、IRON依存時だけ
`default-features = false`にしている。したがってIRON native libraryの動的公開面は
`iron_vaak_v0_*` 16 symbolだけであり、portable用`vaak_*`を偶然再公開しない。

Vaakの既存`str`値は任意byte列なので、untypedなtop-level `str`は`WireValueType.Bytes`として返す。
HostLayoutで`Utf8`と宣言したpropertyは、Snapshot/Patch境界でstrict UTF-8として検査する。

Linux x64では通常のCoreCLR smokeに加え、`net8.0` Native AOT publishした実行ファイルでも同じ11 fixtureを通した。
これはUnity IL2CPPの代替検証ではないが、reflection/dynamic code generationへ依存していないことの独立gateである。

## lifetime / thread

Context registry、prepared table、runner tableはthread-safeである。prepared programは共有でき、異なるrunnerは
並行実行できる。同じrunnerへの重複entryは`BUSY`である。C# facadeはrunnerごとにlockする。

Contextを先にDisposeしても、child `SafeHandle`がdangerous referenceを保持する。runnerはnative側でもpreparedへ
strong referenceを持つため、programを先にDisposeした後も走れる。この性質はdispose順の事故を防ぐためであり、
長寿命objectの明示Disposeを不要にする意図ではない。

## shared snapshot

HostLayout外のSnapshot propertyはVaakから不可視のまま許す。これによりLua、AI、rules engine等へ同じimmutable
snapshotを渡せる。HostLayoutに必要なpropertyの欠落・型違いは従来どおりrun前に拒否する。Vaakが返すPatchには
HostLayout内で実際に変わったpropertyだけが入る。

## v0 Lua policy

- Vaak→Lua、Lua→Vaak、Lua→Unityの同期callbackは0件。
- Lua table/userdata/function/threadはwireへ入れない。schema付きcopyへ変換する。
- Lua adapterが返したPatchはschema/session/run/transaction/base revisionを検査する。
- 複数runtimeが同じpropertyを書くと、値が同じでもconflictとして全体をrejectする。
- Lua coroutine/yield、PUC-Lua C module、特定managed Lua製品の公式supportはこのcheckpointに含めない。

## 未完了のrelease gate

- Unity Editor/Player、Mono/IL2CPPの実機matrix。
- Windows/macOS/Android/iOS native artifact、Unity `.meta` import設定、署名・hash・再現build。
- PUC-Lua 5.4 C shimと特定managed Lua製品adapterの製品別license/AOT監査。
- fuzz/sanitizer、context/diagnostic handle exhaustion、untrusted script向けfuel/cancellation。
- aggregate host value、Command batch、capability grant、完全なExecutionReport wire。

## Vaak全体回帰の既存失敗

`cargo test --release --locked --no-fail-fast`では、STEEL文字列library native試験一件だけが
`Some(0)`対`Some(42)`で失敗した。同じ失敗を変更前の`49b536c`でも再現したため、本差分の回帰ではない。
環境はUbuntu clang 18.1.3。この枝ではSTEEL・参照実装・意味論を変更していない。
