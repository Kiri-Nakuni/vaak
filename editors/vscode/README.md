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
cd editors/vscode
npm ci
npm run compile
```

VS Code で `F5`（拡張開発ホスト）か、`code --extensionDevelopmentPath=$PWD`。

platform 版 VSIX は `vaak-lsp` を同梱する。portable 版や開発ホストでは、
`vaak-lsp` が PATH に無ければ `vaak.server.path` に道を書く。
**見つからなくても色分けは効く**——警告を出して編集は続けられる。

## VSIX にする

ビルドには Node.js 20 以上を使う。

```bash
cd editors/vscode
npm ci
npm run package:vsix           # 現在のplatform用。vaak-lspを同梱
code --install-extension vaak-win32-x64.vsix

# LSPを同梱せず、PATHまたはvaak.server.pathを使う版
npm run package:vsix:portable
```

`package:vsix` は Rust の `vaak-lsp` をrelease buildし、`win32-x64`、
`linux-x64`、`darwin-arm64` 等の現在のtargetを付けたVSIXを作る。
明示した `vaak.server.path` は同梱版より優先される。`package:vsix:portable` は
LSPを含めず、どのplatformにも入れられる小さい版を作る。

Node依存は `package-lock.json` で固定される。VSIXと同梱binaryは同じ手順で
作り直せるビルド成果物なのでGitには入れない。

## TextMate 文法は生成物である

```bash
python3 ../../scripts/gen-tm-grammar.py
```

**鍵語を手で二度書かない。** `src/lexer.rs` と `src/parser.rs` が唯一の出所で、
`cargo test --test grammar` が食い違いを見張っている。
