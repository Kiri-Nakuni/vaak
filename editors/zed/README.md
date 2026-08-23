# Zed 拡張

**Zed は LSP の意味トークンで色を付けない。** だから tree-sitter 文法を置いた。

ただし**色分けに要る分だけ**である——式の優先順位も、領域の規則も、段の数え方も
入っていない。**入れれば一つの決定を二箇所で実装することになる。**

**鍵語の一覧だけは `src/lexer.rs` と一致していなければならない。**
`cargo test --test grammar` が突き合わせているので、片方に足して
もう片方に足し忘れたら落ちる。

意味は言語サーバが持つ。診断・説明・記号一覧・補完はそちらから来る。

## 文法を直したら

```bash
node scripts/sync-grammar.mjs
```

Unix では `scripts/sync-grammar.sh` も同じ処理を呼ぶ。

文法ソースと生成済み `parser.c` は Vaak と同じ版方の
`editors/tree-sitter-vaak` に収めている。通常の clone だけで試験できる。

**Zed は文法を git のコミットから取る。** 同じコミット自身の SHA はその
コミットには書けないので、文法の変更は二つのコミットに分ける。

```bash
# 1. 文法と生成物を収める
git commit

# 2. そのコミットを Zed に pin して収める
node scripts/sync-grammar.mjs --pin
git commit editors/zed/extension.toml
```

`--pin` は指定するコミットに `src/parser.c` があることを確認してから
`repository`・`rev`・`path` を書く。

## 入れ方

```bash
cargo install --path .
```

**`cargo build --release` では足りないことがある。**
Zed の拡張は **WASI の砂場の中で動く**ので、ホストの任意の道を覗けない——
`PATH` から見つかるところに置くのが確実である。

そのうえで Zed の `zed: install dev extension` でこの `editors/zed` を指す。

言語サーバは次の順に探す:

1. **設定で明示された道**

   ```json
   "lsp": { "vaak-lsp": { "binary": { "path": "/…/vaak-lsp" } } }
   ```
2. `PATH` の `vaak-lsp`
3. 開いているワークツリーの `target/release/vaak-lsp` / `target/debug/vaak-lsp`

## 出るもの

| | |
|---|---|
| **診断** | 構文・名前・領域・型。保存を待たずに出る |
| **色分け** | 意味トークン。組み込みの型は型の色、`$名前` は macro の色 |
| **記号一覧** | `fn` `struct` `wrap` `flow` と最上位の宣言 |
| **説明** | 鍵語には**なぜそう書くのか**を出す（`??` `;` `break` `continue` `flow` …） |
| **補完** | 鍵語と、その文書で宣言されている名前 |
