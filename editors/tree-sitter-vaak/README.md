# tree-sitter-vaak

Vaak のエディタ色分けに使う tree-sitter 文法である。

これは処理系の構文解析器ではない。式の優先順位、領域、脱出段などをここへ
再実装せず、`src/lexer.rs` に現れる字句を分類するところまでに留める。
鍵語と組み込み型の一覧は `tests/grammar.rs` が Rust 実装と照合する。

## 生成と試験

版方の根から次を実行する。

```bash
node scripts/sync-grammar.mjs
cargo test --release --test grammar
```

Unix では同じ処理を `scripts/sync-grammar.sh` でも呼べる。

同期スクリプトは tree-sitter CLI 0.25.10 で `src/parser.c` を生成し、corpus
試験を実行する。生成物も版方に収めるため、利用者が Zed 拡張を組み立てる
だけなら Node.js や tree-sitter CLI は要らない。
