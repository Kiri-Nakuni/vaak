# 競プロstdlib clean-room / license ledger

> **状態：実験checkpointの参照記録。** この文書は外部APIをVaakの確定仕様へ昇格させず、
> 外部実装をvendorする許可にもならない。

- 調査日: 2026-08-24〜2026-08-25
- 対象branch: `codex3/stdlib-sparse-table`（ordering / structure / graph checkpointを統合）
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

## 今後の追記規則

各checkpointで、実装前に次を一行へ固定する。

1. 公式URL、release/tag/commit、取得日、license。
2. 参照する公開契約、constraint、計算量、分類。
3. 読まない・転写しないsource/testと、独立oracle。
4. Vaakが異なるempty/paradox/overflow契約を選ぶ場合の差。

license不明の競プロblog、回答snippet、生成済みcodeを実装入力にしない。CC0等の利用可能な資料でも、
codeを利用したなら「仕様だけを参照」と書き換えず、由来と利用範囲を別途明記する。
