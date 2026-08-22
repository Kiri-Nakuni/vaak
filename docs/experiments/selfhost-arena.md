# 小式言語 #1：arena/NodeId で Vaak-on-Vaak

## 結論

この実験の範囲では、**組み込みの stack が無いことはセルフホスティングを妨げない。**
動的配列の `push` / `pop` がそのまま作業積みになる。再帰的な AST 型が作れない点も、
節を一つの配列へ置き、子を整数 `NodeId` で指せば避けられる。

ただし、これは「Vaak コンパイラ全体を Vaak で書けた」という証明ではない。
字句解析、優先順位つき構文解析、AST 構築、深い木の非再帰評価という、
セルフホストに必要な中核経路が現行機能だけで通ることを確かめたものだ。

実例は [`examples/vaak/07-セルフホスト骨格.vaak`](../../examples/vaak/07-セルフホスト骨格.vaak)、
再現用の測定器は [`examples/bench_selfhost.rs`](../../examples/bench_selfhost.rs) にある。

## 作った言語

ASCII の整数四則式を扱う小さな式言語である。

```text
12 + 3 * (4 + 5) + 6 / 2
```

この入力を次の順で 42 にする。

```text
str
  -> Token array
  -> Node array + root NodeId
  -> work stack を使う post-order walk
  -> i64
```

- 字句器は空白、十進整数、`+ - * / ( )` を認識する
- token は値に加え、元の source に対する byte span を持つ
- parser は shunting-yard 法で優先順位と括弧を処理する
- AST は arena に平坦化し、`left` / `right` は整数添字にする
- evaluator は再帰せず、`work_nodes` と `expanded` の二配列へ frame を積む
- 0 除算、壊れた式、範囲外 NodeId は値を捏造せず paradox へ落とす

主例は 13 tokens、11 nodes、16 node visits になる。さらに試験では左へ 256 段伸びる
513-node の木を評価し、最大 513 枠の作業積みを使うことを確認した。
深さを host call stack の深さにしていない。

## データの置き方

### token は文字列を所有しない

token ごとに lexeme を `str` として複製すると、識別子やリテラルの総 byte 数だけ確保が増える。
そこで token は `start` / `end` の整数 span を持ち、source は一つの `str alias` として借りた。
完全な lexer でも、keyword 判定と identifier intern の瞬間だけ source の範囲を比べればよい。

### AST は tagged struct の arena

```text
Node {
    kind,
    value,
    left: NodeId,
    right: NodeId,
    start,
    end,
}
```

Vaak の値は自己完結し、別名は値の中へ入らない（C-48）。再帰構造体も作れない。
一方、整数 ID は通常の値なので、木、DAG、親参照、シンボル参照を同じ方法で表せる。
節を加えた後も既存の ID は変わらない。

これは shallow copy の代用品というより、**共有を値から外へ出す表現**である。
木を複製せず共有したければ、同じ NodeId を複数の欄へ置く。
書き換え可能な場所を共有したければ、arena 自体を `alias` 引数で渡す。

### stack は array の用法

構文解析は `operators` / `values`、評価は `work_nodes` / `expanded` / `values` を使う。
必要な操作は次だけだった。

```text
push(value)
pop() ?? 失敗時の脱出
len()
clear()
```

これは現行の動的配列がすべて持つ。専用 stack 型を中核へ足しても、
この実験では新しい表現力も新しい計算量も得られない。

## 書き味

### 簡潔だったところ

- `fn f(a : T array alias)` で、大きな arena を写さないことと破壊可能性が署名に出る
- `tokens.push(...)` と `values.pop() ?? $return` で、積み操作と失敗伝播を直接書ける
- 構造体のおかげで token、node、測定結果の欄名が保たれる
- `str` の添字が byte なので、ASCII lexer と source span は余分な変換なしに書ける
- arena の生存期間を自前で管理する必要がない。所有する配列の領域に従う

### 冗長だったところ

- 直和型と pattern matching が無いので、`kind` と使わない欄を持つ tagged struct になる
- `alias` は名前しか指せないため、欄や一時値を直接 out-param に渡せず、いったん名前へ束縛する
- 複数の作業積みを同期して `push` / `pop` する frame 表現は、`Frame array` より長い
  （一方 `Frame array` は構築の回数が増える）
- STEEL は無名標準ライブラリをまだ注入しないため、例の先頭で `$return` を定義している
- identifier、文字列 literal、診断まで入れると、source span の比較・hash・UTF-8 方針を
  ライブラリとして揃える必要がある

算術式言語としては約 270 行になった。半分近くは異常系の確認と、
token/node/report の明示的な欄である。アルゴリズム自体は素直だが、tagged union の記述量は多い。

## 測定

2026-08-22、Windows x86-64、`cargo run --release --example bench_selfhost -- 深さ 9`
で測った。Rust 側で Vaak の解析・静的検査・VM 翻訳を一度だけ済ませ、各標本は
**ゲスト内の字句解析 + arena 構築 + 反復評価**と実行器の初期化を含む。中央値である。

### 可変メソッドを直す前

| 左深さ | tokens | nodes | visits | 参照実装 | VM | VM / 参照 |
|---:|---:|---:|---:|---:|---:|---:|
| 8 | 33 | 17 | 25 | 1.924 ms | 0.713 ms | 0.371 |
| 32 | 129 | 65 | 97 | 15.719 ms | 5.715 ms | 0.364 |
| 64 | 257 | 129 | 193 | 54.989 ms | 18.941 ms | 0.344 |
| 128 | 513 | 257 | 385 | 207.718 ms | 73.143 ms | 0.352 |

入力と訪問回数は線形なのに、参照実装と VM の時間は線形より大きく伸びた。
原因は stack という抽象の不足ではなく、**現在の Rust 実装で破壊的メンバ関数が
レシーバ全体を複製してから書き戻す経路**にある。

- 参照実装は `read_place` で配列全体を clone し、`write_method` 後に cell へ戻す
- VM も `Op::Method` へ receiver の `Value` clone を積み、変更後に根へ戻す

したがって長さ N まで一個ずつ `push` する処理が、現状の二実行器では実質 O(N²) になる。
`Vec::push` 自体や arena/NodeId が原因ではない。STEEL への LLVM IR 翻訳は成功したが、
この環境には clang が無いため native 実行時間は測っていない。

この結果から先に行うべき最適化は、専用 stack の追加ではなく、
**名前または解決済み place にある配列を cell 内で直接 mutate する fast path**である。
同じ改善が stack、token buffer、AST arena、一般の利用者配列へ一度に効く。

### cell 内で直接変更した後

上の原因を `codex/inplace-collection-methods` で直し、同じ測定を 11 標本で取り直した。

| 左深さ | tokens | nodes | visits | 参照実装 | VM | VM / 参照 |
|---:|---:|---:|---:|---:|---:|---:|
| 8 | 33 | 17 | 25 | 2.120 ms | 0.499 ms | 0.235 |
| 32 | 129 | 65 | 97 | 12.963 ms | 1.496 ms | 0.115 |
| 64 | 257 | 129 | 193 | 47.793 ms | 3.286 ms | 0.069 |
| 128 | 513 | 257 | 385 | 168.044 ms | 6.277 ms | 0.037 |

VM は深さ 128 で 73.143 ms から 6.277 ms、約 11.7 倍になった。32 から 128 へ
仕事量が約 4 倍になると実行時間も約 4.2 倍であり、配列 work stack の主要経路は
ほぼ線形になった。参照実装にはまだ超線形な費用が残るので別途 profiling が要るが、
少なくとも専用 stack の追加で解ける問題ではない。既存 array の一度の修正が、
token buffer、AST arena、operator stack、評価 stack のすべてへ効いた。

## alias が解いたこと、解かないこと

lexer と parser は出力配列を `var ... alias` で受ける。これにより関数境界の深い複製を避け、
一つの arena を段階間で共有できる。実験中、VM が通常関数の alias 引数を値として渡していた穴も
露出した。参照実装では 42、修正前 VM では 0 だった。VM を参照実装と同じ cell 共有へ直すと、
主例と 256 段例の両方が一致した。

一方、alias は値ではないため、次は直接は書けない。

- node の欄に別の cell への alias を格納する
- closure の環境を、任意の共有 cell の集合として値に入れる
- 配列要素だけを長寿命の参照として渡す

コンパイラの AST と symbol なら arena + ID で大半を解ける。
ただし任意の共有グラフを自然に表したい用途では、手動の heap 配列と整数 handle が常に必要になる。
ここが shallow copy 不在をコーディングだけで補うときの記述上の限界である。

## セルフホストへ残る本当の穴

優先度は次のように見える。

1. **STEEL の map または同等の intern table**
   - 完全な lexer/name resolver は identifier から SymbolId への表を頻繁に引く
   - 配列だけでも open addressing は書けるため不可能ではない
   - ただし hash、再hash、source span 比較まで利用者コードになり、実装量と定数倍が増える
2. **任意入力と成果物を運ぶ host I/O**
   - 埋め込んだ文字列なら今もコンパイルできる
   - 自分の source file を読み、LLVM IR や bytecode を書くには STEEL 側の host interface が要る
3. **tagged union と診断用のライブラリ表現**
   - arena + integer tag で書けるので必須の新機能ではない
   - enum/match 相当があれば、compiler source と node size は小さくなる
4. **source slice / identifier intern の共通部品**
   - `str alias` + byte span で実装可能
   - shallow substring を値にしない設計では、span を標準的な handle として扱う方が合う

結論として、**stack は追加を勧めない。** まず配列の破壊的操作を本当に in-place にし、
STEEL の map/intern と host I/O を進める方が、セルフホスト可能性と通常コードの双方に効く。
この判断では Claude 宛ての「stack を追加してほしい」という依頼は作らない。

## 再現

```bash
cargo run --release --bin vaak -- examples/vaak/07-セルフホスト骨格.vaak
cargo test --release --test selfhost
cargo run --release --example bench_selfhost -- 64 11
cargo run --release --bin steel -- examples/vaak/07-セルフホスト骨格.vaak --emit-ir
```

試験は参照実装と VM の結果を照合し、同じ program が STEEL の LLVM IR に変換できることも確かめる。
