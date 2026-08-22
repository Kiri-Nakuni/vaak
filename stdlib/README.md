# pure Vaak ライブラリ試作

これは未実装のモジュール構文を先取りしない、ソース単位のライブラリ試作である。
必要な `.vaak` だけを利用者ソースの前に置く。各ファイルは単独で前置きでき、
別ファイルの暗黙の読み込みを要求しない。

## 第一段

| ソース | 公開 API | 契約の要点 |
|---|---|---|
| `range/check.vaak` | `range_is_valid`, `range_is_index`, `range_length` | 半開区間。空区間の長さは 0。不正な `range_length` は paradox |
| `array/i64/search_linear.vaak` | `array_i64_find*`, `rfind*`, `contains*`, `count*` | 読み取り alias。探索失敗は paradox、空区間の count は 0 |
| `array/i64/search_binary.vaak` | `array_i64_lower_bound*`, `upper_bound*`, `binary_find*`, `binary_contains*` | 昇順が前提。境界探索は空区間の先頭を返す |
| `array/i64/reverse.vaak` | `array_i64_reverse*` | `var alias` でその場更新。成功は true、不正範囲は paradox |
| `array/i64/prefix_sum.vaak` | `array_i64_prefix_sum`, `array_i64_prefix_range_sum` | 結果は n + 1 要素。空区間の和は 0 |
| `ds/dsu_i64.vaak` | `DsuI64`, `dsu_i64_*` | union by size + path compression。添字範囲外は paradox |
| `ds/fenwick_i64.vaak` | `FenwickI64`, `fenwick_i64_*` | i64 加算に固定。point add と半開区間 sum |
| `ds/fenwick_i64_flat.vaak` | `fenwick_i64_flat_*` | 生の配列一本を受ける性能対照。入れ子の左辺を作らない |

配列を値引数で受けると深い複製になるため、読み取りは `alias`、破壊は
`var alias` に揃えた。subarray を別名にせず `(xs, first, last)` の半開区間を渡す。
総和は Vaak の `i64` と同じく溢れたとき折り返す。

現段階では generic、第一級 callback、sum 型、`match` を要求しない。比較や加算を
hot loop へ直接書く固定演算版を基準にし、将来のモジュール機構ではこのファイル境界を
読み込み・到達可能性・STEEL の特殊化単位として扱えるかを検証する。

`FenwickI64` 版は型名が付く一方、更新の左辺が `tree.data[i]` になる。flat 版は
型による取り違え防止を失う代わりに `data[i]` だけを更新する。この二本は永続 API の
候補を二重化するためではなく、現行 VM の入れ子経路の費用を分離して測る対照実験である。
