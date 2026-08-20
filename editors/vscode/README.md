# VS Code 拡張

**色分けは二層ある。**

| | いつ効くか | 何を見分けるか |
|---|---|---|
| **TextMate 文法** | 開いた瞬間から。**言語サーバが要らない** | 鍵語・型・数・文字列・注釈・`$名前` |
| **意味トークン** | 言語サーバが繋がってから | 呼び出しと変数の区別、注釈の位置での型 |

VS Code は LSP の意味トークンを扱えるので、**Zed と違って文法だけで終わらない。**

## 入れ方

```bash
cargo install --path ../..        # vaak-lsp を PATH に
cd editors/vscode && npm install && npx tsc -p .
```

VS Code で `F5`（拡張開発ホスト）か、`code --extensionDevelopmentPath=$PWD`。

`vaak-lsp` が PATH に無ければ `vaak.server.path` に道を書く。
**見つからなくても色分けは効く**——警告を出して編集は続けられる。

## TextMate 文法は生成物である

```bash
python3 ../../scripts/gen-tm-grammar.py
```

**鍵語を手で二度書かない。** `src/lexer.rs` と `src/parser.rs` が唯一の出所で、
`cargo test --test grammar` が食い違いを見張っている。
