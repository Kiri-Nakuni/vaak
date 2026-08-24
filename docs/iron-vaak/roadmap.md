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
