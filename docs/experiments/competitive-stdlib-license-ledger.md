# 競プロstdlib clean-room / license ledger

> **状態：実験checkpointの参照記録。** この文書は外部APIをVaakの確定仕様へ昇格させず、
> 外部実装をvendorする許可にもならない。

- 調査日: 2026-08-24〜2026-08-25
- 対象branch: `codex3/stdlib-dense-bitset`（`1bb004b`の統合checkpoint上へpure固定長集合を追加）
- Vaak本体のlicense: [`docs/LICENSING.md`](../LICENSING.md)のとおりMIT
- 規律: 公式仕様から契約・分類・計算量だけを調べ、source、test、解説文を転写・翻訳しない

## 参照記録

| ID | 公式資料・固定点 | license | 参照した範囲 | 参照しなかった範囲 / Vaak側の独立部分 |
|---|---|---|---|---|
| ACL-1 | [AC Library公式repository](https://github.com/atcoder/ac-library)、[v1.6 release](https://github.com/atcoder/ac-library/releases/tag/v1.6)、release commit [`864245a`](https://github.com/atcoder/ac-library/tree/864245a)、[固定点のLICENSE](https://github.com/atcoder/ac-library/blob/864245a/LICENSE) | CC0-1.0 | 公式libraryの範囲、配布単位、license | C++ header、internal、testは入力資料にせず、codeを転写していない |
| ACL-DSU-1 | [公式DSU documentation](https://atcoder.github.io/ac-library/production/document_en/dsu.html)（v1.6を固定点として参照） | ACL-1と同じ | `merge` / `same` / `leader` / `size` / `groups`という操作分類、union-findの償却O(alpha(n))、構築O(n) | Vaakの配列表現、paradox、`group_count`、決定的tie、Rust oracleは独立。rollback / potential付きAPIの資料とは扱わない |
| ACL-FENWICK-1 | [公式Fenwick Tree documentation](https://atcoder.github.io/ac-library/production/document_en/fenwicktree.html)（v1.6を固定点として参照） | ACL-1と同じ | point add、半開区間sum、構築O(n)、操作O(log n)、整数和が型幅で折り返す契約 | Vaakの実装、paradox、flat性能対照、非負count invariant、順位選択は独立。ACLに`lower_bound` APIがあるとは記録しない |

公式documentationは一部のconstraint違反を未定義としている。Vaakはそれを継承せず、負長、範囲外、
不整合constraint、非連結difference、非負count違反を各sourceの公開契約でparadoxへ写す。

## 第3 checkpointで独立に定義したもの

| Vaak source | 外部実装の入力 | 独立性を固定する試験 |
|---|---|---|
| `rollback_dsu_i64.vaak` | なし。ACL-DSU-1を通常DSUの分類・計算量比較にだけ使用 | successful mergeだけを保存するsnapshot列を、RustのDSU state複製oracleと比較 |
| `weighted_dsu_i64.vaak` | なし | i64折返し加法群のconstraint graphをRustで別に持ち、DFSでpotential差と矛盾を判定 |
| `fenwick_count_i64.vaak` | なし。ACL-FENWICK-1を通常Fenwickとの分類比較にだけ使用 | 非負`Vec<i64>`を線形走査し、prefix/range/point/k-thを比較 |

三つとも外部snippetを検索・採用せず、Vaakの既存array、alias、flow、i64算術だけから実装した。
Rust側の`Vec`/graph/snapshot modelはtest oracleに限定し、配布APIやVaak sourceへ転写していない。

## fixed i64 ordering checkpointで独立に定義したもの

| Vaak source | 外部実装の入力 | 独立性を固定する試験 |
|---|---|---|
| `array/i64/sort/insertion.vaak` | なし | 固定seed列をRust `Vec::sort`の結果と全要素比較 |
| `array/i64/sort/heap.vaak` | なし | 同じ列と半開区間を三backendで比較し、O(1)作業領域のmax heapを独立実装 |
| `array/i64/sort/merge.vaak` | なし | 同じ列をRust oracleと比較し、同値時に左列を選ぶstable契約を固定 |
| `array/i64/partition/sorted_unique.vaak` | なし | 昇順違反時の原子性、空、重複、i64両端を独立fixtureで比較 |
| `array/i64/compress.vaak` | なし | Rust `Vec::sort` / `dedup` / `binary_search`だけをoracleに使い、unique値と全rankを比較 |

2026-08-25のordering実装では外部repository、競プロblog、snippet、生成済みcodeを入力にしていない。
Rust標準ライブラリの結果は`tests/array_ordering.rs`内のoracleに限り、Vaak側のalgorithm、API名、文書へ
転写していない。既存のACL参照記録も今回のsort / unique / coordinate compression契約の由来とは扱わない。

## static sparse table checkpointで独立に定義したもの

| Vaak source | 外部実装の入力 | 独立性を固定する試験 |
|---|---|---|
| `ds/sparse_table_i64.vaak` | なし | 固定seedの半開区間をRust sliceの`min` / `max`へ照合 |
| `ds/disjoint_sparse_table_i64.vaak` | なし | 同じ区間をRust `i64::wrapping_add` foldへ照合 |

min/maxの重なる2冪区間と、sumの中央suffix/prefixは既知の分類から独立に記述し、外部repository、解説code、
test vectorを入力にしていない。Rust側は結果oracleだけで、Vaakのflat layout、API、empty/paradox、
capacity overflow、公開欄検査はこのrepository内の既存stdlib規律から定めた。ACLにsparse table APIがあるとは
記録せず、ACL-1のsource/testも参照していない。

## compressed ordered multiset checkpointで独立に定義したもの

| Vaak source | 外部実装の入力 | 独立性を固定する試験 |
|---|---|---|
| `ds/ordered_multiset_i64.vaak` | なし。ACL-FENWICK-1を通常Fenwickの分類・計算量比較にだけ使用 | sorted uniqueなRust `Vec<i64>`と非負count列をoracleにし、280操作のcount/rank/k-thを照合 |

外部repository、ordered multiset実装、競プロblog、snippet、test vectorは入力にしていない。Vaak側の固定universe、
未知keyの照会とmutationの区別、空eraseのstatus、0-based k-th、overflow時の原子性は独立に定義した。
Rust vector modelは`tests/ordered_multiset_i64.rs`の結果oracleだけに使い、Vaak sourceへ転写していない。
`FenwickCountI64`とは公開型を合成せず、同じ非負count不変条件を単独source内のflat fieldへ独立に適用した。

## range-update Fenwick checkpointで独立に定義したもの

| Vaak source | 外部実装の入力 | 独立性を固定する試験 |
|---|---|---|
| `ds/fenwick_range_i64.vaak` | なし。ACL-FENWICK-1をpoint Fenwickの分類・計算量比較にだけ使用 | Rust `Vec<i64>`へ半開区間加算を直接適用し、260操作のpoint/prefix/range結果を照合 |

外部repository、range-update Fenwick実装、解説code、snippet、test vectorは入力にしていない。一配列のdifference
表現と二配列のindex係数式は、Vaakの既存i64折返し演算上で独立に導出した。Rust vector modelは
`tests/fenwick_range_i64.rs`の結果oracleだけに使い、Vaak sourceへ転写していない。empty/paradox、公開欄検査、
range違反時の更新前拒否はこのrepositoryの既存stdlib契約へ揃えた。

## bulk ASCII i64 checkpointで独立に定義したもの

| Vaak source | 外部実装の入力 | 独立性を固定する試験 |
|---|---|---|
| `io/ascii_i64.vaak`の`read_n_into` / `format_range` | なし | Rust `i64::to_string`で作った192値の入力・整形結果を、独立wrapping hashと長さで照合 |

外部scanner/formatter、競プロtemplate、snippet、test vectorは入力にしていない。既存single-token契約のASCII空白、
符号、i64境界をbulk loopにも適用し、random oracleと固定fixtureで両APIを照合した。成功prefixだけを確定する
失敗契約、caller-owned配列範囲、20-byte scratch一回、ASCII separator、値間だけの区切りは独立に定めた。
stdin/stdout、host名、streaming、reserve、zero-copy viewの資料・実装はこのcheckpointへ持ち込んでいない。

## UTF-8 JSON / JSON Lines checkpointで独立に定義したもの

| Vaak source | 外部仕様・要求の入力 | 読まない範囲 / 独立性を固定する試験 |
|---|---|---|
| `codec/json_utf8.vaak` | [RFC 8259](https://www.rfc-editor.org/rfc/rfc8259)のJSON token、escape、UTF-8相互運用要件と、既存MIT `string.vaak`のRFC 3629妥当性契約 | 外部parser/serializer source、test vector、snippetを使わない。flat node/edge表、i64限定number、stable error/budget code、duplicate拒否、順序保存を独立fixtureでreference/VMへ照合 |
| `codec/jsonl_utf8.vaak` | PraTeX担当者が`rtex/docs/validation/AUTOCHECKDATABASE/to-vaak/20260825-010859-utf8-jsonl-standard-library-request.md`へ置いた受入要件だけ | GPL-3.0のrtex source、codec実装、schemaを一行も読まず転写しない。LF/CRLF/終端LFなし、chunk分割、record位置、上限、原子性を独立fixtureで照合 |

RFC本文の文言や例を文書・試験へ転写せず、wire grammarの必要条件だけを実装した。PraTeX側から運んだのも
invalid UTF-8/escape/surrogate/trailing、位置、budget、決定的出力、chunk分割、codec/capability分離という
設計要求だけである。PraTeX固有build manifest schema、file I/O、GPL sourceはVaakへ持ち込んでいない。

JSON numberは外部libraryの表現を採らず、現行Vaakが正確に持つi64だけを初版値域にした。小数・指数を
unsupported codeへ分け、f64丸めやdecimal文字列表現を新しい一般則にしない。Rust側の試験は期待byte列と
error位置を直接構成するだけで、別JSON実装の出力をVaak sourceへ転写していない。

JSONLの毎record suffix copy案とbuffer-head案、毎chunk再探索案とscan cursor案は同じrepository内benchmarkで
比較し、suffix copyと再探索を棄却した。測定値は`stdlib/BENCHMARK.md`へ残し、外部実装のbenchmarkや
algorithmを入力にしていない。

## DenseBitSetU32 checkpointで独立に定義したもの

| Vaak source | 外部仕様・要求の入力 | 読まない範囲 / 独立性を固定する試験 |
|---|---|---|
| `ds/dense_bitset_u32.vaak` | TeX埋め込み・LVMINIBVS・競技programに共通して有限集合が必要という分類と、このrepositoryの既存stdlib規律だけ | LuaTeX、rtex、LVMINIBVS、競プロlibraryのsource/test/snippetを使わない。Rust `Vec<bool>`を結果oracleだけに使う240操作と三domain fixtureを三backendへ照合 |

32-bit word、0-based index、末尾padding 0、同一長だけの集合演算、paradox、変更status、API名、実装loopは
Vaakの現行`u32 array`、bit演算、alias、flowから独立に定義した。LuaTeX公式資料からTeX埋め込みに有限な
文字分類が必要というdomain分類を得ても、LuaTeXのGPL-2.0-or-later source、header、test、bitset表現、APIを
閲覧・転写していない。GPL-3.0のrtex repositoryからもcodeをVaakへ運ばず、PraTeX固有schema、catcode規則、
file I/Oはこのsourceへ含めていない。添付されたLVMINIBVS評価から運んだのはsnapshot内の有限maskという
要求だけで、game source、adapter、world identity、Unity/Lua APIは入力にしていない。

Rust `Vec<bool>`は`tests/dense_bitset_u32.rs`内でmembership・集合演算・rank結果を計算するoracleに限定し、
内部表現やalgorithmをVaak sourceへ転写しない。TeX fixtureのASCII code、LVMINIBVS fixtureの架空の12候補、
競プロfixtureの70整数はこのcheckpoint用に独立構成したtest dataである。u8-per-bit、flat u32、named u32の
比較もrepository内の同一workloadだけで測り、外部benchmark結果を入力にしていない。

## 今後の追記規則

各checkpointで、実装前に次を一行へ固定する。

1. 公式URL、release/tag/commit、取得日、license。
2. 参照する公開契約、constraint、計算量、分類。
3. 読まない・転写しないsource/testと、独立oracle。
4. Vaakが異なるempty/paradox/overflow契約を選ぶ場合の差。

license不明の競プロblog、回答snippet、生成済みcodeを実装入力にしない。CC0等の利用可能な資料でも、
codeを利用したなら「仕様だけを参照」と書き換えず、由来と利用範囲を別途明記する。
