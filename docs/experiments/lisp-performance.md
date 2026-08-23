# 小 LISP の別名調整と Safe Rust 比較

[Vaak で書いた小 LISP](lisp.md)の字句環境を、値の深い複製と `alias` の
push/pop で書き分け、参照実装・VM・STEEL native・Safe Rust を測った記録です。
言語機能の提案ではなく、現在ある意味論でどこまで性能を詰められるかを確かめる実験です。

実装は [06-LISP.vaak](../../examples/vaak/06-LISP.vaak)、Vaak 内の比較器は
[bench_lisp.rs](../../examples/bench_lisp.rs)、STEEL と Rust の交互測定器は
[bench_lisp_steel.rs](../../examples/bench_lisp_steel.rs) にあります。Rust 側の実装は
[rust_lisp.rs](../../examples/support/rust_lisp.rs) です。新しい Rust コードは
`unsafe` を禁じ、外部 crate を使っていません。

## 何を変えたか

素朴な環境は `let` ごとに二本の配列を深く複製します。

```vaak
var scoped_names := names;
var scoped_values := values;
scoped_names.push(name);
scoped_values.push(bound.value);
let body := eval(tokens, next, scoped_names, scoped_values, true);
```

調整版は、関数へ `alias` で渡した一組の配列を作業 stack にします。

```vaak
names.push(name);
values.push(bound.value);
let body := eval(tokens, next, names, values, false);
names.pop();
values.pop();
```

`body` が通常の値でも失敗を表す `Step` でも、呼び出し側へ戻ってから必ず二本とも
`pop` します。実行器そのものが中断する誤りでは環境を再利用しないため、巻き戻す必要は
ありません。専用 stack 型は使わず、既存の可変配列だけで書けます。

この実験の途中で、名前レシーバに対する `push` が集合体全体を隠れて複製する実装差を
見つけました。参照実装と VM を cell 上の直接変更へ揃え、利用者定義メソッドの探索も
型を調べるだけの深い複製を除きました。4,000 回の `push` は参照実装で
80.189 ms から約 2.5 ms になりました。以下はその修正後の値です。

## 深い複製と alias push/pop

深さ 48、607 bytes の nested `let` を一標本 10 回、9 標本測り、中央値を取りました。
単位は一評価あたりの ms です。

| 実行器 | 入力処理 | 深い複製 | alias push/pop | 短縮 |
|---|---|---:|---:|---:|
| 参照実装 | tokenize + stream eval | 13.943 | 11.779 | 15.5% |
| 参照実装 | tokenized + stream eval | 11.914 | 9.668 | 18.9% |
| VM | tokenize + stream eval | 1.822 | 1.762 | 3.3% |
| VM | tokenized + stream eval | 1.054 | 0.919 | 12.8% |

alias は効果がありますが、この入力では名前探索、文字列比較、`Step` の構築、再帰呼び出しも
残ります。環境複製だけを消して全体が桁違いに速くなるわけではありません。一方、深さを
増やしたときに環境複製量が二次的に育つ道は消えます。

## STEEL native と tuned Safe Rust

比較条件は次のとおりです。

- 深さ 48、607 bytes、答え 47 の同じ nested `let`
- LLVM 22.1.8 の clang と Rust release をともに O3
- 全反復の答えを checksum に入れ、最適化で評価を消せない形にする
- 正解検査は計測区間の後で行う
- Rust は各 workload を 200 ms 以上 warm-up する
- STEEL と Rust の標本順を交互にし、中央値を代表値にする

100,000 回を 7 標本測った結果です。

| pipeline | STEEL native | tuned Safe Rust | STEEL / Rust |
|---|---:|---:|---:|
| source から値まで | 4.669 µs | 4.544 µs | **1.027** |
| 事前処理済み表現から値まで | 2.048 µs | 0.460 µs | **4.45** |

最初の行は、各実装が入力文字列から同じ意味の値を得るまでの全 pipeline 比較です。
STEEL は tokenize して平らな token 列を直接評価し、Rust は借用 lexer、名前 intern、arena AST
を作って評価します。実装戦略は違いますが、外から見た仕事の境界は同じです。この条件では
STEEL は 2.7% 遅いだけでした。

STEEL 側の process 起動費を見分けるため、20,000・50,000・100,000 回の中央値から
`総時間 = 固定費 + 反復数 × 一回の費用` を単純に当てはめると、傾きは STEEL
4.524 µs、Rust 4.528 µs、比は 0.999 でした。三点だけの補助的な推定ですが、少なくとも
source-to-value の定常費用を「STEEL が何倍も遅い」と見る根拠はありません。

二行目は同じ仕事ではありません。STEEL は `str array` を毎回 stream 解釈し、演算子判定、
整数解析、文字列の名前比較を行います。Rust は演算子・整数・名前を解決済みの arena AST を
`NodeId` で辿り、確保済みの evaluator も再利用します。したがって 4.45 倍は LLVM backend と
Rust backend の差ではなく、主に中間表現の差です。

## ここから詰められるもの

測定から、次の優先順位が見えます。

1. Vaak 側も token stream のまま評価せず、arena + 整数 `NodeId` の AST を一度作る。
2. 識別子を `hash` で intern し、評価中の名前を整数で比較する。
3. 空の環境配列を評価ごとに作らず、容量を保った evaluator を再利用する。
4. source-to-value、同じ中間表現の evaluator、process 起動を別々に測る。

最新の STEEL 第四段には `hash` があり、1 と 2 は新しい基底データ構造を足さずに書けます。
セルフホスト実験でも arena + NodeId + 配列 work stack は成立しました。性能調整の限界を
上げるために専用 stack を追加する必要はなく、重要なのは arena、intern、安定した整数 handle、
そして大きな集合体を意図せず複製しない `alias` の実装です。

## 再現

```powershell
cargo run --release --example bench_lisp -- --depth 48 --iterations 10 --samples 9 --tsv

cargo run --release --example bench_lisp_steel -- `
  --depth 48 --iterations 100000 --samples 7 `
  --clang "C:\Program Files\LLVM\bin\clang.exe"
```

絶対値は電源状態、温度、割り込み、process 起動で揺れます。比を取り直すときは、ほかの
release build を止め、同じ入力・最適化・反復数で交互に測ってください。
