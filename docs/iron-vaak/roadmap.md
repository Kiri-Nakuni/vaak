# IRON VAAK / Unity native連携 roadmap

更新日: 2026-08-24

## 位置づけ

**IRON VAAK**はVaakの.NET版の正式名である。このroadmapはVaak言語の意味論を変更する計画ではなく、
既存の`PreparedProgram` / `EmbeddingRunner`をC ABIとmanaged facadeから安全に使うための外側の計画である。
Unity固有のobject lifecycle、main thread、scene、component、LuaのstackやGCはVaakへ持ち込まない。

WASMはsandboxed kernelやWeb向けの別経路として残すが、Unity/.NETの主経路にはしない。

## 段階

| 段階 | 成果物 | gate |
|---|---|---|
| 0. 設計監査 | `native-ffi-v0.md`、`exchange-schema-v0.md`、`script-runtime-adapter-v0.md` | 既存C-n/S-nとの衝突が無く、未決事項を決定扱いしていない |
| 1. C header fixture | export名、固定幅record、size/alignment、version negotiationだけのheaderとC smoke | Rust/.NET/IL2CPPの全表現が一致する。Vaak実行はまだ繋がない |
| 2. native handle | `PreparedProgram` / `EmbeddingRunner`のopaque handleと`prepare once / run many` | use-after-free、二重解放、同時実行、panicがmanaged側へ漏れない |
| 3. exchange codec | Snapshot/Patch/Command/Diagnosticのbounded codec | malformed/overlap/overflow/unknown-required sectionを全て拒否する |
| 4. IRON VAAK facade | .NET Standard 2.1 facadeと一般.NET用NuGet | `SafeHandle`、copy-only buffer、Mono/.NET runtime smoke |
| 5. Unity package | UPM package、platform別native plugin、main-thread commit adapter | Editor/Player、Mono/IL2CPP、Windows/macOS/Linux/Android/iOS実機matrix |
| 6. generic script adapters | fake adapter、Lua C API adapter、managed Lua adapterのoptional package | C#↔Vaak↔Luaの相互再入0、Lua無しのcore build成功 |
| 7. hardening | fuzz、sanitizer、native symbol、package reproducibility、resource limits | ABI compatibility fixtureと配布物hashをrelease gateへ入れる |

### 2026-08-24 checkpoint

`crates/iron-vaak-ffi`で段階1--3のうち、safe Rustだけで閉じる共通中核を先行実装した。
固定幅record、世代付きhandle、prepare once/run many、再入`BUSY`、panic poison、scalar/UTF8/BYTESの
Snapshot/Patch bulk codecまでであり、C export header/symbol、Command、完全なDiagnostic/ExecutionReport、
.NET/Unity packageは未着手である。このslice単体を段階1--3の完了とは数えない。

### 2026-08-24 codex3 checkpoint

`codex3/iron-vaak-dotnet`で上のsafe coreを実際の外部経路へ接続した。

- raw pointerだけを隔離した`iron-vaak-native` C export shim
- thread-safe context/prepared/runner handleと、同一runnerの`BUSY`
- `.NET Standard 2.1` / `net8.0`の`SafeHandle` facade
- UPM source package、managed Snapshot/Patch codec、main-thread apply scheduler
- 特定製品非依存の`ILuaPlanExecutor` / `LuaScriptAdapter`
- VaakとLuaを同一immutable snapshotから独立実行するstrict plan composition

段階1--4のLinux x64縦切りはnative経由のmanaged smokeまで通った。段階5はUPM source layoutまでであり、
Unity Editor/PlayerとIL2CPP実機matrixは未実施。段階6はmanaged Lua contractとfake adapterまでで、
PUC-Lua C adapterおよび公式support対象製品は未選定である。

## 長期候補: IRON JIT VAAK

**IRON JIT VAAK**を、IRON VAAKの同じprepared jobをより速く実行するoptional backendとして長期候補に置く。
これは新しいVaak方言ではなく、参照実装・bytecode VM・STEELと同じ結果を返す実装である。既存の固定C ABI、
managed `SafeHandle` facade、Snapshot/Patchのcopy-only境界は変えず、backend選択をhost側のfeature negotiationにする。

```text
Vaak source + HostLayout
  -> prepare/check/type-check once
  -> VM bytecode (全targetのfallback)
  -> optional JIT artifact (許可されたtargetだけ)
  -> 同じ Snapshot/Patch/Diagnostic ABI
```

Unity PlayerのIL2CPP、iOS、console等はruntime code generationや実行可能memoryの制約が異なるため、JITを
必須経路にしない。一般.NET desktopとJITを許すUnity Editorを最初のprototype候補にし、Playerはbytecode VMまたは
将来のAOT artifactへ確実にfallbackする。managed `Reflection.Emit` / `DynamicMethod`だけに依存する設計も採らない。

長期gate:

1. 同じprepared programとhost layoutをVM/JITで共有し、JIT有無をscriptから観測できる意味にしない。
2. 参照実装・VM・STEELとの差分fixtureをJITにも全件通す。未対応命令は黙って意味を変えずVMへfallbackする。
3. warm workloadでprepare費、compile費、steady-state、code size、allocationを別々に測り、JITの閾値を決める。
4. target triple、CPU feature、Vaak/ABI version、host layout hashをartifact identityへ含める。
5. W^X、unwind、panic/exception隔離、code cache上限、破損artifact拒否をnative release gateに入れる。
6. generated codeへUnity object、managed pointer、`lua_State`、callbackを埋め込まない。host accessは既存の
   index付きlayoutとowned value境界だけを通す。
7. JITをfuel/cancellation/security sandboxの代用にしない。それらは別の公開契約として扱う。

最初の調査sliceはbackendを実装せず、PraTeX/Unity型workloadのVM profile、hot命令、compile amortization、
x64/arm64 desktopで使えるJIT基盤のlicense/AOT共存性を比較する。その測定でsteady-stateの利益がcompile費と
配布・保守費を上回る範囲を確認してからprototypeへ進む。

## 実装開始前の停止線

- 同期`HostFn`をUnity/Luaのcallbackへ直結しない。v0 safe profileはsnapshot→planだけである。
- in-flightのVM実行を止めるfuel/cancellationは現APIに無い。意味論/API判断なしに擬似実装しない。
- runtime error後のVaak内after-stateと、UnityへPatchを原子的にapplyするhost transactionを混同しない。
- `EmbeddingRunner`のthread migration / `Send`保証は、公開契約にする前に実装型と全targetで監査する。
- patch conflict、command ordering、top-level値の利用方法はhost policyである。共通既定を採るなら別途合意する。
- Lua、managed Lua製品、Unity packageはoptional adapterであり、`vaak` crateやIRON VAAK coreの依存にしない。

## 最初の互換性target

| host | backend / architecture | 優先 |
|---|---|---:|
| 一般.NET | .NET 8、Windows/Linux/macOS x64/arm64 | P0 |
| Unity Editor | Mono、Windows x64 / macOS arm64+x64 | P0 |
| Unity Player | IL2CPP、Windows x64 / macOS arm64 / Linux x64 | P0 |
| Android | IL2CPP、arm64-v8a、x86_64 emulator、16 KiB page-size確認 | P0 |
| iOS | IL2CPP、device arm64、simulator arm64+x64、static link `__Internal` | P0 |
| Android 32-bit | armeabi-v7a | P2。需要とRust/Unity supportを確認後 |
| Web | WebAssembly | このroadmapの主経路外 |
