# タグ付き値：`sum` / `match` を足さずにどこまで書けるか

この実験は、意図的に仕様から外している `sum` 型と `match` 構文を提案し直すものではない。
既存の `struct`、整数 tag、`switch`、関数だけで同じ計算を四通りに配置し、木を辿る
参照実装と VM で意味と費用を測った。

実物は [`examples/experiments/tagged_*.vaak`](../../examples/experiments/)、測定器は
[`examples/bench_tagged.rs`](../../examples/bench_tagged.rs)、意味の照合は
[`tests/tagged_value.rs`](../../tests/tagged_value.rs) にある。core と決定書は変更していない。

## 擬似的な再現

たとえば整数節と二項演算節を持つ AST は、次のように置ける。

```vaak
let NODE_INT := 0;
let NODE_ADD := 1;
let NODE_MUL := 2;

struct Node {
    let kind : i64 := NODE_INT;
    let value : i64 := 0;
    let left : i64 := 0 - 1;
    let right : i64 := 0 - 1;
};

fn eval_flat (node : Node, left : i64, right : i64) {
    (switch (node.kind)
        case NODE_INT => node.value
        case NODE_ADD => left + right
        case NODE_MUL => left * right)
    ?? $return
} -> i64;
```

`switch` は主題を一度だけ評価し、腕を上から順に比べ、当たった腕だけを評価する（C-4）。
どの腕にも当たらなければ paradox なので、`??` で未知 tag の処理を明示できる（C-26）。
再帰する値型は要らず、`Node array` と整数の子 ID で木・DAGを表せる。

一方、これは閉じた直和型と同じ安全性を持たない。

| 欲しい性質 | tag + `switch` |
|---|---|
| variant の選択 | `kind` の整数で表せる |
| payload | `struct` の欄で表せる |
| 遅延分岐 | `switch` が持つ |
| 未知 variant | unmatched paradox と `??` で処理できる |
| tag と payload の整合 | 型では保証しない。constructor 関数の規約で守る |
| 網羅性 | 静的には保証しない |
| payload の分解 | 腕の中で欄を明示的に読む |

実用上は tag 値を一箇所に集め、variant ごとの constructor 関数を置き、dispatch も一関数へ
集めるとよい。無効状態を「構築不能」にはできないが、無効 tag の扱いと使わない欄を一箇所へ
閉じ込められる。標準ライブラリや生成道具で支援できる範囲であり、新構文は要らない。

## 四つの配置と書き味

同じ四腕の計算を 5,000 要素について 8 回、合計 40,000 dispatch した。

1. **inline switch** — tag と payload をその場で計算し、その場で分岐する。
2. **named function** — tag と payload を引数で受ける一関数へ dispatch を集める。
3. **parallel arrays** — tag、left、right を三本の配列へ分ける。
4. **struct array** — `Tagged array` へ完成した値を `push` し、`value.kind` で分岐する。

inline は短いが各利用箇所に腕が散る。関数版は不変条件と未知 tag の処理を集約できる代わりに、
インタプリタでは関数呼び出しの定数費用が見える。平行配列は走査が軽いが、三本の長さと添字を
常に同期させる必要がある。構造体配列は arena AST に最も近く、節一個が自己完結する代わりに、
要素と欄を辿る費用がある。

構造体配列は最初から完成形を `push(new Tagged(...))` した。append-only arena の通常の構築を
測るためである。`values[i].field := ...` の現在の VM 固有の二乗経路は、下で分離して測った。

## 測定

2026-08-22、Windows x86-64、release build。`cargo run --release --example bench_tagged --
5000 8 7` で、解析・静的検査・VM 翻訳を測定外にし、warm-up 後 7 回の中央値を取った。
各 run は配列構築も含む。

| 配置 | 参照実装 | VM | VM / 参照 |
|---|---:|---:|---:|
| inline switch | 56.378 ms | 36.879 ms | 0.654 |
| named function | 122.890 ms | 56.191 ms | 0.457 |
| parallel arrays | **46.216 ms** | **29.753 ms** | 0.644 |
| struct array | 93.749 ms | 78.499 ms | 0.837 |

全方式の checksum は 99,979,952 で一致した。

named function と inline の全体差を 40,000 dispatch で割ると、参照実装で約 1.66 us、VM で
約 0.48 us/回である。引数の算術は同じなので、主に Vaak の関数 frame の費用が表れている。
高頻度の interpreter hot loop では `switch` を inline に置く価値がある。一方、通常の compiler
pass では invariants を一関数へ集める読みやすさとの交換になる。

parallel arrays が inline より速いのは、tag と payload を構築時に一度だけ計算し、8 回の走査で
再利用するからでもある。struct array は parallel arrays に対して参照実装で約 2.03 倍、VM で
約 2.64 倍だった。これは tag 表現が不可能な差ではなく、自己完結した節と平行配列の locality・
要素 clone の交換である。AST の正規表現としては struct array、特定 pass の hot columns には
平行配列という使い分けができる。

現在の三実行器はいずれも腕を上から順に比較する。腕数 `K` に対して dispatch は最悪 O(K) である。
tag が少数なら十分実用的だが、variant が非常に多い処理系では費用の上限になる。意味論を変えずに
できる将来の最適化は、pattern が副作用のない整数定数だけの `switch` を VM の jump table または
比較木へ落とすことである。任意の pattern 式は評価順が観測できるため、同じ最適化をしてはいけない。

## 10,000 要素が 60 秒を越えた本当の理由

最初の struct-array 版は、10,000 個を既定値で作ってから三つの欄を個別に書いていた。

```vaak
values[i].tag := i mod 4;
values[i].left := i;
values[i].right := i mod 7;
```

遅さは tag + `switch` や構造体配列そのものではなく、**VM の入れ子左辺の書き戻し**だった。

参照実装の `resolve_place` は root cell と `[Index(i), Field(name)]` の経路を作り、`step_set` が
root を借りたまま目的の欄だけを変更する。型を辿る途中で選んだ一要素は clone するが、配列全体は
clone しない。したがって一欄の書き換えは配列長 N に比例しない。

VM compiler は直接の `a[i] := v` には `StoreIndex` を出す fast path を持つ。しかし
`a[i].field := v` は `SetField` の後に `store_back(Index)` へ進み、root の `a` を通常の
`Op::Load` で積み直す。`Op::Load` は `Value::clone()` を呼び、`Value::Array(Box<ArrayVal>)` の
derive `Clone` は `Vec<Value>` 全体を深く写す。その配列へ一要素を書き戻すので、一欄 O(N)、
全要素で O(N²) になる。

一欄だけを書き換える縮小例と、直接の scalar 添字書き込みを比較した。各 N について配列構築と
N 回の書き換えを一度行い、同じ測定器で中央値を取った。

| N | 参照 scalar | 参照 nested | VM scalar | VM nested |
|---:|---:|---:|---:|---:|
| 100 | 0.068 ms | 0.113 ms | 0.026 ms | 2.490 ms |
| 200 | 0.099 ms | 0.224 ms | 0.044 ms | 9.764 ms |
| 400 | 0.191 ms | 0.414 ms | 0.082 ms | 41.136 ms |
| 800 | 0.267 ms | 0.848 ms | 0.124 ms | 133.214 ms |

参照 nested は倍加ごとにほぼ 2 倍、VM nested は概ね 4 倍である。コード上の経路とスケールの
両方が、root array の深い複製による O(N²) を支持する。最初の測定器は 10,000 要素・三欄の
初期化を、tree は 21 run、VM は 70 run 繰り返していたため、この診断経路を極端に増幅していた。

これは実装済みの `a[i].field := v` に残る既存性能穴である。直すなら arena 専用機能より先に、
VM へ root slot と経路を保持する一般的な load/store-place、または少なくとも名前 root の
`StorePath` を入れ、参照実装の `Place` と同じ一箇所の意味へ揃えるのが筋である。

## `wrap` と一級 arena への含意

Claude の `wrap NodeId = i64` 対照実験は、`NodeId` と `TokenId` の取り違えを静的に止め、
STEEL native では包む前と同じ機械語になった。これは強い結果であり、**番号に名前を付けるだけ**
なら、まず `wrap` の比較・添字の意味を決める案が小さい。

ただし arena 提案の動機は、取り違え防止だけではなかった。

1. payload の型、handle の型、所有する storage を一つの契約にする。
2. append と「今追加した slot の handle」を一操作にし、off-by-one と途中の不変状態を消す。
3. handle を任意の整数から構築不能にするなら、`new NodeId(i)` で検査を迂回できない。
4. backend が `arena[h].field` を root 全体の値渡しでなく、直接 place として下げられる。
5. handle を値型依存グラフの leaf として扱い、再帰 AST/DAG の標準的な表現を示せる。

`wrap` でも 1 の「NodeId と TokenId」は取れるが、arena instance と handle の組までは結ばない。
同じ `Ast` 型の二 instance 間の取り違えは、一級 arena の初版案でも残る。instance identity を
入れない限り、ここは `arena + root` を一構造体で運ぶ規約が要る。

### Claude からの四つの質問への回答

1. **主動機は取り違えだけではない。** 本体は storage/handle/payload の契約、atomic alloc、
   opaque handle、direct-place lowering である。`wrap` の実験により、型名だけなら既存機能で
   かなり取れることが分かったため、arena を入れる根拠はこの残差で判断すべきである。
2. **書き換えは `ast[h].kind := 5` の既存 lvalue 形が自然である。** 範囲外は現在の添字代入と
   同じ誤りにする。ただし VM は上で実測した二乗経路を直さなければならない。一級 arena を単に
   array の糖衣へ落とすだけでは性能上の解決にならない。
3. **LISP と Forth で異種 ID を実際に取り違えた記録はない。** Vaak LISP は flat token stream、
   Forth は token 区間と平行辞書配列で、NodeId arena をまだ使っていない。セルフホスト骨格にも
   accidental な取り違えは記録されていない。Claude が意図的に TokenId を NodeId として渡した
   対照例は、裸 i64 では誤答 0 のまま完走し、wrap では型エラーになった。したがって現時点の利得は
   「既発 bug の修正」より「将来の compiler 規模で bug を構築不能にする」が正確である。
4. **`.alloc` の行数削減は小例では小さい。** 272 行の式 parser で node allocation site は二つだけ。
   現在の `nodes.push(node); NodeId(nodes.len() - 1)` を一式へ畳めるので数行を減らす程度であり、
   計算量も償却 O(1) のまま変わらない。価値は速度より、push と handle 発行を不可分にし、opaque
   handle の唯一の生成元にできることにある。実 compiler で node constructor が増えるほど効く。

## 現時点の判断

- `sum` / `match` を戻さなくても、小から中規模の tagged AST/IR は書ける。
- interpreter hot path では dispatch helper の関数呼び出しが見える。必要な pass だけ inline にする。
- canonical storage は struct arena、列指向の hot pass は parallel arrays という選択肢がある。
- 一級 arena の要否を tag boilerplate と結び付けない。tag は既存の `switch` と library 規約で扱う。
- arena を決める前にも、VM の `a[i].field` 深い複製は既存機能の穴として直す価値が高い。
- `wrap` の比較・添字を先に定めて実例を増やし、それでも残る storage/handle coupling と opaque
  allocation が一級 arena の判断材料になる。

## 再現

```bash
cargo test --release --test tagged_value
cargo run --release --example bench_tagged -- 5000 8 7
```
