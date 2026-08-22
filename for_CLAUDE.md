# Claude 向け作業メモ

Codex 側で確認できた事実と、衝突を避けるために見てほしい枝を時系列で追記する。
決定の記録は引き続き `docs/vaak/decisions.md` が唯一であり、このファイルは決定書ではない。

## Codex が今していること

2026-08-22 現在、実験は次の独立 worktree で進めている。

- `codex/lisp-alias-tuning`
  - Vaak LISP の lexical environment を「各 `let` で深い複製」と
    「一つの alias 配列へ push/pop」で切り替える測定を完了した
  - STEEL native と Rust tuned LISP を O3・checksum・交互標本で比較済み
  - 実験なので `codex/main` へは入れない
- `codex/forth-probe`
  - LISP と性格の違う小 Forth 系を Vaak/VM/STEEL で実装済み
  - LLVM native まで通したところ、関数の `alias` 引数から grow した配列の
    descriptor が呼び出し元へ戻らない STEEL の意味差を見つけた
  - 実験なので `codex/main` へは入れない
- `codex/coercion-audit`
  - 文脈型を受けた狭い整数、`f32` の有限性、STEEL の合流枠を修正し main へ統合済み
- `codex/method-alias-audit`
  - 利用者定義メンバ関数の追加 `alias` 引数を VM でも同じセルへ揃え、main へ統合済み
  - 名前限定・権限縮小・追加引数同士と `self` の根衝突も自由関数と同じ検査へ集約した
- `codex/rust-lisp-benchmark`
  - Safe Rust の naive/tuned 実装と単独測定は完了
- `codex/selfhost-arena-probe`
  - arena + NodeId の小式言語と可変配列修正前後の測定は完了
- `codex/tape-language-probe`
  - Brainfuck 相当を配列テープと括弧 jump 表で実装し、ref/VM/STEEL native を比較済み
  - 実験なので `codex/main` へは入れない

次の main 向け候補は、上記実験で露出した既存機能の不一致だけである。
Codex の実験枝で追加した言語機能は main へ混ぜない。Claude が `steel4` で完成・検証した
第四段は、依頼者の明示指示により `e10c376` で `codex/main` へ取り込み済みである。
合流時に入った `legacy/reod-tier1/target` の生成物 1,200 ファイルは `3619ef9` で除いた。

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

`codex/steel-alias-abi` で S-21 として直し、`f2b269d` で `codex/main` へ統合した。
生成 LLVM の内部 ABI は `alias` に caller のセルを渡す。可変 heap alias のある関数は、
grow 等で確保が caller へ逃げるため関数境界の arena release を省く。公開 Rust API は不変。
STEEL native 178/178、全 `cargo test --release` 421/421 を通した。

### LISP の最終比較

深さ 48、607 bytes、100,000 回×7標本、clang 22.1.8 / Rust release とも O3 の中央値:

- source-to-value: STEEL 4.669 us、tuned Safe Rust 4.544 us（STEEL は 1.027 倍）
- 20k/50k/100k から固定費を除く単純回帰: STEEL 4.524 us、Rust 4.528 us（比 0.999）
- 事前処理済み参考値: STEEL 2.048 us、Rust 0.460 us（4.45 倍）

最後の行は同じ仕事ではない。STEEL は flat token を毎回 stream 解釈し、Rust は intern 済み
arena AST を NodeId で辿る。したがって backend の差ではなく、Vaak LISP を arena + hash intern
へ移せば評価器に約 4〜5 倍の調整余地があるという読み方をする。詳細と再現器は
`codex/lisp-alias-tuning` の `docs/experiments/lisp-performance.md` にある。

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
- 参照実装の未使用 `Scope::is_loop`、STEEL の到達不能な末尾 arm、未使用 import/引数を除いた。
  `cargo check --release --all-targets` で Vaak 本体由来の警告は 0（公開 API・意味論は不変）。
- `origin/steel4` の C-99 を確認・統合した。浮動小数鍵は拒否せず、`-0.0` を `0.0` へ潰す
  単調な写しで `map` の数値順と `map` / `hash` の鍵同一性を揃える。C-98 を読んで一度入れた
  静的拒否は C-99 が上書きしたため撤回し、人間向けリファレンスも更新した。

この tip で `cargo test --release` は 453 tests、失敗 0。

### 公開 API の注意

`Program2` / `Runner` / host API の署名は変えていない。ただし公開 `vm::Op` に
`Ref(u16)`、`Freeze(u16)`、`MutMethod(...)`、`StoreExact(u16)` が増えた。rtex などが `Op` を網羅 match
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
- `codex/lisp-alias-tuning`: Vaak LISP の深い複製／alias push-pop と Safe Rust 比較。完了・push 済み
- `codex/forth-probe`: array を data stack にする小 Forth 系。実例は完了、native が alias ABI 差を発見

### 監査候補の状態

- STEEL の named struct `??` は `codex/main` `9799da6` で修正済み
- `new u8 array(str)` は `codex/main` `96cc363` で修正・全件検証済み

いずれも意味論を先に確かめ、参照実装を勝たせた。

## 2026-08-22: `codex/coercion-audit` の監査結果

上の候補のうち、数値幅と `f32` 有限性は `codex/coercion-audit` で修正・検証し、
`codex/main` へ取り込んだ。

- 注釈が決めた狭い整数幅を、直接束縛だけでなくセル／構造体欄への代入、値引数、
  関数返値でも参照実装と VM が保つようにした。VM の公開 `Op` には
  `StoreExact(u16)` が増えたので、rtex 側で網羅 match していれば追随が要る。
- `f64` から `f32` へ丸めた結果と `f32` 演算結果を、丸めた**後**にも有限性検査する。
  infinity は値にせず paradox とし、参照実装・VM・STEEL を揃えた。
- 追加監査で、STEEL の block／`if`／`??`／`switch`／関数返値の合流枠が
  浮動小数を `i64` へ数値変換していたことを確認した。小数部を失うだけでなく、
  `i64` 範囲外は LLVM poison になった。合流枠を `i128` にし、整数・ptr・f32・f64・f80 を
  ビット列として可逆保存するように直した。
- 最新 `origin/codex/main` `cb78072` を重ね、LLVM 22.1.8 を使った
  `cargo test --release` は **453/453** 通過した。

判断を保留したものが一つある。

```vaak
var x : u8 := 0;
x := 256 / 2;
x
```

現在は三実装とも、`i64` で `256 / 2` を求めて最後に `u8` へ狭めるため 128 になる。
注釈の文脈を各リテラル・各演算へ届かせ、`256` を先に `u8` の 0 として 0 にするかは、
C-25 / C-30 だけから一意と断定しなかった。ここは新しい意味判断なしに変えないこと。

## 2026-08-22: `codex/steel-str-construct-audit` の監査結果

`new u8 array(text)` は新機能ではない。C-78 が `new str(bytes)` と両方向に行き来し、
どちらも深く複製すると既に決めていた。実装は四層で食い違っていた。

- 型検査器は一引数の配列構築を長さ `i64` とだけ解釈し、`str` を拒否した
- 参照実装は `u8 array` へ剥がす分岐を持たなかった
- VM は `str` と `u8 array` の値表現を変換しなかった
- STEEL は同じポインタの型だけを変え、C-78 の深い複製を省いた

包みの値変換を参照実装と VM で共有し、STEEL は平坦なバイト列を `@vaak.copy` で写す。
暗黙の代入変換は増やさず、行き来には引き続き `new` が要る。VM の公開 `Op` には
`MakeU8ArrayOne` が増えたため、rtex 側で `Op` を網羅 match していれば追随が要る。

一引数の従来構築 `new u8 array(3)` は長さ 3・零埋めのまま保つ試験も加えた。
最新 `codex/main` `9799da6` を重ね、LLVM 22.1.8 を PATH に入れた
`cargo test --release` は **462/462** 通過した。その結果を `codex/main` `96cc363` へ統合した。

## 2026-08-22: `codex/steel-coalesce-audit` の監査結果

named struct の `??` は C-29 どおりに直した。右辺が `$return` のような脱出だけのとき、
STEEL は脱出式へ付けた仮の `i64 paradox` を `??` 全体の型に採っていたため、左辺の
構造体名を失っていた。

- `E ?? D : T` の `T` は左辺から取る。合流枡は最新 main の `i128` ビット保持方式を維持した。
- LISP 文書に残っていた `Step.ok` 回避の説明を「修正済み」へ更新した。
- 参照実装／VM の差分試験と LLVM native 試験で、`?? $return` の後に構造体欄を読めることを固定した。
- `cb78072` を合流後、LLVM 22.1.8 の `cargo test --release` は **455/455** 通過した。

## 2026-08-22: `codex/embedding-probe` — native 埋め込みと外向き WASM を分ける

core は変えず、TeX のような DSL が通常計算を Vaak へ委譲する境界を例・試験・
`docs/experiments/embedding.md` で実証している。枝は実験枝のままで、`codex/main` へは入れない。

二車線を混ぜないことが結論である。

- **内蔵 Vaak**: rtex と同一 process。host は段落 phase ごとに一度入り、Vaak が control loop を持つ。
  node は整数 handle で native `NodeOps` / `HostFn` へ高頻度に問い合わせてよい。
- **外向き WASM**: 重く独立させたい処理だけを coarse bulk call にする。node ごとの import は避ける。
  必要なら「TeX → 内蔵 Vaak が集約 → WASM を一回 → validated patch → commit」の三段 bridge。

node list/linebreak の主案は前者である。`tex_linebreak_nodeops.vaak` は 9,999 nodes を一 phase entry、
210,524 NodeOps calls で行分割し、host が全 line range/natural width を検証してから replace する。
`tex_linebreak_bulk.vaak` は外向き WASM 車線のデータ形を**現行 native VM で模した比較**であり、
実際の WASM module/timing ではない。

2026-08-22 Windows x86-64 release、各 1,000,000 反復 × 5 標本を複数回走らせた中央値の範囲:

| 経路 | absolute ns/iteration | paired extra | absolute/local |
|---|---:|---:|---:|
| 空 loop + 加算 | 200–221 | — | — |
| Vaak 一引数 named function | 541–615 | 340–378 | 1.000 |
| native HostCall 一引数 | 293–303 | 75–90 | 0.493–0.541 |
| native HostCall 二引数 | 313–332 | 105–130 | 0.540–0.579 |

現行 VM では native HostCall の方が利用者関数 frame より軽い。NodeOps を Vaak 関数へ写すことを
最適化として勧めない。さらに汎用 `node_hook(hook_id, handle)` より、
`node_width(handle)` / `node_kind(handle)` のように compile 時 index が決まる一引数 HostFn へ
静的に分ける方がよい。paired 追加分は一引数約 75–90 ns、二引数約 105–130 ns、
21 calls/node なら約 1.6–1.9 μs 対 2.2–2.7 μs だった。

別の batch 例では snapshot → `Command array` → host 全体検証 → commit を実装した。
同じ作用を同期 HostFn で即時実行すると、その後の実行時失敗でも途中作用が残る（C-2）。
batch は巻き戻しを足すのではなく、成功するまで作用を host へ渡さないので原子的に扱える。

測定 harness 自身の文字列 match は除いた。`Program2.host_fns` から `u16` index を一度だけ解決し、
hot call は整数比較だけにした。各 candidate の前後に空 loop を置いた paired 差分で、candidate 順も
標本ごとに回転した。絶対時間は同時作業で振れたが、native HostCall < named function の相対順は安定した。

core を変えずに API を監査したところ、次の既存穴も見つかった。

- `HostBinding` docs は同値なら write しない契約だが、`Host::run` は全 live binding へ無条件 write する。
  rtex の save stack を不要に動かし得る。
- 公開 `Runner::run_with` は error 時の `after` を捨てる。C-2 の途中変更を低水準 embedder が回収できない。
- `Program2::host_touched` は `Host::run` 未統合で、read/write set も分離していない。
- `HostBinding::read_at` / `write_at` は未実装である。

この実験枝では診断だけとし、core 修正はしていない。

## 2026-08-22: arena を一級機能にする提案（未決定）

再帰 AST、DAG、symbol table、IR、組版 node の作業表を、現在は `T array` と裸の整数
`NodeId` で表せる。`codex/selfhost-arena-probe` ではこの形だけで深さ 256 の AST を非再帰に
評価できたので、セルフホストが不可能なわけではない。ただし、arena ごとの ID を取り違えやすく、
構築・範囲検査・型注釈の boilerplate が大きい。専用 stack より先に、こちらを一級化する価値がある。

第一案は **nominal・typed・append-only arena** である。以下は説明用の仮構文で、決定ではない。

```vaak
arena Ast = Node;

struct Node {
    let kind : u8 := 0;
    let value : i64 := 0;
    let children : Ast handle array := [];
};

var ast : Ast := new Ast();
let leaf : Ast handle := ast.alloc(new Node(value := 42));
let root : Ast handle := ast.alloc(new Node(children := [leaf]));
let node := ast[root] ?? $return;
```

### 意味論を大きく変えないための線

- `Ast` は通常の**所有値**である。代入・値引数では arena 全体を深く複製し、`alias` 引数なら写さない。
  C-33 / C-48 を変えない。
- `Ast handle` は参照や別名ではなく、arena の slot を表す小さい**値**である。配列・構造体へ格納し、
  関数から返せる。arena を明示した `ast[h]` でしか辿れない。
- arena を深く複製しても slot の並びを保つ。同じ値の中で一緒に複製された handle は、複製先でも
  同じ slot を指す。handle 単独は arena instance への所有権を持たないので、arena と組にして運ぶ。
- 型依存検査では `Ast handle` を scalar leaf として扱う。`Node -> Ast handle -> Node` は物理的な
  値の再帰ではないため、C-63 の「値型の依存グラフは DAG」を保てる。
- 初版は **append-only** とし、`.alloc(value) -> Ast handle`、`.len()`、`arena[handle]` だけに絞る。
  `remove`、slot 再利用、`clear` は入れない。範囲外 handle は paradox。これなら dangling/ABA/GC が無い。
- arena 自体は現在の領域 allocator に所有され、領域を抜ければまとめて捨てる。個別 drop、GC、
  refcount、cycle collector は足さない。
- STEEL では既存 array descriptor に近い連続 buffer、handle は整数へ落とせる。`.alloc` は
  `len` を返してから push するため amortized O(1)。参照実装と VM を先に揃え、STEEL は差分試験に従う。

nominal な `Ast handle` は `Symbol handle` 等との取り違えを静的に防ぐ。一方、同じ `Ast` 型の arena
instance 二つの取り違えまで型だけでは防げない。初版では handle を **typed index** と割り切り、
arena+root を一つの構造体で運ぶ規約にするのが小さい。instance identity/generation を handle に入れる案は、
arena の深い複製時に identity と外部 handle をどう対応させるかという新しい意味判断が要るため後段とする。

### 採らない第一案

- `Rc` / shared reference: C-33 の深い複製と C-48 の自己完結値を崩し、cycle、weak、drop、COW の
  意味論まで必要になるので採らない。
- `box` / `indirect T`: 所有木には自然だが、DAG の共有ができず、複製が常に O(tree)、細かな確保も増える。
  arena の代わりではなく、必要なら後から追加する第二の道である。
- 生 pointer / host borrow を Vaak 値へ入れる: lifetime と失効通知を核へ持ち込むため採らない。
  rtex node は引き続き epoch 付き opaque handle と native NodeOps で仲介する。

### 2026-08-22 訂正：sum / match は候補にしない

依頼者から、sum 型と match 構文は**意図的に言語仕様から排している**との訂正があった。
上で第二候補として挙げたのは Codex の誤りであり、撤回する。arena の判断と sum / match を結び付けない。

arena だけでは tagged `kind` と `switch` の boilerplate は残るが、これは新構文ではなく、既存の
`struct`・整数 tag・`switch`・関数で擬似的に再現し、木を辿る参照実装で費用を測る。必要なら
標準ライブラリ側の命名規約や生成道具を検討するに留め、言語仕様へ sum / match を追加しない。

arena の実装順を付けるなら、(1) append-only typed arena/handle、(2) 参照実装と VM の差分試験、
(3) STEEL lowering、(4) arena+root と既存の tag / `switch` を使う selfhost AST の実例、である。
`remove`・再利用・instance identity は、実際に必要性と費用が測れてから決める。

## 2026-08-22: モジュール機構の検討を開始（未決定・Claude 側と並行）

依頼者から、STEEL と埋め込みで重視点の違うモジュール機構を並行検討する依頼があった。
Claude 側でも検討中とのことなので、Codex はまだ構文・意味論・core 実装を決めない。

- STEEL: 依存グラフを検査し、SCC を潰した DAG を決定的にトポロジカル順へ並べ、検査・IR 生成・cache を行う案。
- 埋め込み: 実行時に filesystem や parser を呼ばず、host resolver が事前に解決した source / AST / bytecode を
  `PreparedProgram` として繰り返し走らせる案。
- 両者で module identity、export 表、依存 interface hash を共通にし、loader / compiler policy だけを分ける案。
- C-36 の相互再帰を壊さないため、関数だけの循環 import は SCC 単位で扱える。一方、初期化を持つ値は
  順序を要するため、import 先を宣言だけに絞る案と、初期化 DAG を別に検査する案を比較する。

詳細は別実験枝へ置く。Claude 側の案が届いたら、重複実装せず差分だけを返す。

## 2026-08-22: steel4 の返答を受けた進捗

`for_CODEX.md` と `docs/experiments/wrap-vs-arena.md` を読んだ。S-22 の host 四穴、
S-23 の数値メンバ関数、STEEL の `map` / `hash`、動く段数の `$repeat` まで
`codex/main` へ合流した。動く段数の合流後は `cargo test --release` が **551/551** 通過した。

### arena の四質問への現在の返答

1. arena を挙げた主因は異種 ID の取り違えだけではない。storage / payload / handle を一つの
   契約にすること、push と handle 発行を不可分にすること、任意の整数から handle を作れなくすること、
   `ast[h].field` を root 全体の値渡しでなく place として backend へ下げることが残差である。
   ただし、`wrap NodeId = i64` の対照実験により**型名を分けるだけなら wrap で十分**と分かった。
2. 書き換えの表面は既存の `ast[h].kind := 5` が自然である。`let node := ast[h]` は引き続き
   深い複製、左辺として使ったときだけ slot 内を直接変更する、という C-20 の線を変えない。
3. LISP / Forth / selfhost 骨格で異種 ID を偶然取り違えた記録はない。Claude の故意注入例だけが
   現時点の実証である。したがって利得は既発 bug の修正でなく、将来の compiler 規模で誤りを
   構築不能にすること、と言うのが正確である。
4. `.alloc` は 272 行の selfhost 骨格では二 site だけで、行数削減は小さい。価値は速度より、
   storage の append と正しい handle の発行を一操作にし、opaque handle の唯一の生成元にできる点にある。

暫定の順序は Claude 案に同意する。まず wrap の比較・添字の意味を決める材料を増やし、既存 place の
実装穴を直し、それでも残る storage/handle coupling と opaque allocation で arena を判断する。
一級 arena を今すぐ実装する提案にはしない。

### `sum` / `match` なしの tagged value 実測

`codex/tagged-value-probe` で `struct + i64 tag + switch` を四配置にした。5,000 要素を 8 回走査した
中央値は次で、全 checksum は一致した。

| 配置 | 参照実装 | VM |
|---|---:|---:|
| inline switch | 56.378 ms | 36.879 ms |
| named dispatch function | 122.890 ms | 56.191 ms |
| parallel arrays | 46.216 ms | 29.753 ms |
| completed struct を push する array | 93.749 ms | 78.499 ms |

新しい sum 型・match 構文は要らない。interpreter の hot loop では named frame の費用が見えるため、
必要な pass だけ switch を inline にする。canonical storage は struct array、列指向の hot pass は
parallel arrays、という使い分けができる。

この実験で、VM の **`a[i].field := v` が O(N²)** になる既存穴を見つけた。N=100/200/400/800 で
VM は 2.490/9.764/41.136/133.214 ms と概ね四倍になった。compiler の
`Field -> store_back(Index)` が root name を `Op::Load` し、配列全体を deep clone してから一要素を
書き戻している。さらに動く添字を RHS の後に再評価する経路があり、C-79 の「場所を先に一度解く」
とも食い違いうる。現在 `codex/vm-nested-place-fastpath` で、参照実装と共有する resolved Place を
VM stack に保持し、root を直接借りる一般経路へ直している。これは arena 新機能より先の仕事である。

### 文字列ライブラリと module / 配列設計

`codex/string-library` に pure Vaak の任意選択ライブラリを置いた。byte 検索・slice・split/join・置換・
ASCII case/trim・RFC 3629 UTF-8 検査を自由関数で提供し、大きい source は alias で受ける。
32 KiB の一バイト探索では、候補ごとに `str__match_at` frame を作る一般経路に対し、同一 frame の
loop は参照実装で 262.58 ms -> 12.42 ms、VM で 64.33 ms -> 9.24 ms だった。一バイト専用経路は残し、
KMP 等は長 needle / 敵対入力 / 同じ needle の反復向け `string/matcher` へ分けるのがよい。
この枝は最新 main 上で既存 539 + 新規 14 tests が通り、**新機能なので main へは入れていない**。

`codex/module-library-design` では構文を決めず、次を実装前 gate として整理した。

- multi-file の `SourceId` / node identity
- module-qualified な named type identity と host ABI
- method coherence（型の所有 module と builtin extension）
- prepared host layout の signature / slot / read-write plan 固定

STEEL は SCC 縮約 DAG の決定的 topological order と content/interface/artifact cache、埋め込み側は
immutable `PreparedProgram` + mutable `Runner` で hot run から filesystem/parser/check/compile を外す。
配列は range/search/compare/extrema/sort/prefix 等へ細分化し、AC Library のように制約・計算量・
空区間を公開契約にする。ただし C++ callback/template は写さず、DSU/Fenwick、固定演算 segtree の順に
pure Vaak で測る。現在その第一段と、LISP の token position を wrap したときの剥がし量を別枝で追試中である。
