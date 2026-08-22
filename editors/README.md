# 編集器の対応

| | 色分け | 診断・説明・補完 | 入れ方 |
|---|---|---|---|
| [Zed](zed/) | tree-sitter 文法 | 言語サーバ | `zed: install dev extension` |
| [VS Code](vscode/) | TextMate 文法＋**意味トークン** | 言語サーバ | `npm install && npx tsc -p .` |

**言語サーバはどちらも同じもの**（`vaak-lsp`）である。

```bash
cargo install --path .
```

## 文法が二つあることについて

**色分けの文法は編集器ごとに要る。** Zed は tree-sitter、VS Code は TextMate、
そして VS Code だけが LSP の意味トークンも使える。

**だが鍵語の一覧は一つである。**

| | 出所 |
|---|---|
| tree-sitter（`grammar.js`） | 手書き。**`cargo test --test grammar` が字句器と突き合わせる** |
| TextMate（`.tmLanguage.json`） | **生成物。** `scripts/gen-tm-grammar.py` が字句器から作る |
| 意味トークン | **字句器そのもの。** `src/lsp.rs` が `vaak::lexer::lex` を呼ぶ |

**片方に鍵語を足してもう片方に足し忘れたら、試験が落ちる。**
