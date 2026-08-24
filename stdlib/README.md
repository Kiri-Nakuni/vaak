# Vaak文字列ライブラリ

`string.vaak`は、既存の`str`と配列操作だけで書いた任意選択のライブラリである。
中核の無名標準ライブラリ（S-3）を変更せず、`str_*`自由関数として提供する。

Vaakにはまだmodule/import機構がない。埋め込むホストは
`vaak::stdlib::STRING`を利用者ソースの前へ置いてからparse/check/compileする。

```rust
let app = r#"
    let input := "  Apple,banana,APPLE  ";
    let trimmed := str_trim_ascii(input) ?? "";
    let fields := str_split(trimmed, ",") ?? new str array(0, "");
    str_join(fields, " | ")
"#;

let source = format!("{}\n{}", vaak::stdlib::STRING, app);
let program = vaak::parser::parse(&source)?;
```

動くVaak側の例は[`examples/文字列.vaak`](examples/文字列.vaak)にある。単独の
プログラムではなく、`string.vaak`を前置きして使う利用者ソースである。

## API

入力本体は`alias`で受け、走査前の深い複製を避ける。needle、separator、
replacementは値で受ける。これは小さい文字列を一度複製する代わりに、文字列
リテラルを直接渡せ、`str_find(source, source)`もC-87へ抵触せず書ける設計である。
返される文字列・配列は新しい所有値である。

| 関数 | 結果 |
|---|---|
| `str_eq(source, other)` | バイト列が同じか |
| `str_starts_with(source, needle)` | 前方一致 |
| `str_ends_with(source, needle)` | 後方一致 |
| `str_contains(source, needle)` | 部分列を含むか |
| `str_find(source, needle)` | 最初のバイト位置。無ければparadox |
| `str_find_from(source, needle, from)` | `from`以降の最初の位置 |
| `str_find_byte(source, byte, from)` | `from`以降の一バイト検索 |
| `str_rfind(source, needle)` | 最後の位置。無ければparadox |
| `str_rfind_from(source, needle, from)` | `from`以下の最後の位置 |
| `str_slice(source, from, upto)` | バイト半開区間`[from, upto)` |
| `str_trim_ascii(source)` | 両端のASCII空白を除く |
| `str_split(source, separator)` | 空欄と末尾の空欄を保持して分割 |
| `str_join(parts, separator)` | 文字列配列を連結 |
| `str_replace_all(source, old, replacement)` | 左から右への非重複置換 |
| `str_repeat(source, count)` | `count`回連結 |
| `str_ascii_lowercase(source)` | ASCII大文字だけを小文字化 |
| `str_ascii_uppercase(source)` | ASCII小文字だけを大文字化 |
| `str_eq_ignore_ascii_case(source, other)` | ASCIIだけ大小を無視して比較 |
| `str_utf8_valid(source)` | RFC 3629の妥当性検査 |
| `str_is_utf8_boundary(source, index)` | 妥当なUTF-8の符号位置境界か |
| `str_slice_utf8(source, from, upto)` | UTF-8妥当性と両境界を検査してslice |

## 端の契約

### 空needle

- `starts_with`、`ends_with`、`contains`は`true`。
- `find`は`0`、`find_from`は`from`を返す。
- `rfind`は`source.len()`、`rfind_from`は`from`を返す。
- `split`の空separatorと`replace_all`の空oldは、零幅一致をどのように
  列挙するかを暗黙に決めないためparadox。

### 添字と回数

- 位置はすべてバイト添字。
- `find_from`と`rfind_from`の`from`は`0..=source.len()`だけが有効。
  範囲外と負数はparadox。
- `find_byte`の`from`も同じ範囲だけが有効。見つからなければparadox。
- `rfind_from`の`from`は検索開始位置を含む。needleが空でなければ、末尾を
  越える有効な位置は最後の候補位置へ丸める。
- `slice`は`0 <= from <= upto <= len`だけが有効。それ以外はparadox。
- `repeat`の負回数はparadox。0回と空のsourceは空文字列。非空sourceでは
  `count * source.len()`を`i64`で表せなければ、周回を始める前にparadox。

### UTF-8

`str`の表現と通常APIはバイト単位である。検索・分割・置換・大小変換は
Unicodeの正規化や大小対応を行わない。ASCII大小変換は128以上のバイトへ
触れないため、妥当なUTF-8入力を壊さない。

`str_slice`は指定されたバイトを忠実に複製するため、符号位置の途中を切れば
結果は不正なUTF-8になり得る。この費用と判断を隠さない。妥当性を保つ必要が
あるときは`str_slice_utf8`を使う。この関数は入力全体を検査するためO(n)である。

## 実装上の性質

- 検索は素朴な照合で、最悪O(source × needle)。巨大な検索には将来、前処理済み
  matcherまたは外部WASMを使う余地がある。
- CSV/JSON lexerの区切りには`str_find_byte`を使える。これは一バイトのneedleを
  作らず、候補位置ごとの`str__match_at`関数frameも持たない。
- 出力を作る操作は`.push()`で構築する。現行の幾何増加により償却線形だが、
  `reserve`が加われば既知長の`repeat`や`join`をさらに調整できる。
- `str_utf8_valid`は過長符号化、UTF-16 surrogate範囲、U+10FFFF超過を拒む。
- 第一級callback、sum型、match構文は必要としない。
- 参照実装、VM、STEEL nativeの同一ソース試験は`tests/string_library.rs`にある。
# pure Vaak ライブラリ試作

これは未実装のモジュール構文を先取りしない、ソース単位のライブラリ試作である。
必要な `.vaak` だけを利用者ソースの前に置く。単独sourceはそのまま前置きでき、複合sourceは
冒頭に列挙した依存sourceも明示して前置きする。別ファイルの暗黙の読み込みは行わない。

## 第一段

| ソース | 公開 API | 契約の要点 |
|---|---|---|
| `range/check.vaak` | `range_is_valid`, `range_is_index`, `range_length` | 半開区間。空区間の長さは 0。不正な `range_length` は paradox |
| `array/i64/search_linear.vaak` | `array_i64_find*`, `rfind*`, `contains*`, `count*` | 読み取り alias。探索失敗は paradox、空区間の count は 0 |
| `array/i64/search_binary.vaak` | `array_i64_lower_bound*`, `upper_bound*`, `binary_find*`, `binary_contains*` | 昇順が前提。境界探索は空区間の先頭を返す |
| `array/i64/reverse.vaak` | `array_i64_reverse*` | `var alias` でその場更新。成功は true、不正範囲は paradox |
| `array/i64/prefix_sum.vaak` | `array_i64_prefix_sum`, `array_i64_prefix_range_sum` | 結果は n + 1 要素。空区間の和は 0 |
| `array/i64/sort/insertion.vaak` | `array_i64_insertion_sort*` | stable、in-place。O(n²)、追加領域O(1) |
| `array/i64/sort/heap.vaak` | `array_i64_heap_sort*` | unstable、in-place。O(n log n)、追加領域O(1) |
| `array/i64/sort/merge.vaak` | `array_i64_merge_sort*` | stable、bottom-up。O(n log n)、作業配列O(n) |
| `array/i64/partition/sorted_unique.vaak` | `array_i64_sorted_unique_in_place` | 昇順を先に検査し、隣接重複をその場で除いて長さを返す |
| `array/i64/compress.vaak` | `CoordinateCompressionI64`, `array_i64_coordinate_compress`, `coordinate_compression_i64_*` | 元配列を保ち、昇順unique値と0-based rankを返す。binary search・merge sort・sorted uniqueへ明示依存 |
| `ds/dsu_i64.vaak` | `DsuI64`, `dsu_i64_*` | union by size + path compression。添字範囲外は paradox |
| `ds/rollback_dsu_i64.vaak` | `RollbackDsuI64`, `rollback_dsu_i64_*` | successful mergeだけをsnapshotへ保存。rollback可能にするためpath compressionは使わない |
| `ds/weighted_dsu_i64.vaak` | `WeightedDsuI64`, `weighted_dsu_i64_*` | i64折返し加法群のpotential差。不整合constraintはparadox |
| `ds/fenwick_i64.vaak` | `FenwickI64`, `fenwick_i64_*` | i64 加算に固定。point add と半開区間 sum |
| `ds/fenwick_count_i64.vaak` | `FenwickCountI64`, `fenwick_count_i64_*` | 非負point/total上限を保ち、prefix/rangeと累積個数`lower_bound` |
| `ds/fenwick_i64_flat.vaak` | `fenwick_i64_flat_*` | 生の配列一本を受ける性能対照。入れ子の左辺を作らない |
| `ds/fenwick_range_i64.vaak` | `RangeAddPointFenwickI64`, `RangeAddSumFenwickI64`と各prefix API | i64折返しrange add。point getは一配列、range sumは二配列 |
| `ds/heap_i64.vaak` | `MinHeapI64`, `MaxHeapI64`, `min_heap_i64_*`, `max_heap_i64_*` | 固定比較の二分heap。heapify、push/pop/peek/replace |
| `ds/deque_i64.vaak` | `DequeI64`, `deque_i64_*` | 容量倍増ring buffer。両端push/popは償却O(1) |
| `ds/segtree_i64.vaak` | `SumSegtreeI64`, `MinSegtreeI64`, `MaxSegtreeI64`と各prefix API | O(n) build、point set/get、半開区間prod/all_prod。空区間は各identity |
| `ds/lazy_segtree_i64.vaak` | `RangeAddSumSegtreeI64`, `range_add_sum_segtree_i64_*` | i64折返し加算上のrange add/range sum。build O(n)、更新・query O(log n) |
| `ds/sparse_table_i64.vaak` | `SparseMinI64`, `SparseMaxI64`, `sparse_min_i64_*`, `sparse_max_i64_*` | immutableな固定min/max。build O(n log n)、半開区間query O(1) |
| `ds/disjoint_sparse_table_i64.vaak` | `DisjointSparseSumI64`, `disjoint_sparse_sum_i64_*` | immutableなi64折返し和。build O(n log n)、半開区間query O(1) |
| `ds/ordered_multiset_i64.vaak` | `OrderedMultisetI64`, `ordered_multiset_i64_*` | sorted unique universe固定。重複、順位、0-based k-thをO(log n)で扱う |
| `graph/csr_scc_two_sat_i64.vaak` | `CsrBuilderI64`, `CsrI64`, `SccResultI64`, `BfsResultI64`, `TopologicalResultI64`, `TwoSatI64` | 安定順序CSR、再帰なしSCC/BFS、辞書順topological、2-SAT。添字はi64固定 |
| `io/ascii_i64.vaak` | `io_ascii_i64_*` | hostが一括で渡す`str`の整数scannerと返却用`str` formatter。stdin/stdout自体は持たない |

配列を値引数で受けると深い複製になるため、読み取りは `alias`、破壊は
`var alias` に揃えた。subarray を別名にせず `(xs, first, last)` の半開区間を渡す。
総和は Vaak の `i64` と同じく溢れたとき折り返す。

三種のsortは配列全体版と`[first, last)`版を持つ。有効な空区間と一要素区間は成功し、負数、逆転、
末尾越えは変更前にparadoxへする。insertion/mergeは同値要素を追い越さず、heapは安定性を保証しない。
`sorted_unique`も昇順違反を全走査してから変更するため、不正入力を途中まで圧縮しない。

座標圧縮は入力を複製してstable merge sortとsorted uniqueを適用し、元の各値をbinary searchでrankへ写す。
空入力は空の`unique_values`と`ranks`を持つ通常結果で、欠損値の`rank_of`と範囲外`value_at`はparadox。
公開欄を直接変えた後の外形は`coordinate_compression_i64_is_valid`で検査できるが、module privacyの代用となる
新しい不変条件は導入しない。

通常`DsuI64`はunion by sizeとpath compressionで償却O(alpha(n))、`RollbackDsuI64`はunion by sizeだけで
leader/mergeがO(log n)である。rollback版の`snapshot`はsuccessful merge数を返し、同一集合mergeは履歴を
増やさない。`undo`は最後のsuccessful mergeを、`rollback_to(snapshot)`はそれ以降を戻す。

`WeightedDsuI64.merge(a, b, difference)`は`potential(b) - potential(a) == difference`をi64の
折返し加法群上で課す。整合する重複constraintは成功し、矛盾はparadox。非連結`diff`もparadoxである。
通常版やrollback版と意味を混ぜず、potential版はpath compressionを使うためrollbackを提供しない。

`FenwickI64`の任意deltaとsumはi64で折り返すため、prefixの大小は単調とは限らず順位選択を提供しない。
`FenwickCountI64`は各pointとtotalを`0..=i64::MAX`へ保ち、違反する更新を変更前にparadoxへする専用型である。
`from_counts`はO(n)、point add・prefix/range/get・`lower_bound(target)`はO(log n)。順位選択は
`1 <= target <= total`だけを受け、`prefix(index + 1) >= target`となる最小の0-based indexを返す。
module privacy未完成のため各struct欄は見えるが、直接変更後のDSU parent/history/potentialやcount Fenwickの
非負/total不変条件は保証しない。公開関数で構築・更新した値を契約対象とし、opaque fieldを新しい意味論で装わない。

`RangeAddPointFenwickI64`はdifference列を一つのFenwickに持ち、range addとpoint getをO(log n)で行う。
`RangeAddSumFenwickI64`はdifference `d[index]`と`d[index] * index`の二本を持ち、
`prefix(last) = last * D(last) - W(last)`によりprefix/range sumもO(log n)で返す。二型の`from`はO(n)、
追加領域はそれぞれO(n)とO(2n)である。

演算はVaakのi64と同じ2の冪を法とする折返しで、range更新の値や係数積のoverflowだけをparadoxへ変えない。
有効な空区間は更新成功・sum 0、負数・逆転・末尾越えは更新前にparadox。任意deltaによりprefixは単調でないため
順位選択を提供しない。二配列型の公開欄を直接壊した値は契約対象外だが、`range_add_sum_fenwick_i64_is_valid`が
長さと係数対応をO(n log n)で再検査する。

現段階では generic、第一級 callback、sum 型、`match` を要求しない。比較や加算を
hot loop へ直接書く固定演算版を基準にし、将来のモジュール機構ではこのファイル境界を
読み込み・到達可能性・STEEL の特殊化単位として扱えるかを検証する。

`FenwickI64` 版は型名が付く一方、更新の左辺が `tree.data[i]` になる。flat 版は
型による取り違え防止を失う代わりに `data[i]` だけを更新する。この二本は永続 API の
候補を二重化するためではなく、現行 VM の入れ子経路の費用を分離して測る対照実験である。

heapのmin/maxもgeneric callbackを使わず比較をhot loopへ直接書いた。`data`欄はmodule privacyが
未完成の間は公開されるため、直接変更後は`*_heapify`でheap propertyを復元する。dequeは
`data/head/size`のring bufferで、容量不足時だけ論理順にコピーして倍増する。公開欄の構造不変条件は
`deque_i64_is_valid`で検査できる。

segment treeはgeneric monoidを装わず、sum/min/maxを別named typeへ固定した。`prod(first, last)`は
有効な空区間を許し、sumは0、minは`i64::MAX`、maxは`i64::MIN`を返す。範囲外、逆転区間、負の長さ、
内部配列長をi64で表せない長さはparadoxである。sumとrange-add-sumの算術overflowは、Fenwickと同じく
Vaakのi64規則で折り返す。

range addとsumは2の冪を法とする加算monoid上でも自然に合成できるのでlazy版を独立型にした。一方、
折返し加算は大小関係を保存しないため、nodeのmin/maxへdeltaを足すだけのrange-add-min/maxは正しくない。
overflowだけ別にparadoxへ変える契約も導入していない。range assign、任意predicateの`max_right` /
`min_left`、generic actionはAPI候補のままにし、callback/module specializationと現行の複合place費用を
測る前に固定variantを増やさない。

`SparseMinI64` / `SparseMaxI64`は更新しない配列をO(n log n)でflat tableへ展開し、idempotentな
min/maxを重なる二区間からO(1)で返す。`DisjointSparseSumI64`は各block中央から左suffix・右prefixを作り、
端点の最高相違bitに対応する二集約をi64の折返し加算で結ぶ。三者とも有効な空区間を許し、minは
`i64::MAX`、maxは`i64::MIN`、sumは0を返す。範囲外・逆転はparadoxである。

sparse tableはstatic query用で、point/range更新後も使える構造を装わない。公開欄を直接変更した値は
契約対象外だが、`*_is_valid`はflat shapeと構築済み集約をO(n log n)で再検査できる。generic idempotent演算や
任意monoid callbackを先取りせず、min/max/sumの固定三演算だけを別APIにした。

`OrderedMultisetI64`は構築時のsorted uniqueな`keys`を複製し、未知keyをonline追加しない。`insert`、
`erase_one`、`count`、`contains`、`order_of_key`、0-based `kth`はO(log n)、`len`はO(1)である。任意keyの
`count`は0、`order_of_key`はそのkey未満の総個数を返す。universe外のmutationと範囲外`kth`はparadox、
universe内で個数0の`erase_one`は`false`であり、失敗時に個数を変えない。

内部は`keys`、非負count用の`fenwick`、`total`を一つの型に持つ。`FenwickCountI64`と同じ非負・上限の
不変条件とbit walkを使うが、別named型を入れ子にせず単独sourceとしてSTEELまで前置きできる形にした。
公開欄を直接変更した値は契約対象外で、`ordered_multiset_i64_is_valid`がkey順・shape・各count・totalを
O(n log n)で検査する。online balanced tree、乱数priority、generic key比較はこのAPIから推測させない。

`io/ascii_i64.vaak`はS-4のhost境界を変えない。hostがstdin等を一括で`str`として渡し、Vaakは
cursorを明示して読む。出力は`str`へ追記し、最上位の値としてhostへ返す。標準host名、streaming、
buffer flushは未決であり、このsourceはそれらを暗黙に導入しない。

graph sourceは各頂点内で辺の追加順を保つCSRを一度構築し、そのflat表現を反復Kosaraju SCCと
BFS、topological sort、2-SATで共有する。BFSの未到達距離・親は`-1`、始点の親は始点自身で、同距離の
親はCSR内で最初に発見したものを保つ。topological sortは利用可能な頂点番号が小さい順の辞書順最小で、
cycleは`TopologicalResultI64(false, [])`という通常値にする。SCCの成分番号は異なる成分間の辺`u -> v`に対して`group_of[u] < group_of[v]`と
なる位相順である。空graph、self-loop、parallel edge、切断graphを通常入力として扱う。2-SATの
充足不能は`TwoSatResultI64(satisfiable := false, assignment := [])`であり、不正添字のparadoxと分ける。

[`examples/graph_scc_io.vaak`](examples/graph_scc_io.vaak)はgraph sourceと`io/ascii_i64.vaak`を前置きし、
host-ownedな一括入力`str`から有向辺を読み、一括出力`str`を返す接続例である。stdin/stdoutやhost名を
pure libraryへ持ち込まず、既存の競技入力層とgraph演算を組み合わせる。
