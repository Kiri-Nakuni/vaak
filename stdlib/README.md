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
| `ds/heap_i64.vaak` | `MinHeapI64`, `MaxHeapI64`, `min_heap_i64_*`, `max_heap_i64_*` | 固定比較の二分heap。heapify、push/pop/peek/replace |
| `ds/deque_i64.vaak` | `DequeI64`, `deque_i64_*` | 容量倍増ring buffer。両端push/popは償却O(1) |
| `ds/segtree_i64.vaak` | `SumSegtreeI64`, `MinSegtreeI64`, `MaxSegtreeI64`と各prefix API | O(n) build、point set/get、半開区間prod/all_prod。空区間は各identity |
| `ds/lazy_segtree_i64.vaak` | `RangeAddSumSegtreeI64`, `range_add_sum_segtree_i64_*` | i64折返し加算上のrange add/range sum。build O(n)、更新・query O(log n) |
| `io/ascii_i64.vaak` | `io_ascii_i64_*` | hostが一括で渡す`str`の整数scannerと返却用`str` formatter。stdin/stdout自体は持たない |

配列を値引数で受けると深い複製になるため、読み取りは `alias`、破壊は
`var alias` に揃えた。subarray を別名にせず `(xs, first, last)` の半開区間を渡す。
総和は Vaak の `i64` と同じく溢れたとき折り返す。

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

`io/ascii_i64.vaak`はS-4のhost境界を変えない。hostがstdin等を一括で`str`として渡し、Vaakは
cursorを明示して読む。出力は`str`へ追記し、最上位の値としてhostへ返す。標準host名、streaming、
buffer flushは未決であり、このsourceはそれらを暗黙に導入しない。
