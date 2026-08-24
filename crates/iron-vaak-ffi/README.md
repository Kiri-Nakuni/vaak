# iron-vaak-ffi

IRON VAAKのUnity/.NET native境界について、raw pointerをsafe sliceへ変換した後の中核を固定する
独立crateである。Vaakの言語意味論は実装せず、`vaak::embedding::prepare`、`PreparedProgram`、
`EmbeddingRunner`だけを利用する。

## このcheckpointで接続したもの

- `rlib` / `cdylib` / `staticlib`を作れる独立package
- `#![forbid(unsafe_code)]`で検査するsafe Rust中核
- 世代とkindを検査する64-bit opaque prepared/runner handle
- zero、解放済みhandleに対するidempotent destroy
- UTF-8 sourceと固定幅`HostLayoutEntryV0`によるprepare once
- 同じpreparedを保持するrunnerとrun many
- 同一runnerの再入・重複mutable accessに対する`BUSY`
- 最外周とrunner実行内の`catch_unwind`、`INTERNAL_PANIC`、runner poison
- P/Invoke/IL2CPPで同じ幅にできる`repr(C)` record、C11/C++17 header、`sizeof`/`offsetof` fixture
- little-endianの一括`SettingsSnapshotV0` decodeと`SettingsPatchV0` encode
- runtime error時にも、既存C-2/S-22どおりafter-stateをPatchとしてreportへ保持する経路

## 意図的にまだ接続しないもの

- raw C pointerを受けるexport shim。safe sliceへ変換する最小箇所には監査済みの小さな`unsafe`
  または別のinterop実装が必要なので、このsafe-only checkpointではABI symbolを公開しない
- `HostFn`、nativeからmanaged/Luaへのcallback、suspend/resume、fuel/cancellation
- Unity object pointer、managed pointer、Lua state/stack index/value
- PatchのUnity main-thread apply。runtime error時にapplyするかdiscardするかもhost policyの未決事項
- `PreparedProgram`/runnerのcross-thread公開保証
- aggregate value、GameCommand、capability grant、完全なExecutionReport wire

このcrateが生成するPatchは未適用のtransaction候補であり、Unity状態を直接変更しない。
`HostLayoutEntryV0`のcapability table indexはhost-owned indexのcopyにすぎず、hostはapply時に
grantのscope/generation/resource limitを再検査しなければならない。

## focused checks

```powershell
cargo fmt --manifest-path crates/iron-vaak-ffi/Cargo.toml -- --check
cargo test --release --locked --manifest-path crates/iron-vaak-ffi/Cargo.toml
gcc -std=c11 -fsyntax-only crates/iron-vaak-ffi/tests/header_smoke.c
gcc -x c++ -std=c++17 -fsyntax-only crates/iron-vaak-ffi/tests/header_smoke.c
```
