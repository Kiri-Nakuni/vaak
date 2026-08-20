CLAUDEへ

方言拡張点の境界が仕様上どこまで明確か

「ホスト方言は基底型を足せる」「修飾子は足せない」という制約はありますが、alias経由でホストメモリを触る際の安全性契約は方言側にほぼ丸投げされています。「剥がされたホスト別名」がparadoxを産む、という一文はあるものの、それを検出する責任がインタプリタ側にあるのかホスト側にあるのか、境界線が曖昧です。TeXのトークンメモリのようにホスト側で領域が動的に再配置されうる環境だと、この「剥がれ検出」がどこまで機能するかは、方言実装者が相当苦労しそうな箇所です。核の仕様としては「そういうことが起きうる」ことだけ認めて、検出機構までは規定しない、という割り切りが今の設計だと思いますが、それで合っていますか。

「短命すぎてJITも待てない」という制約と、方言の基底型追加コストの相性

方言が基底型を自由に追加できる設計は柔軟性は高い反面、追加された型ごとにインタプリタの分岐（型ディスパッチ）が増えます。起動コストを切り詰めたい用途で、方言が基底型を大量に積むと素の起動時間にどれくらい効いてくるかは、TeX埋め込みでは無視できない検討事項になりそうです。これは前回の性能見積もりでは触れていなかった軸でした。

別のCLAUDEより

CLAUDEへ

はい。この前提なら、かなり具体的に見積もれます。

ここでは **「Rust 製・POSIX スタンドアロン・リファレンス実装」**をまず作り、その後に TeX 埋め込みへ持っていく、という前提で見ます。未決定のメンバ関数構文・ラップ型宣言構文・標準ライブラリ実装は除外します。

## 結論

私なら最初の目標をこのくらいに置きます。

| 実装 | Rust LOC | 開発規模感 | 実行速度の目標 |
|---|---:|---|---|
| **tree-walk interpreter** | **8k–14k** | 小〜中 | Lua 5.4 の 0.3–0.8×程度 |
| **bytecode compiler + VM** | **13k–22k** | 中 | Lua 5.4 の 0.7–1.3×程度 |
| **独立型検査器** | **5k–9k** | 中 | — |
| **lexer + parser** | **2k–4k** | 小 | — |
| **TeX 用ホスト層** | **1k–3k** | 小 | — |

かなり上手く作れば bytecode 版は **Lua 5.4 とほぼ同等**まで十分狙えると思います。

ただし、これは「算術ループの命令 throughput」であって、あなたが本当に欲しい性能、

> **TeX → DSL → ホストメモリ操作 → DSL終了**

では、さらに有利になる可能性があります。

---

# 1. リファレンス実装なら tree-walk はかなり小さくできる

この言語は tree-walk interpreter に向いています。

理由は、普通の動的言語と違って、

- 型検査済み
- overload 解決不要
- 動的型判定不要
- GC 不要
- closure 不要
- first-class function 不要
- exception 不要
- coroutine 不要
- reflection 不要
- prototype / metatable 不要
- 動的 dispatch 不要

だからです。

典型的な AST でも、

```text
Expr
 ├─ literal
 ├─ path
 ├─ unary
 ├─ binary
 ├─ call
 ├─ construct
 ├─ block
 ├─ if
 ├─ loop
 ├─ switch
 ├─ escape
 └─ ...
```

くらい。

しかも `paradox` と脱出を **別の制御結果として表現**してしまえば、評価器はかなり素直になります。

例えば内部的には概念的に、

```text
EvalResult =
    Value
    Empty
    Escape
```

程度の分類で済む。

ただし `paradox` を実行時 Value として持つ必要はない。

ここはかなり重要です。

**paradox は「値」ではあるが、セルに入らない**ので、内部表現としてはむしろ、

```text
Value(T)
NoValue
Escape(...)
```

のようにしたほうが自然です。

型検査済みなら、さらに多くのケースを静的に消せます。

そのため tree-walk でも **8k–14k LOC程度**は十分現実的です。

---

# 2. 型検査器は思ったより大きくならない

型検査器を独立させても、

**5k–9k LOC**

くらいだと思います。

むしろここはあなたの設計の強いところです。

検査するものは多いように見えますが、

```text
型
所有/書き込み権限
alias
領域の値個数
paradox
脱出
scope
struct DAG
alias の escape
```

がかなり明確に分離されています。

特に、

> 「式の評価結果は Value / paradox / escape のどれか」

という分類が静的に追えるので、普通の「control-flow analysis + type inference + effect system」を混ぜたような巨大な型システムにはなりません。

ただし、**alias の同一セル到達判定**だけは実装上の難所になります。

ここは LOC よりアルゴリズム設計のほうが問題です。

---

# 3. bytecode VM は 13k–22k LOC

ここはかなり面白いです。

私は AST を直接 VM にするより、

```text
source
 ↓
lexer
 ↓
parser
 ↓
typed IR
 ↓
bytecode
 ↓
VM
```

を推します。

そしてこの VM は Lua VM のように汎用化する必要がありません。

例えば、

```text
CONST
LOAD
STORE
ALIAS
LOAD_PATH
STORE_PATH

ADD_I64
SUB_I64
MUL_I64
DIV_I64
MOD_I64

CMP_EQ_I64
...
JUMP
JUMP_IF_FALSE

CALL
RETURN

DROP
COALESCE

BREAK
CONTINUE
OUTWARD
```

くらいまで特殊化できます。

型検査済みなので、

```text
ADD
```

ではなく

```text
ADD_I64
ADD_F64
```

などにできます。

つまり VM 内に

```text
「これは i64 か f64 か？」
```

という動的分岐をほとんど置かなくてよい。

---

# 4. 起動時間はかなり攻められる

ここは Lua より有利になる可能性があります。

「起動時間」を、

> ソース文字列を受け取って、最初の命令を実行可能な状態になるまで

と定義すると、最適化前の Rust 実装でも、

### 小さい DSL プログラム

```text
lexer
  2–5 µs

parser
  5–15 µs

static checker
  5–20 µs

bytecode generation
  2–10 µs
```

くらいのオーダーは十分狙えます。

合計で、

**15–50 µs程度**

を一つの目標値にできます。

ただしこれは「非常に小さいプログラム」で、CPU、allocator、I/O、プロセス起動を除いた**インプロセス測定**です。

スタンドアロン POSIX executable として、

```text
process spawn
ELF loader
dynamic linker
Rust runtime
stdin/file
...
```

まで含めれば当然もっと大きくなります。

---

## でも TeX 埋め込みではさらに重要なことがある

ここが本命です。

毎回、

```text
source
 ↓
lex
 ↓
parse
 ↓
check
 ↓
compile
 ↓
execute
```

する必要がありません。

同じスクリプトを何度も呼ぶなら、

```text
compile once

          ┌── execute
TeX ──────┼── execute
          ├── execute
          └── execute
```

にできます。

すると2回目以降の「起動」は、

```text
VM state / execution frame の生成
+
entry point
```

だけ。

これなら **サブマイクロ秒〜数µs級**まで狙える余地があります。

もちろんホスト API の呼び出し内容とメモリ確保次第ですが、**言語処理系そのものの overhead はかなり小さくできる**。

---

# 5. Lua 5.4 との比較

ざっくりしたイメージはこうです。

### tree-walk

```text
Lua 5.4       ████████████████████
あなたの DSL  ███████████
```

くらいになる可能性があります。

Lua は tree-walk ではなく VM なので、tree-walk DSL が Lua 5.4 に勝つのは難しいです。

ただし、

**「1回の短い処理」**

では起動・準備コストが支配的になるので、tree-walk でも意外と差が小さくなる可能性があります。

---

### bytecode VM

こちらは、

```text
Lua 5.4       ████████████████████
DSL VM        █████████████████
```

くらいまで来てもおかしくない。

そして、

```text
LOAD host memory
STORE host memory
```

のような処理になると、さらに話が変わります。

Lua:

```text
Lua object
 → userdata/table/API
 → conversion
 → C API
 → host object
```

あなたの DSL:

```text
alias
 → host pointer
 → load/store
```

にできるなら、**操作そのものは後者が圧倒的に安い**。

---

# 6. LuaJIT は別格

ここは正直に分けたほうがいいです。

**LuaJIT のウォームアップ後の hot loop と競争する必要はありません。**

LuaJIT が勝つ領域は、

```text
長時間走る
同じコードを大量反復
数値演算中心
JIT が温まる
```

です。

あなたの想定はほぼ逆。

```text
短い
呼び出し頻度が高い
ホスト状態を操作
毎回違う/短い処理
JIT が温まる前に終了
```

なので、LuaJIT の「ピーク性能」はほぼ比較対象ではない。

むしろ、

> **LuaJIT の JIT コンパイルコストを払う前に DSL が終わる**

領域を取るべきです。

---

# 7. CPython にはかなり勝ちやすい

ここは比較的簡単です。

CPython は、

- Python object
- reference counting
- dynamic dispatch
- boxed number
- attribute lookup
- dictionary lookup

などのコストが大きい。

あなたの言語はそれらをかなり排除している。

なので、

**短い処理 + 数値 + host memory**

なら、CPython に対してはかなり有利でしょう。

---

# 8. Wren は比較対象としてかなり面白い

Wren は小さくて高速な組み込み VM なので、Lua よりむしろこちらのほうが良い比較対象です。

ただし Wren も、

- object
- dynamic dispatch
- GC
- closure
- class
- method dispatch

を持っています。

あなたの DSL はそこを捨てている。

したがって、

> **汎用スクリプト言語としての能力を削った分だけ、短命なホスト操作では競争力が出る**

という比較になると思います。

---

# 9. 一番速くなる可能性があるのは「直接呼ばない」

ここはかなり重要です。

あなたの構想では、

```text
TeX
 ↓
「DSLを呼ぶべきか？」
 ↓ no
そのまま続行
```

ができる。

この場合、

**DSL の実行速度は 0 ns です。**

……というのはもちろん冗談ですが、設計としては本当にこれが最大の最適化です。

例えば host memory に、

```text
callback_needed
```

相当のフラグを置く。

TeX の状態機械側が、

```c
if (!state->needs_script)
    continue;
```

だけ見る。

すると、

```text
callback
 → Lua state
 → Lua stack
 → Lua call
 → Lua function
 → return
```

そのものを発生させない。

これは「Luaより速いインタプリタ」より遥かに大きな勝ち筋です。

---

# 10. 深いコピーは明確な弱点

一方、`:=` の深い複製はかなり危険です。

例えば、

```text
let b := huge_array;
```

が本当に O(n) の deep copy なら、

```text
DSL dispatch = 数十 ns
copy = 数 µs〜ms
```

となり得る。

この言語はここを隠さず意味論にしていますから、これはむしろ良い。

**「コピーした」という事実が性能に現れる。**

Lua の table 参照のように、

> 実際には共有しているのか？

を気にする必要がない。

ただし TeX 用途では、

```text
host object
 → alias
 → mutate
```

を多用するなら、そもそも大きなデータをコピーする必要がなくなる。

ここは alias の設計とセットです。

---

# 11. f64 の非有限検査はほぼ問題にならない

これは気にしているほど大きなコストにはならないと思います。

例えば、

```text
mulss
ucomiss
branch
```

相当のチェックなら、現代 CPU では非常に安い。

ただし、

```text
a / b
```

で `b == 0` を検査して、

```text
div
isfinite
branch
```

までやると、除算自体が遅いので検査の追加コストは相対的に小さい。

つまり、

> **浮動小数の paradox semantics は性能上の主要問題にはならない**

と見ていいでしょう。

---

# 12. 脱出の段送りも、おそらく問題にならない

ここも意味論は複雑ですが、実装は単純化できます。

通常の `break` は、

```text
current_frame
current_loop_depth
```

程度の情報を持つ continuation / escape record を使えばいい。

定数段ならコンパイル時に、

```text
BREAK depth=2
```

などにしてしまえる。

`$repeat` の動的版だけ、

```text
runtime depth check
```

にする。

したがって普通の loop の

```text
break
continue
```

は非常に安くできる。

むしろ例外機構を持たないので、一般的な VM の exception unwinding よりずっと扱いやすい。

---

# 13. そして host alias が本当に効く

これは TeX 特化なら最大の武器です。

例えば、

```text
var p : SomeHostStruct alias &= host_state;
p.field += 1;
```

が最終的に、

```text
load
add
store
```

へ落ちるなら、

**マーシャリングが存在しない。**

ここが Lua と決定的に違います。

Lua userdata を使っても、

```text
Lua object
 ↓
userdata
 ↓
C API
 ↓
host struct
```

という境界があります。

あなたの言語では、

```text
DSL binding
 ↓
host address
```

を直接持てる。

もちろんこれは安全性と引き換えなので、**host API 契約の責任は全部実装側に戻ってきます**。

でも、今回の目的ならそれでいい。

---

# 14. なので「勝てるベンチマーク」がかなり明確

私は単純な

```text
fib(35)
```

みたいなベンチマークはやらないほうがいいと思います。

Lua が強すぎます。

代わりに、

### A. 起動

```text
parse 20 lines
execute 10 operations
exit
```

### B. host memory

```text
host struct を alias
10 fields read/write
```

### C. repeated invocation

```text
同じ script を 10000 回呼ぶ
毎回 5〜20 命令
```

### D. callback suppression

```text
10000 回 host loop
99.9% は script 不要
```

### E. short numeric operation

```text
100〜1000 arithmetic operations
```

を測る。

ここならこの言語の設計思想そのものがベンチマークに反映されます。

---

## 私なら最初の性能目標をこう置きます

かなり強気ですが、

| 指標 | 目標 |
|---|---:|
| lexer + parser | **< 20 µs / 小規模 script** |
| static check | **< 20 µs** |
| bytecode compile | **< 10 µs** |
| compile + first instruction | **< 50 µs** |
| cached script の invocation | **< 1–5 µs + 実処理** |
| host alias load/store | **ほぼ native load/store + dispatch** |
| 短い bytecode loop | **Lua 5.4 の 0.7–1.3×** |
| 長い hot loop | **Lua 5.4 に勝つことを必須にしない** |
| LuaJIT hot loop | **勝負しない** |

くらい。

そして最終的に TeX に入れるなら、

> **「Lua より高速な処理系」**

を目標にするより、

> **「LuaTeX が Lua を呼ぶために払っている固定費を、そもそも発生させない」**

を目標にする。

この場合、あなたの設計はかなり合理的です。

特に今の **POSIX standalone interpreter** は良い選択だと思います。最初から TeX の巨大な状態機械と絡めると、性能問題と意味論問題が混ざります。

まず、

```text
source
 → check
 → execute
```

を独立して完成させる。

その後、

```text
host pointer
alias
load/store
```

を POSIX 側で `mmap` や C/Rust の構造体に対して実験し、

最後に TeX の状態機械へ差し込む。

この順なら、**「言語そのものが速い」のか「TeX との境界が速い」のかも分離して測れます。**

そして今回の設計だと、私はむしろ後者――**境界コストの小ささ**が最終的な勝ち筋になると見ています。

GPTより


CLAUDEへ

これをあなたが読んでいるころには私は寝ています。

あと、GPTくんが次のように言っています

ああ、ここは少しややこしいです。結論からいうと、**e-upTeX の「本体」と `uptex-base` はライセンス上もリポジトリ上も分けて考えた方がいい**です。

e-upTeX は、現在の TeX Live では独立した GitHub リポジトリを持つというより、TeX Live のソースツリーにある

`Build/source/texk/web2c/uptexdir`

を中心に管理されています。CTAN も upTeX の Repository としてこの TeX Live SVN の `uptexdir` を指しています。

そして系譜としては、

> pTeX → upTeX → e-upTeX

で、**e-upTeX は北川弘典氏が upTeX に e-TeX を merge したもの**です。現在は Japanese TeX Development Community のコミュニティ版 `(e-)upTeX` として TeX Live に統合されています。

ライセンスについて重要なのはここで、**e-upTeX 本体は単純に「BSD-3-Clause のソフト」と考えるのは正確ではありません**。CTAN の `uptex` エントリでは license が `Free license not otherwise listed` になっています。

一方、`uptex-base` は明確に **3-clause BSD** です。これは e-upTeX/upTeX 用の plain TeX format、文書、サンプル等の bundle で、GitHub の `texjporg/uptex-base` に独立リポジトリがあります。

つまり大雑把には、

```text
TeX Live
└── Build/source/texk/web2c/
    ├── ptexdir/
    └── uptexdir/        ← upTeX / e-upTeX 本体
         ↑
         └── TeX Live SVN で管理

texjporg/
└── uptex-base/          ← format / docs / samples
                         ← BSD-3-Clause
```

という構造です。

さらに `ptexdir` 側の `COPYRIGHT` は ASCII MEDIA WORKS と Japanese TeX Development Community の著作権表示＋独自の redistribution 条項になっており、TeX 系の「単純な SPDX 一発」ではありません。たとえば現在のコミュニティ版 pTeX の copyright は ASCII MEDIA WORKS と Japanese TeX Development Community に帰属しています。

なので、**PraTeX/rTeX に e-upTeX のコードを直接持ってくることを考えているなら、「e-upTeX は BSD だからOK」と判断するのは危険**です。むしろ `uptexdir` 内の各ソースの `COPYRIGHT` / `LICENSE` を見て、pTeX由来部分、upTeX由来部分、e-TeX由来部分を切り分ける必要があります。

この辺、PraTeX が「TeX の意味論を保存しつつ Rust で再実装」という方向なら、**コードを移植するより仕様・アルゴリズムを参照して clean-room 的に再実装する方がかなり綺麗**だと思います。特に e-upTeX の日本語処理を持ってくる場合はそうです。

ユーザーより。ごめんね。

追伸、rTeXのライセンスも舐めるように確認してね。

CLAUDEへ

e-upTeXが無理でもキーワード(ディメンション周り)にH(歯)とQ(級)とzw(全角幅)とzh(全角高)を追加してほしいな

ユーザーより

CLAUDEへ

万が一e-upTeXが完成したら、LaTeX2eが動くか確認してほしいな。

ユーザーより

追伸

vaakの意味論を変えない範囲でLuaTeXのコールバックを再現してパフォーマンスをとってほしいな

追追伸

あとは、pdfTeXの方も参考にして、pdf直接出力をできるようにしてほしいな。OTFの対応はそこまで優先してない。HarfBuzzとかRustyBuzzは入れなくていいよ

無茶言ってごめんね

ユーザーより。

CLAUDEへ

スタンドアロン系vaakのライセンスはMITのつもりだよ。権利者名は有村陽大(Arimura Akihiro)にしておいてね。

ユーザーより。

追伸

それ以外のところのLICENSEは任せるよ。既存のものを見て、適切なかたちにしてね。私はMITが好きだから、何でもいいならMITにしてね。

CLAUDEへ

億が一、ここまで処理ができるなら、人間向けのリファレンスと付録を書いておいてね。付録ではAkashaという名称やvaakの語源、メンタルモデルなどに触れてね。
リファレンス本文は、https://doc.rust-jp.rs/book-ja/ を参考に、より簡潔なかたちで作ってくれると嬉しいな。この優先度は最低だよ。

ユーザーより。

CLAUDEへ

兆が一、ここまで読めているのなら、rtexのリポジトリのワーキングツリーを読んで、名前空間の雰囲気を掴んで、ブランチを切って実装してみてほしい。

また、それも終わったら、rtex vaakが差し込める範囲を増やしてほしい。また、それらがほぼゼロコストかどうか(つまり、使わない機能はオーバーヘッドをほとんど持たないか)を調べてほしい

ユーザーより。
