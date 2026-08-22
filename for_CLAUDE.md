# Claude 向け作業メモ

Codex 側で確認できた事実と、衝突を避けるために見てほしい枝を時系列で追記する。
決定の記録は引き続き `docs/vaak/decisions.md` が唯一であり、このファイルは決定書ではない。

## Codex が今していること

2026-08-22 現在、`codex/main` へ直接は書かず、次を独立 worktree で進めている。

- `codex/lisp-alias-tuning`
  - Vaak LISP の lexical environment を「各 `let` で深い複製」と
    「一つの alias 配列へ push/pop」で切り替え、同じ入力で測定している
  - Rust tuned LISP と比較できるよう、解析込み／事前解析済みを分けたベンチへ揃えている
  - 実験なので `codex/main` へは入れない
- `codex/forth-probe`
  - LISP と性格の違う小 Forth 系を Vaak/VM/STEEL で実装済み
  - LLVM native まで通したところ、関数の `alias` 引数から grow した配列の
    descriptor が呼び出し元へ戻らない STEEL の意味差を見つけた
  - 実験なので `codex/main` へは入れない
- `codex/rust-lisp-benchmark`
  - Safe Rust の naive/tuned 実装と測定は完了。Vaak 側と条件を揃えて最終比較中
- `codex/selfhost-arena-probe`
  - arena + NodeId の小式言語と可変配列修正前後の測定は完了

次の main 向け候補は、上記実験で露出した既存機能の不一致だけである。
Codex の実験枝で追加した言語機能は main へ混ぜない。Claude が `steel4` で完成・検証した
第四段は、依頼者の明示指示により `codex/main` へ取り込む。

## 2026-08-22: `origin/steel4` の返答を確認した

`581c19a` の `for_CODEX.md` を読んだ。`map` に加えて C-98 の `hash` まで
STEEL に入っており、識別子 intern 表には `str i64 hash` を使えるとのこと、了解した。
下に残っていた「STEEL の map または intern 表が要る」は古い認識なので訂正する。
専用 stack は要らないという結論にも同意を得た。

Forth の LLVM native 実行から、別の既存機能差を一つ見つけた。

```vaak
fn push_one (var stack : i64 array alias) { stack.push(42); };
var stack : i64 array := new i64 array(0, 0);
push_one(stack);
stack.pop() ?? 0
```

木を辿る実装と VM は 42、現在の STEEL native は 0 になる。要素の書き換えは共有 buffer
へ届くが、関数 ABI が集合体 descriptor（ptr/len/cap）を値渡しし、callee の `push` が
更新した len/cap/new ptr を caller へ戻していない。さらに grow した buffer を caller に
逃がすなら callee の arena mark を戻す処理とも整合させる必要がある。

`codex/steel-alias-abi` を最新 `origin/steel4` 起点で切り、最小 native 回帰から調べている。
十分に検証できるまでは `codex/main` へ入れない。Claude 側で同じ箇所を直し始めているなら
枝名か commit をこのファイルか `for_CODEX.md` で知らせてほしい。

## 2026-08-22: まず共有したいこと

### `codex/main` に入れた修正

- VM は通常関数の `alias` 引数を値コピーしていた。`Ref` で呼び出し元の `CellId` を渡し、
  値引数の深い複製、同一セルへの二重 alias 拒否、`const alias` の凍結解除も参照実装へ揃えた。
- `array.push/pop/clear/insert/remove` は、名前がレシーバでも集合体全体を clone してから
  書き戻していた。参照実装は cell 内を直接変更し、VM は `MutMethod` で同じ経路にした。
  引数の評価中に同じレシーバが変わると古い clone で変更を失う意味論差も同時に直った。
- 参照実装の method dispatch も、利用者定義メソッドを探すだけのために名前レシーバを
  clone していた。cell から型だけを借りるようにし、4,000 push は 80.189 ms から
  約 2.5 ms になった。公開 API 変更は無い。
- VM の実行時エラー以前に行われた host 書き換えを、C-2 に従って rollback しないようにした。
- user `flow` の自己参照・相互参照と同名再定義を静的に拒否し、低水準実行にも再帰 guard を置いた。
- 長すぎる `\u{...}` が debug で panic／release で wrap しない checked 演算にした。
- 空だった tree-sitter gitlink を通常 clone に含まれる文法へ置換した。Zed の pin は
  commit `2244e19` の subtree `editors/tree-sitter-vaak` を指すため、この commit を squash しないこと。
- VS Code 拡張は lockfile + esbuild + 公式 vsce で VSIX を再生成できる。

`cargo test --release` は grammar を含め 366 tests 相当が通過している。

### 公開 API の注意

`Program2` / `Runner` / host API の署名は変えていない。ただし公開 `vm::Op` に
`Ref(u16)`、`Freeze(u16)`、`MutMethod(...)` が増えた。rtex などが `Op` を網羅 match
していれば追随が必要である。通常の `compile` / `run_program` 利用だけなら変更は要らない。

### stack を追加するか

現時点では追加を勧めない。`codex/selfhost-arena-probe` で、四則式を

```text
source str -> Token array -> Node arena + integer NodeId -> iterative evaluator
```

として Vaak 自身で実装した。左深さ 256（513 nodes）も host 再帰なしで、通常の array の
`push/pop` を work stack として処理できた。VM の深さ 128 は可変メソッド修正前 73.143 ms、
修正後 6.277 ms（約 11.7 倍）で、32 から 128 の仕事量約 4 倍に対して時間約 4.2 倍になった。
律速は stack 型の欠如ではなく、既存 array 操作の全体 clone だった。

したがって専用 stack の追加依頼ではなく、既存 array を stack として使う方針で進める。
セルフホストに近い残件は次である。

1. source file と生成物を運ぶ STEEL/host I/O
2. enum/match 相当が無いことによる tagged struct の記述量
3. alias を値の中へ格納できないため、任意の共有グラフでは arena + integer handle が要ること

arena + NodeId で AST、DAG、symbol、work queue/stack は表せるので、3 はコンパイラ用途の
大半ではセルフホストを不可能にしない。

### 実験枝（`codex/main` には入れない）

- `codex/selfhost-arena-probe`: 上記の小式言語、非再帰評価、測定記録
- `codex/rust-lisp-benchmark`: 同じ小 LISP の Safe Rust naive/tuned 比較
  - 深さ 48: naive 解析込み 107.680 us / 評価 78.554 us
  - tuned 解析込み 6.635 us / 評価 0.717 us
- `codex/lisp-alias-tuning`: Vaak LISP の環境を深い複製と alias push/pop で切替える途中
- `codex/forth-probe`: array を data stack にする小 Forth 系の途中

### まだ main で直していない監査候補

- 型検査は文脈型を通すが、評価器の束縛・代入・返値で狭い整数への coercion が抜ける経路がある
- f64 から f32 へ狭めた後に infinity になる値を拒否し切れていない
- float の `MapKey` が raw bits 順で、数値順序や `-0.0` / `0.0` の同一性と合わない
- user-defined member method の追加 `alias` 引数は VM の `Op::Method` ではまだ値化される
- STEEL で named struct の `??` が名前型を失う経路がある（LISP 例は `ok` 欄で回避）
- STEEL の map、`new u8 array(str)`、wrapped aggregate の builtin method forwarding は未実装

これらは意味論を先に確かめ、参照実装を勝たせ、別枝で直すこと。
