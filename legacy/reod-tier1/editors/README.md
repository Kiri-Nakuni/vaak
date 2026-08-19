# エディタ対応

`.reod` ファイルのシンタックスハイライト。

| | |
|---|---|
| `vscode/` | TextMate grammar。そのまま使える |
| `tree-sitter-reod/` | tree-sitter 文法。Zed が使う |
| `zed/` | Zed 拡張（`tree-sitter-reod` を参照する） |

## VSCode

```bash
ln -s "$PWD/editors/vscode" ~/.vscode/extensions/reod
```

VSCode を再起動すると `.reod` が色付く。

## Zed

コマンドパレットから `zed: install dev extension` を選び、`editors/zed` を指定する。

**Zed はローカルパスでの文法参照を受け付けない**（`path` は不可、`repository` + `rev` が必須で、必ず git から clone する）。そのため `editors/tree-sitter-reod` 自体をローカル git リポジトリにしてあり、`extension.toml` は `file://` URL でそれを指している。

```toml
[grammars.reod]
repository = "file:///home/suima/Documents/mydsl/editors/tree-sitter-reod"
rev = "..."
```

パスが絶対で埋まっているので、**プロジェクトを別の場所へ移したら `extension.toml` の `repository` を書き換える**必要がある。GitHub 等へ公開する場合はその URL に差し替えればよい。

`editors/tree-sitter-reod/.git` は入れ子のリポジトリになる。上位を `git init` する際は、`.gitignore` に加えるかサブモジュールにするかを決めること。

## 文法の再生成

`grammar.js` を編集したら：

```bash
./scripts/sync-zed-grammar.sh
```

parser の再生成 → 全例のパース検証 → 全クエリの検証 → コミット → `extension.toml` の `rev` 更新、までを行う。パースに失敗した場合は `rev` を更新しないので、壊れた文法が Zed に載ることはない。

そのあと Zed で `zed: reload extensions`。

手動でやる場合：

```bash
cd editors/tree-sitter-reod && npx tree-sitter-cli generate
cd editors/tree-sitter-reod && npx tree-sitter-cli parse test/syntax-coverage.reod
```

コミットして `extension.toml` の `rev` を新しい SHA に更新するのを忘れないこと（`rev` が古いままだと Zed は前の文法を使い続ける）。

`test/syntax-coverage.reod` は Tier 1 のインタプリタでは実行できない構文（`typ` / `nfor` / `ifor` / `match` / 配列 / レジスタ層）も含む。ハイライトは設計書の全体をカバーする必要があるため。

## 文法とインタプリタの関係

インタプリタは AST を作らない（読みながら実行するカーソルマシン）。tree-sitter 文法はそれとは別に「全体を構文解析できる部分集合」を定義したもので、両者は別物である。

とくに、**`goto` で読み飛ばされる区間が構文的に壊れていてもよい**（設計書 §4.3、例4）という性質は tree-sitter では表現できない。そういう区間はエディタ上で ERROR ノードになるが、実行には影響しない。

もう一箇所ずれている。ブロックの末尾が `if ... else ...` の場合、それを「末尾式」と読むか「文」と読むかは構文だけでは決まらない。インタプリタは「残りがちょうど1つの式として読み切れるか」で判定する（`src/interp.rs` の `eval_block_value`）。tree-sitter 側は文として読む側に倒してある——ハイライトには影響しない。
