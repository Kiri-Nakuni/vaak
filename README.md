# Vaak

Vaak（ヴァーク）は、ホストへ埋め込み、短い計算kernelを繰り返し実行するための小さな静的型付き言語です。
スタンドアロンでも動きますが、入出力、filesystem、TeX、Unity、Lua等のobjectは言語へ暗黙に持ち込まず、
ホストが公開した値・関数・Snapshot/Patch境界を通します。

このrepositoryは開発中です。言語意味論の正本は
[`docs/vaak/decisions.md`](docs/vaak/decisions.md)、木を辿る参照実装は
[`src/interp.rs`](src/interp.rs)です。実験文書や別backendが食い違う場合は、この二つを優先します。

## まず動かす

RustとCargoを用意し、repository rootで実行します。

```console
cargo build --release --locked
cargo run --release --locked -- examples/vaak/01-探索.vaak
```

結果は`4499`です。もう少し小さい例なら、`.vaak`ファイルの最上位へ値を一つ残します。

```vaak
let width := 6;
let height := 7;
width * height
```

構文、型、`paradox`、`;`、`??`、host APIは
[`docs/reference.md`](docs/reference.md)から読めます。試験されている例は
[`examples/vaak/README.md`](examples/vaak/README.md)にあります。

## 実行系

| 実行系 | 入口 | 位置づけ |
|---|---|---|
| 参照実装 | `vaak file.vaak` | 木を辿る意味の基準 |
| bytecode VM | Rust API / embedding API | prepare once / run manyの主な埋め込み経路 |
| STEEL | `steel file.vaak` | LLVM IRを生成する部分実装。native化には`clang`が必要 |
| Portable | `portable` | WASIまたは素のWASM C ABIからVMを使う入口 |
| LSP | `vaak-lsp` | 構文・名前・領域・型診断と編集支援 |

STEELは未対応機能を別の意味へ読み替えず、compile errorとして拒否します。backendの対応範囲と
コマンドは[`docs/reference.md`](docs/reference.md#9-ツールと実装範囲)を参照してください。

## Rustへ埋め込む

[`src/embedding.rs`](src/embedding.rs)は、次を一つの公開境界にまとめます。

- `HostLayout`: host値とhost functionの名前・順序・型を固定するdescriptor
- `PreparedProgram`: parse、check、type-check、VM compileを一度だけ行った結果
- `EmbeddingRunner`: runnerとscratchを再利用し、layout/value/function不一致を実行前に拒否

PraTeX型の埋め込み実験とSnapshot → command buffer → validate → commitの分担は
[`docs/experiments/embedding.md`](docs/experiments/embedding.md)にあります。hostの逐次作用は後続の
Vaak errorで巻き戻らないため、複数更新を原子的にしたい場合はowned Patchを返し、hostが全体検査後に
一回だけcommitします。

## pure Vaak標準ライブラリ

任意選択ライブラリはcore意味論を増やさず、既存Vaakだけで書かれています。module/importはまだ無いため、
hostが必要なsourceを利用者programの前へ依存順で連結してから一度だけprepareします。

このcheckpointには、文字列、配列・探索・整列、DSU、Fenwick、heap/deque、segment tree、sparse table、
ordered multiset、固定長dense bitset、CSR graph/SCC/BFS/topological sort/2-SAT、一括ASCII整数I/O、UTF-8 JSON/JSONL等の
試作があります。公開関数、境界条件、依存順は[`stdlib/README.md`](stdlib/README.md)、測定と棄却案は
[`stdlib/BENCHMARK.md`](stdlib/BENCHMARK.md)を参照してください。

source冒頭の明示依存から決定的な前置き順を得られます。tie-breakはlocaleでなくUTF-8 byte順です。

```console
python3 scripts/resolve-stdlib.py codec/jsonl_utf8.vaak
python3 scripts/resolve-stdlib.py --concat codec/jsonl_utf8.vaak > program-prefix.vaak
```

header契約とstable診断codeは[`stdlib/DEPENDENCIES.md`](stdlib/DEPENDENCIES.md)にあります。resolverは
Vaak本文から依存を推測せず、missing、cycle、重複、非canonical pathを成功順序にしません。

JSON/JSONLをRust hostから使う場合は、次の順で前置きします。codecはfileやsocketを開きません。

```rust
let source = format!(
    "{}\n{}\n{}\n{}",
    vaak::stdlib::STRING,
    vaak::stdlib::JSON_UTF8,
    vaak::stdlib::JSONL_UTF8,
    application,
);
```

TeX文字class、game snapshotの有限mask、競技programの集合には
`vaak::stdlib::DENSE_BITSET_U32`を単独で前置きできます。論理長と末尾paddingを値の契約として保持するため、
最速だった生の`u32 array`案ではなくnamed型を公開しています。比較値と棄却理由はbenchmark文書に残しています。

## IRON VAAK (.NET / Unity)

`codex3/iron-vaak-dotnet` branchでは、IRON VAAKのC ABI、`.NET Standard 2.1` / `.NET 8` facade、
Unity向けUPM source package、製品非依存Lua plan adapterを縦切りしています。同じimmutable Snapshotを
VaakとLuaへ渡し、両runtimeが完全にreturnした後にowned Patchを検査・合成する境界です。同期的な
Lua → Vaak → Lua再入は作りません。

現状はLinux x64で接続可能性を検証したadapter候補であり、production runtimeやLVMINIBVS採用済み機能では
ありません。hard fuel、完全なmemory accounting、typed hook codec、Unity Editor/Player・Mono/IL2CPP・
対象OS matrix、artifact hardeningが採用gateに残ります。IRON JIT VAAKも長期候補であり、VM fallbackや
同じ差分fixtureを迂回しません。

## self-host実験

`codex3/steel-selfhost` branchでは、Vaakで書いた算術subset compilerがLLVM IRを生成し、そのcompiler自身を
Rust STEELでnative化する最小縦切りまで通しています。Rust版STEEL全体の置換は未完成なので、完成扱いせず、
fallbackとしてpure Vaak numeric bytecode interpreterを実装しました。同じ13,000,010 opcodeではRust oracle
62.294 ms、Vaak/STEEL 87.657 ms（約1.41倍）でした。nested compiler呼出しはSIGSEGVするため、最小fixture、
signal 11、再現commandをignored棄却記録として保持しています。

## 検証

全体ゲートは次です。

```console
cargo test --release --locked --no-fail-fast -- --test-threads=1
```

`codex2/steel-short-circuit-fixes`の2026-08-26 checkpointでは844 passed、0 failed、0 ignoredです。
旧checkpointのgraph 3件、heap、ASCII I/O、stringのnative失敗は、STEELが`&&` / `||`の右辺を常に
評価していたことが主因でした。集合体返値がparadoxのときnull仮値を深い複製していたSIGSEGVも防ぎ、
負長bitset constructorとalias-growのignored fixtureを通常gateへ戻しています。string fixtureだけは
非重複全置換後の正しい値`A--あ`に対して試験が`A-Aあ`を期待していたため、期待値を修正しました。
修正前の失敗値と性能上の棄却理由は実験文書に残しています。

## 文書の地図

- [実用リファレンス](docs/reference.md)
- [動く例](examples/vaak/README.md)
- [形式構文](docs/vaak/16-形式構文.md)
- [設計判断の正本](docs/vaak/decisions.md)
- [理由を書かない反論用probe](docs/vaak/probe.md)
- [pure Vaak標準ライブラリ](stdlib/README.md)
- [LSP・Zed・VS Code](editors/README.md)
- [ライセンス境界](docs/LICENSING.md)

Vaak本体は[MIT License](LICENSE)です。GPLのPraTeX/rtexからVaakへ運ぶのは要求、測定値、設計判断だけで、
sourceやtestを転記しません。
