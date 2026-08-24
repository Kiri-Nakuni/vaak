# 競プロ向けpure Vaak標準ライブラリroadmap

> **状態：実験checkpoint。決定ではない。** 新しい構文、意味論、S-n、module API、host capabilityを
> この文書では確定しない。`docs/vaak/decisions.md`と
> [`modules-and-library.md`](modules-and-library.md)を変更するものでもない。

- 調査・実測日: 2026-08-24〜2026-08-25
- branch: `codex3/stdlib-dense-bitset`
- dense bitset base: `codex3/stdlib-json-jsonl` = `1bb004b252183ad4b3706a81470733ec3657d466`
- integrated bulk I/O checkpoint: `codex3/stdlib-bulk-io` = `62498f1add42bf05f39385af09411ae3ebb728bd`
- integrated Fenwick range checkpoint: `codex3/stdlib-fenwick-range` = `2012cb0a4bd41a098dad8aa8f7dbbd0d15098828`
- integrated ordered multiset checkpoint: `codex3/stdlib-ordered-multiset` = `3e2eeef2202a4ede6394675bd8d8942cb9a0b0eb`
- integrated sparse checkpoint: `codex3/stdlib-sparse-table` = `2ea12164647d232c2a451274f80439526db9c057`
- integrated ordering checkpoint: `codex3/stdlib-ordering` = `0f22c4899d2692fe785a8f0de8f8652bb8cc3006`
- integrated structure checkpoint: `codex2/stdlib-heap-deque` = `4a31a5fb4810e67a37e82c658cfd913a0446c0e2`
- integrated graph checkpoint: `codex2/stdlib-graph` = `e8c2f93aeddd6394ef77342543a7a09637de8057`
- inherited base: `origin/codex2/full` = `7c5ccd706e4dc466dad45baf275c0b550c8bc777`
- 実装範囲: pure Vaak source、差分試験、benchだけ。core/STEELの意味は変更しない

## 1. 第一checkpointの境界

Vaakにはまだ確定したmodule/import、generic type、第一級callback、module privacyが無い。そこで一つの
巨大な`stdlib`を作らず、必要な`.vaak` sourceだけを利用者sourceの前へ置く現行方式を保つ。公開名は
将来のmodule分割を先取りするprefixを持つが、将来のmodule pathや再export構文までは約束しない。

今回加えたのは次の三sourceである。

| source | 固定した範囲 | 固定しないもの |
|---|---|---|
| `stdlib/ds/heap_i64.vaak` | i64固定のbinary min/max heap | generic比較、任意priority、opaque field |
| `stdlib/ds/deque_i64.vaak` | i64固定の容量倍増ring buffer | generic要素、allocator、同期queue |
| `stdlib/io/ascii_i64.vaak` | 既存`str`上のi64 scanner/formatter | stdin/stdout、標準host名、streaming |

DSUの`DsuI64` / `dsu_i64_*`、Fenwickの`FenwickI64` / `fenwick_i64_*`と同様、named structは
PascalCase、自由関数は型・演算を含むsnake_case prefixへ揃えた。読み取りは`alias`、破壊は
`var alias`で受け、集合体の値渡しによる意図しない深い複製を避ける。

## 2. i64 min/max heap契約

min/maxを一つのruntime tagやcallbackへまとめず、比較を各hot loopへ直接書く。現在の参照実装では
小関数frame自体が無視できず、STEELだけでinlineされる抽象を全backend共通APIの基準にできないためである。

| API | 空 | 時間 | 追加領域 |
|---|---|---:|---:|
| `*_new()` | 空heapを返す | O(1) | O(1) |
| `*_from(values)` | 空配列も有効 | O(n) | O(n)、入力を一度複製 |
| `*_heapify(heap)` | 成功`true` | O(n) | O(1) |
| `*_len`, `*_is_empty`, `*_peek` | `peek`だけparadox | O(1) | O(1) |
| `*_push` | 成功`true` | 償却O(log n) | 配列伸長分 |
| `*_pop` | paradox | O(log n) | O(1) |
| `*_replace` | paradox | O(log n) | O(1) |
| `*_is_valid` | `true` | O(n) | O(1) |

`replace`は先頭を新値へ置き換え、以前の先頭を返す。min版は最小、max版は最大を先頭にする。
重複値、`i64::MIN`、`i64::MAX`を通常値として扱い、算術集約はしないので整数overflowを新たに生まない。

module privacyが無いため`data`欄は利用者から変更できる。直接変更後の操作結果はheap propertyを前提とし、
`heapify`で復元できる。これをopaque typeの代用となる新しい意味論にはしない。

## 3. i64 deque契約

`DequeI64`は`data : i64 array`、`head : i64`、`size : i64`を持つ。logical offsetはring上の物理添字へ
overflow無しで写す。満杯のpushだけが容量を4または2倍へ伸ばし、論理順にO(n) copyする。

| API | 空・不正 | 時間 | 備考 |
|---|---|---:|---|
| `new`, `with_capacity` | 負capacityはparadox | O(capacity) | capacity 0は有効 |
| `len`, `capacity`, `is_empty`, `is_valid` | query | O(1) | 公開欄の構造不変条件を検査 |
| `push_front`, `push_back` | 成功`true` | 償却O(1) | grow時だけO(n) |
| `peek_front`, `peek_back` | 空はparadox | O(1) | 値を残す |
| `pop_front`, `pop_back` | 空はparadox | O(1) | 値を除いて返す |
| `get(index)` | 負・範囲外はparadox | O(1) | logical index |
| `clear` | 成功`true` | O(1) | 容量を保持 |

capacity倍増が`i64`を越える場合は配列長を折り返さずparadoxにする。実際のallocation failureは既存runtimeの
resource failureであり、このlibraryは新しい回復意味論を定めない。

アルゴリズム上の償却O(1)と現行backendで速いことは別である。1024要素の一標本では、ring dequeは
生配列の`remove(0)`より木で約105倍、VMで約26倍遅かった。named functionと複合placeの定数費用が
支配している。flat表現、規模別crossover、coreの既存place fast path後を比較してから永続APIを判断する。

## 4. 差分試験とbench

`tests/heap_deque.rs`は次を固定する。

- 空のpeek/pop/replace、負capacity、範囲外getがparadox
- heapify、min/max順、重複、`i64`両端、wrap/grow/clear
- 固定seedの128操作をRust `BinaryHeap` / `VecDeque`へも適用する独立oracle
- 各操作後の値、長さ、heap/ring不変条件を参照実装とVMで比較
- 同じrandom sourceをSTEEL LLVM IRへ変換し、clangがあればnative exit値まで比較

このmachineにはclangが無いため、2026-08-24のcheckpointではSTEEL IR生成が成功し、native実行gateは
明示skipになった。clangのあるrelease環境ではskipせず同じsourceを走らせる。

`examples/bench_array_library.rs`はparse/check/VM compileを計時外にし、tree/VMを同じprocess内で測る。
5回平均のrawな一標本は`stdlib/BENCHMARK.md`に記録した。絶対時間や1024要素だけからbackend/APIを
決定しない。

### 4.1 第二checkpoint: 固定i64 segment tree

generic monoid、action callback、module specializationを先取りせず、次の二sourceを加えた。

| source / 型 | 構築 | 更新 | query | 空区間identity |
|---|---:|---:|---:|---|
| `segtree_i64.vaak` / `SumSegtreeI64` | O(n) | point set O(log n) | get O(1)、prod O(log n)、all O(1) | 0 |
| 同 / `MinSegtreeI64` | O(n) | point set O(log n) | 同上 | `i64::MAX` |
| 同 / `MaxSegtreeI64` | O(n) | point set O(log n) | 同上 | `i64::MIN` |
| `lazy_segtree_i64.vaak` / `RangeAddSumSegtreeI64` | O(n) | range add O(log n) | get/prod O(log n)、all O(1) | 0 |

全rangeは`[first, last)`で、0長の木と有効な空区間を許す。負長、逆転、範囲外、内部配列長の
`i64` overflowはparadoxである。sumとrange-add-sumの値計算はFenwick同様、Vaakのi64算術で折り返す。
range-add-sumは2の冪を法とする加算でも`sum += delta * length`が成立するため、意味論を増やさずlazyに
できる。

一方、折返し加算は順序を保存しない。例えば`i64::MAX`と0へ1を加えると`i64::MIN`と1になるので、
nodeのmin/maxへdeltaを足すだけのrange-add-min/maxは誤る。更新だけoverflow時paradoxへ変える別契約も
導入しない。range assignは実装可能だが、pending actionを持つ複数配列へのcompound placeと再帰関数枠の
費用が大きい段階で永続APIを増やさず、別型候補として保留した。

現構文では`&=`の対象は名前に限られ、`tree.data`へ直接aliasを張れない。そこでnamed型のhot pathは
`tree.data[i]`を通る。API追加前の再測定でもalias関数枠とcompound placeの費用を再確認し、追加後には
flat配列へloopをその場書きしたsegment queryとnamed型をpaired測定した。結果は
`stdlib/BENCHMARK.md`にraw値として残し、計算量と現backendの定数費用を混同しない。

`tests/segtree_i64.rs`は0長、identity、範囲、容量overflow、i64折返し、重なるlazy更新と、Rustの
独立vector oracleから生成した決定的random列をreference/VM/STEELへ同じsourceで流す。再帰helperでは
成功時の早期flowが外側frameまで抜ける形を避け、分岐値を自然に返す。これはcore意味論の変更ではない。
2026-08-24のfocused gateは31 passed、全release gateは746 passed、0 failed、1 ignored。STEELは
LLVM IR生成まで成功し、このmachineにはclangが無いためnative実行部分だけを既存方針どおりskipした。

`max_right` / `min_left`はpredicateの単調性とidentity条件を要求する。generic callbackが未完成の間に
`>= threshold`等の固定predicate variantを乱造せず、callback/module specializationのAPI候補として保留する。

### 4.2 第三checkpoint: DSU派生と非負count Fenwick

通常の`DsuI64` / `FenwickI64`と意味を混ぜず、次の三sourceを独立型として加えた。

| source / 型 | 公開操作 | 計算量 | 意味上の境界 |
|---|---|---|---|
| `rollback_dsu_i64.vaak` / `RollbackDsuI64` | merge/same/size/group_count、snapshot/undo/rollback_to | leader/merge O(log n)、snapshot O(1)、rollback O(戻すsuccessful merge数) | union by sizeだけ。path compressionをせず、同一集合mergeは履歴を増やさない |
| `weighted_dsu_i64.vaak` / `WeightedDsuI64` | merge/same/diff/size/group_count | 償却O(alpha(n)) | `potential(b)-potential(a)`をi64折返し加法群で課し、矛盾と非連結diffはparadox |
| `fenwick_count_i64.vaak` / `FenwickCountI64` | O(n) build、point add、prefix/range/get、total、lower_bound | build O(n)、query/update O(log n)、total O(1) | point/totalを非負i64へ保ち、違反更新を原子的にparadoxへする |

rollbackとweightedを一型のflagで切り替えない。前者のrollback可能性はpath compression無しという計算量を、
後者のpotential queryはpath compressionと加法群を要求するためである。weightedの`difference`を通常の順序付き
整数差とは呼ばず、`i64::MAX + 1 == i64::MIN`となる2の冪剰余の群演算まで差分試験へ固定した。

任意deltaを許す`FenwickI64`はprefixが単調でないため`lower_bound`を追加しない。順位選択が必要な場合だけ
非負count専用型を使い、`1 <= target <= total`で`prefix(index + 1) >= target`となる最初の0-based indexを返す。
overflow、point underflow、範囲外、空木の順位選択は、部分更新せずparadoxになる。

`tests/dsu_variants.rs`はrollbackをRustのDSU state snapshot、weightedを独立constraint graph/DFSへ照合する。
`tests/fenwick_variants.rs`は非負`Vec<i64>`の線形prefix/range/k-th oracleへ照合する。どちらも固定seedの操作列を
reference / VM / STEELへ流す。外部仕様の参照範囲は
[`competitive-stdlib-license-ledger.md`](competitive-stdlib-license-ledger.md)へ分離した。
2026-08-24のfocused gateは26 passed、全release gateは760 passed、0 failed、1 ignored。STEEL LLVM IR生成は
成功し、このmachineにclangが無いためnative実行部分だけを既存方針どおりskipした。

### 4.3 P1第一checkpoint: 固定i64 ordering

module、generic比較、callbackを増やさず、配列の順序層を次の細粒度sourceへ分けた。

| source | 公開操作 | 時間 | 追加領域・安定性 |
|---|---|---:|---|
| `array/i64/sort/insertion.vaak` | 全体 / 半開区間sort | O(n²) | O(1)、stable |
| `array/i64/sort/heap.vaak` | 全体 / 半開区間sort | O(n log n) | O(1)、unstable |
| `array/i64/sort/merge.vaak` | 全体 / 半開区間sort | O(n log n) | O(n)、stable |
| `array/i64/partition/sorted_unique.vaak` | 昇順検査後のin-place unique | O(n) | O(1) |
| `array/i64/compress.vaak` | unique値/rank構築、rank/value往復、外形検査 | 構築O(n log n) | O(n) |

sortの範囲は`[first, last)`で、有効な空・一要素区間は成功する。負数、逆転、末尾越えは変更前にparadox。
sorted uniqueも昇順を先に全検査し、不正列を部分的に短縮しない。座標圧縮は入力を保持し、昇順で重複のない
`unique_values`と元要素ごとの0-based `ranks`を返す。空入力は空の通常結果、欠損値と範囲外rankはparadox。

`compress.vaak`は既存binary search、今回のmerge sort、sorted uniqueを冒頭で明示依存にし、同じ判断を
別sourceへ複製しない。`tests/array_ordering.rs`ではRust `Vec::sort` / `dedup` / `binary_search`を独立oracleにし、
重複、空、不正範囲、`i64::MIN/MAX`、元配列保持をreference / VM / STEEL nativeへ同じsourceで流す。
Ubuntu clang 18.1.3の2026-08-25 focused gateは10 passed、0 failed、inventoryの`tests/array_library.rs`は
12 passed、0 failedだった。benchmarkのraw値は`stdlib/BENCHMARK.md`へ分離した。

統合した旧checkpointはclang無しの環境でnative部分をskipしていた。現在のLinux環境での全release gateは
776 passed、6 failed、1 ignored。失敗は今回未変更のnative試験だけで、旧`tests/graph_library.rs`の
SCC I/Oとtopological sort二件がsignal終了、旧`tests/heap_deque.rs`の長いrandom heap列、
`tests/io_ascii_i64.rs`のformatter/scanner往復、`tests/string_library.rs`のpure Vaak例がexit 0になった。
各testのreference / VMは一致する。このP1ではcore・STEEL・旧sourceを変更せず、継承baselineとして残して
意味論所有者の領域へ先回りしない。

### 4.4 P1第二checkpoint: static sparse table

更新の無いrange queryを、固定演算ごとに二sourceへ分けた。

| source / 型 | build | query | 空区間identity | 追加領域 |
|---|---:|---:|---|---:|
| `sparse_table_i64.vaak` / `SparseMinI64` | O(n log n) | min O(1) | `i64::MAX` | O(n log n) |
| 同 / `SparseMaxI64` | O(n log n) | max O(1) | `i64::MIN` | O(n log n) |
| `disjoint_sparse_table_i64.vaak` / `DisjointSparseSumI64` | O(n log n) | sum O(1) | 0 | O(n log n) |

min/maxはidempotentなので、区間長以下の最大2冪を両端から取った重なる二区間を比較する。sumは重複できない
ため、各levelのblock中央から左suffixと右prefixを構築し、`first`と`last - 1`の最高相違bitに対応する二値を
折返し加算する。どちらも整数`leading_zeros`とbit演算だけを使い、query loopやcallbackを持たない。
有効な空・一要素区間を通常結果にし、負数、逆転、末尾越えはparadoxへ分ける。

`tests/sparse_table_i64.rs`は非2冪長、空、i64両端、折返し和、公開欄破損と固定seedのrange列をRustの
slice min/max・`wrapping_add` oracleへ照合する。2026-08-25のfocused gateはreference / VM / STEEL nativeで
6 passed、0 failed、inventoryは12 passed、0 failed。`examples/bench_sparse_table.rs`は同じ512要素・
4096 queryを64幅の線形走査とpairedにし、raw値を`stdlib/BENCHMARK.md`へ残した。
全release gateは782 passed、6 failed、1 ignoredで、失敗6件は4.3に列挙した未変更のnative baselineと同じ。

`gcd`版は`i64::MIN`を含む非負化、符号、identityの契約を数値checkpointで先に定める。任意idempotent演算や
monoid callback、更新可能なsparse tableはこのsourceから推測させず、generic/module所有者へ残す。

### 4.5 P1第三checkpoint: compressed ordered multiset

online balanced treeやgeneric比較を先取りせず、構築時にsorted uniqueなi64 universeを固定する一型を加えた。

| source / 型 | build | mutation / query | 不正・空 | 追加領域 |
|---|---:|---:|---|---:|
| `ordered_multiset_i64.vaak` / `OrderedMultisetI64` | O(n) | insert、erase、count、rank、k-th O(log n)、len O(1) | universe外mutationと範囲外k-thはparadox、空eraseはfalse | O(n) |

`count(value)`はuniverse外も0、`order_of_key(value)`は任意i64について未満の総個数を返す。`kth(index)`は
0-basedで、重複を個数分だけ順位へ含める。mutation前にpoint/totalの非負・`i64::MAX`上限を検査するため、
失敗はFenwick欄を部分更新しない。constructorは入力universeを複製し、重複・降順をparadoxへする。

内部は`keys`、`fenwick`、`total`の三fieldで、`FenwickCountI64`と同じ非負count invariantとbit walkを使う。
別named型を入れ子にした補助呼出は現行STEELの共通sourceにできなかったため、このsourceは暗黙依存を持たず
flat fieldを直接扱う。core/STEELは変更せず、将来のmodule compositionやonline key構造のAPIを確定しない。

`tests/ordered_multiset_i64.rs`は固定境界と280操作を独立Rust vector oracleへ照合し、reference / VM /
STEEL nativeで5 passed、0 failed。inventoryは12 passed、0 failedで、単独前置きと全source名衝突も通った。
`examples/bench_ordered_multiset.rs`は256-key、1024 insert、rank/k-th各2048件を線形count列とpairedにし、
raw値を`stdlib/BENCHMARK.md`へ残した。
全release gateは787 passed、6 failed、1 ignoredで、失敗6件は4.3に列挙した未変更のnative baselineと同じ。

### 4.6 P1第四checkpoint: range-update Fenwick

既存`FenwickI64`のpoint-add契約を変えず、range updateの用途を一source内の二型へ分けた。

| 型 | build | update | query | field |
|---|---:|---:|---:|---:|
| `RangeAddPointFenwickI64` | O(n) | range add O(log n) | point get O(log n) | difference Fenwick一本 |
| `RangeAddSumFenwickI64` | O(n) | range add O(log n) | point/prefix/range sum O(log n) | differenceとindex係数の二本 |

二配列型はzero-based difference `d`について`D(last) = sum(d[0..last])`、
`W(last) = sum(d[index] * index)`を持ち、`prefix(last) = last * D(last) - W(last)`を使う。これはi64の
2の冪を法とする加算・乗算でも成立するため、overflowだけを別のparadoxへしない。任意deltaを許すのでprefixの
単調性を仮定するlower-boundは提供せず、非負順位には既存`FenwickCountI64`を使う。

有効な空区間は更新成功・sum 0、負数・逆転・末尾越えは全fieldを触る前にparadox。二配列型は公開欄のshapeと
`weighted[index] == delta[index] * index`をO(n log n)で検査できる。`tests/fenwick_range_i64.rs`はi64両端、
零長、不正範囲、公開欄破損、入力複製と260操作のRust vector oracleをreference / VM / STEEL nativeへ流し、
5 passed、0 failed。inventoryは12 passed、0 failedで、単独前置きと全source名衝突も通った。

`examples/bench_fenwick_range.rs`は256要素への2048 range updateとpoint/range queryを生配列の線形処理と
pairedにし、raw値を`stdlib/BENCHMARK.md`へ残した。現行の木を辿る実装では二型とも線形処理より遅く、VMでは
point型が約1.13倍、sum型が約1.51倍速い一標本だった。計算量とcompound fieldの定数費用を分けて扱う。
全release gateは792 passed、6 failed、1 ignoredで、失敗6件は4.3に列挙した未変更のnative baselineと同じ。

### 4.7 P1第五checkpoint: bulk ASCII i64

host-ownedな入力`str`と最上位へ返す出力`str`の境界を変えず、既存`ascii_i64.vaak`へ二つのadditive APIを加えた。

| API | allocation / frame | 成功 | 失敗 |
|---|---|---|---|
| `io_ascii_i64_read_n_into` | caller-owned i64配列、一つのbulk frame | 指定欄へcount個を書きcursorを進める | 初期範囲違反は原子的。token失敗は成功prefixだけ確定 |
| `io_ascii_i64_format_range` | 結果str一つ、20-byte scratch一つ | 値間だけASCII separatorを置く | 範囲外・非ASCII separatorはparadox |

bulk scannerはtokenごとの文字列を作らず、whitespace/sign/digit/overflowを一つのloop内で処理する。途中失敗で
既に読んだ値を巻き戻さず、positionは最後に成功したtoken直後、失敗欄は未変更とする。既存の一token readが
失敗token自身のcursorを変えない契約と揃え、部分writebackの新しい一般則は作らない。

bulk formatterは`i64::MIN`を負のまま桁へ分解し、固定scratchへ逆順に置いて結果へ戻す。separatorを値の間に
だけ置くため、有効な空範囲は空`str`で、末尾separatorは付けない。`str.reserve`、caller-owned可変capacity、
streaming flush、stdin/stdout runner、host名はこのpure sourceへ入れない。

`tests/io_ascii_i64_bulk.rs`は固定境界、成功prefix、範囲違反と192値のRust parse/format oracleをreference / VM /
STEEL nativeへ流し、5 passed、0 failed。既存single-token試験のreference / VMも不変である。
`examples/bench_io_ascii_bulk.rs`は同じ2048整数のreadと1024整数のformatをpairedにし、raw値を
`stdlib/BENCHMARK.md`へ残した。bulk readは木で約1.81倍、VMで約1.05倍速い一方、bulk formatはこの標本で
木約1.05倍、VM約1.12倍遅く、per-value allocation削減を速度保証とは扱わない。
全release gateは797 passed、6 failed、1 ignoredで、失敗6件は4.3に列挙した未変更のnative baselineと同じ。

### 4.8 P2第一checkpoint: UTF-8 JSON / JSON Lines

PraTeX側から来た一般codec要件だけを入力にし、PraTeX固有schemaやfile capabilityを持たない二sourceを加えた。

| source / 型 | 公開操作 | 表現・境界 |
|---|---|---|
| `codec/json_utf8.vaak` / `JsonUtf8Document` | parse、serialize、typed accessor、append-only builder | node/edge平行配列。UTF-8、順序保存、duplicate拒否、i64 number限定 |
| `codec/jsonl_utf8.vaak` / `JsonlUtf8Reader` | feed、finish、next、buffered bytes、serialize line | 有界owned chunk。LF/CRLF/終端LFなし、record位置、terminal error |

JSON documentを再帰structにせず、leafを先、containerを後に追加するflat DAGへした。builderのchild IDは親より
小さく、serializerは全node/edgeのcanonical shape、UTF-8、key重複を出力前に検査する。object keyは入力・追加順を
保ち、sortしない。parse失敗は空document、serialize失敗は空bytesだけを返すため、部分結果をcommitしない。

numberは`i64::MIN..=i64::MAX`だけを値として持ち、`-0`を0へ正規化する。正しい小数・指数はunsupported、
整数範囲外とnumber構文違反は別codeである。これによりf64丸め、任意精度、decimal表現を初版から暗黙に決めない。
入力byte、container深さ、node、decoded string/key、edge、出力byteに独立budgetとstable codeを持たせた。

JSONLはfeedしたchunkを完全なrecordになるまでUTF-8検査しないので、scalar、`\u` escape、numberの途中で分割できる。
JSON errorのrecord内offset/line/columnに、0-based record番号、絶対record開始、絶対error位置を加える。空・空白行は
skipせず固有errorにし、reader errorはterminalとする。record/総入力/未読bufferを別上限にし、file/socket/stdin、
PraTeX build manifest schema、host capabilityをsourceへ入れていない。

最初のreaderはrecordごとに残りsuffixを複製したが、512件で参照575.383 ms・VM203.288 ms、2048件でVM3.128 sに
なった。同じfixtureでbuffer head＋間欠compactへ変えると512件が参照284.985 ms・VM73.284 ms、2048件VM272.645 ms
になったため、毎record copy案は棄却した。raw値と再現commandは[`stdlib/BENCHMARK.md`](../../stdlib/BENCHMARK.md)
に残す。さらに4,096-byte recordを16-byte chunkで伸ばすfixtureでは、毎chunkの改行再探索がVM 943.026 ms、
scan cursor版が51.895 msだったため、探索済みprefixの再走査も棄却した。codec契約とerror code一覧は
[`stdlib/README.md`](../../stdlib/README.md)を参照する。

### 4.9 共通core checkpoint: fixed dense bit set

TeX埋め込み、LVMINIBVS、競技programのいずれにもある「一つの既知snapshot内の有限集合」を、
application固有adapterより先にpure Vaakで共有するため、`stdlib/ds/dense_bitset_u32.vaak`を加えた。

| 型 / source | 固定した範囲 | 別層へ残したもの |
|---|---|---|
| `DenseBitSetU32(length, words)` | 0-based固定長集合、32-bit word、末尾padding 0 | Unicode分類、TeX catcode、game world identity、vertex意味 |
| 単一点操作 | contains / assign / insert / remove、変更status | atomic、同期、host object handle |
| 集合操作 | copy / union / intersection / xor / difference / complement | 可変長集合、sparse集合、generic word幅 |
| query | count / intersects / subset / first / next / 0-based k-th | iterator構文、callback、sentinel一般則 |

単一点操作はO(1)、全体操作はO(ceil(length / 32))で、負長、範囲外index、shape違反、長さ違いは
paradoxにする。二項mutationは両operandを先に検査してから更新し、末尾paddingを常に0へ保つ。
sourceは他libraryへ依存せず、`vaak::stdlib::DENSE_BITSET_U32`として明示前置きできる。

`tests/dense_bitset_u32.rs`の7試験は、0/1/31/32/33境界、deep copy、padding、変更前拒否に加え、
TeX ASCII文字class、12候補のLVMINIBVS有限解釈mask、70要素の競プロ集合を同一sourceで通す。
固定seed 97-bit・240操作はRust `Vec<bool>`を結果oracleにし、通常値の全fixtureを参照実装・VM・
STEEL clang `-O2` nativeで照合した。focused gateは7 passed、0 failed、inventory gateは12 passed、0 failed。
全release gateは833 passed、6 failed、1 ignoredで、失敗は4.3に列挙した未変更のgraph 3件、heap、
ASCII I/O、stringのSTEEL native baselineと同じである。

故意に公開fieldを壊した値と負長constructorのparadox回収は参照実装・VMでも確認した。一方、負長を
`??`でfallbackへ回収するisolated sourceをSTEEL nativeへ通すと期待終了値42に対し48となったため、
そのerror-pathを三backend一致の主張へ含めない。valid-valueのmutation・長さ違い拒否はSTEEL nativeでも
一致している。これは既存STEELのparadox/fallback経路の観測として残し、pure library checkpointからcoreや
言語意味を変更しない。意味論・STEEL所有者が直した後に同じfixtureをnative gateへ昇格する。

2051 bit・集合演算128回ではu8-per-bit案が融合packed flat案より木28.152倍、VM28.295倍遅く、payloadも
7.888倍大きかったため棄却した。同じ三走査のflat u32に対してもnamed型は木4.723倍、VM2.667倍の時間を
要したが、論理長・padding・長さ一致を値として運べないため公開APIには採らず性能対照だけ残す。
raw値、環境、再現commandは
[`stdlib/BENCHMARK.md`](../../stdlib/BENCHMARK.md)に固定した。

## 5. flat graph checkpoint

`stdlib/graph/csr_scc_two_sat_i64.vaak`は別sourceを暗黙に読み込まず、次を一ファイルで提供する。

| 層 | API | 契約 |
|---|---|---|
| builder | `csr_i64_builder_new`, `add_directed`, `add_undirected`, `build` | 頂点内で追加順を保持。無向self-loopは同じ向きの二辺 |
| CSR | `CsrI64(start, to)`、個数・degree・neighbor・reverse | `start`はV+1、`to`はEのflat配列。不正公開欄はparadox |
| SCC | `scc_i64`, `scc_i64_same` | 反復Kosaraju。`group_count`とV長の`group_of` |
| BFS | `bfs_i64` | 単一始点。V長の`distance`/`parent`、未到達は-1 |
| topological | `topological_sort_i64` | 辞書順最小。cycleは`acyclic=false`と空order |
| 2-SAT | `two_sat_i64_add_clause`, `set_value`, `solve` | literal graphをCSR/SCCへ渡す。充足不能は通常結果 |

CSR構築、SCC、2-SATはいずれもO(V+E)である。`add_*`のhot pathは既存辺を毎回再走査せずO(1)、公開欄を
利用者が直接変更した場合の全検査は`build`/`solve`前に一度行う。頂点・変数・辺端点はi64固定の
0-based indexで、負数、範囲外、V+1や2Vをi64で表せない個数はparadoxにする。

SCCはvertex昇順とCSR内の辺順をtie-breakに使い、異なる成分を結ぶ辺`u -> v`には
`group_of[u] < group_of[v]`となる番号を決定的に付ける。空、self-loop、parallel edge、切断graphを
通常入力として扱う。再帰は使わず、4096頂点の有向路を参照実装・VM・STEEL IRで通したため、現行の
評価深さ上限へDFS深さを重ねない。

BFSも再帰せず、V要素を一度だけ入れるflat queueでO(V+E)とする。未到達の`distance`/`parent`は-1、
始点のparentは始点自身である。同じ最短距離の親候補はCSRの辺順で最初に発見したものを保つ。
topological sortはKahn法のready集合を固定i64 min-heapにし、利用可能な頂点番号が小さい順、すなわち
辞書順最小のorderをO(E+V log V)で返す。cycleは不正入力ではないためparadoxにせず、partial prefixも
公開せず`TopologicalResultI64(false, [])`へ原子的に畳む。parallel edgeはindegreeを辺ごとに数える。

2-SATは`(x_i == value_i) OR (x_j == value_j)`を直接追加する。充足不能は
`TwoSatResultI64(false, [])`であり、不正入力のparadoxではない。充足可能ならV長のbool assignmentを返す。
固定seed graphはRustの推移閉包による相互到達性、固定seed clauseはRustの全割当列挙を独立oracleにし、
参照実装・VM・STEELの同じsourceを照合する。ACLのsource/testは使っていない。

`stdlib/examples/graph_scc_io.vaak`は既存`io/ascii_i64.vaak`とgraph sourceを前置きし、hostが一括で渡す
`str`から`n m`とm辺を読み、成分数とV個のgroup番号を一括`str`で返す。これはpure層と競技I/O層の
接続例であり、stdin/stdout、標準host名、streaming、callbackを新設しない。

### 5.1 既存DSU/FenwickとのAPI差分

このbranchはbase `54e05ab`に既に含まれる`DsuI64`/`FenwickI64`をそのまま参照し、別branchのmergeや
core変更はしていない。三者は同じi64 indexでも状態契約が異なる。

| API | 状態 | 結果 | graphとの関係 |
|---|---|---|---|
| `DsuI64` | merge/経路圧縮でmutable | leader、size、group数。全頂点label配列は返さない | 無向辺を逐次処理。CSRや辺順を要求しない |
| `FenwickI64` | point addでmutable | i64のprefix/半開区間sum | 頂点・辺ではなく数列index。overflowはVaak i64の折返し |
| `CsrI64` + BFS/SCC/topological | build後はread-onlyとして利用 | V長flat配列またはorder | 有向辺順と全graph検証を共有 |

将来の連結成分・Kruskal fixtureは、CSRをDSU内部へ埋めたり`DsuI64`の永続APIを変えたりせず、辺列を
順に`dsu_i64_merge`へ渡す利用例として始める。Fenwickを距離queueやpriority queueへ流用せず、用途の
違いを維持する。

## 6. 標準入力・scanner・outputの現状

### 6.1 既存host APIで分かったこと

- S-3は`print`を持たず、S-4は出力をhostの仕事にしている。
- standalone CLIの`vaak <file>`はfileを**Vaak source**として読み、最上位に残った値を表示する。
  利用者programがstdinを読むAPIではない。
- WASI `portable` binaryがstdinから読むのもVaak sourceであり、競技入力streamではない。
- `HostBinding`は実行前snapshotと実行後writebackを、`HostFn`は型付き同期callを既に提供する。
- host値は最上位scopeにだけ見えるので、library関数へは`str alias`等で明示的に渡す。
- C-31により最上位結果の解釈はhost側にある。したがってoutput `str`を値として返し、hostが一度書く形は
  新しい言語意味を要しない。

### 6.2 今回実装できたpure層

`io_ascii_i64_read(source, var position)`はASCII whitespaceを飛ばし、任意の`+`/`-`付きi64を読む。
空token、数字以外を含むtoken、overflow、cursor範囲外はparadoxであり、失敗時cursorを動かさない。
成功時は数字直後へ進む。`i64::MIN`は負のaccumulatorで扱い、中間値をoverflowさせない。

`io_ascii_i64_append(var output, value)`は十進ASCIIを`str`へ追記し、space/newline helperも持つ。
formatterも`i64::MIN`を正のi64へ反転せず処理する。各整数はO(桁数)、scannerは読んだbyte数に線形である。

P1第五checkpointでは`read_n_into`を加え、caller-owned配列へ指定個数を一つのframeで埋められるようにした。
`format_range`は範囲全体を一度に整形し、整数ごとの`reversed`配列に代えて20-byte scratchを一度だけ持つ。
どちらも既存の一括input/output手順の内側だけであり、host APIや言語意味は増やさない。

想定host手順は次である。

1. hostがstdinを一度byte列として読む。
2. 既存host value/layoutを使って`input : str`を渡す。
3. Vaak側は`cursor : i64`を持ち、scannerへ`input`とcursorを渡す。
4. Vaak側はoutput `str`を構築して最上位に残す。
5. hostが結果を一度stdoutへ書く。

この手順の2と5を既存CLIの標準挙動へ加えることはapplication変更であるため、今回実装していない。

### 6.3 未決のI/O境界

- `stdin` / `stdout`等の標準host名と型をVaak projectが保証するか
- 全入力snapshot、chunk pull、同期HostFnのどれを標準競技runnerにするか
- EOF、OS error、非UTF-8、output errorをどのstable categoryへ写すか
- zero-copy host byte view、reserve、bulk append/writeをcoreへ足すか
- instruction/byte/output budgetとcancellationを誰が所有するか

これらは新しいprimitive/capability/S-nを要しうるため、未解決として意味論・host API所有者へ返す。

## 7. rich stdlibへの段階的roadmap

APIの綴りではなく、現行Vaakで意味と性能を一段ずつ検証できる順にする。

競技プログラミングはこの枝の主要用途の一つであり、最終inventoryは最小構成に絞らない。array、
data structure、graph、string、数値を異様なほど広く揃える。ただし各項目は、固定型source、独立oracle、
reference/VM/STEEL差分、計算量、空・範囲・overflow、bench、参照元/license記録が揃って初めて
「実装済み」とする。module/generic/callbackや新しいhost意味をroadmap上の数だけで先取りしない。

### 次のcheckpoint優先順位

| 順位 | まとまり | 具体的な順序 | 先に満たすgate |
|---:|---|---|---|
| P0 済 | 基準構造 | 通常/rollback/weighted DSU、通常/count Fenwick、min/max heap、deque、sum/min/max segtree、range-add-sum | 第三checkpointまでの差分試験とbench |
| P1 済 | 静的range・順序・低alloc I/O | **array sort/compress・sparse/disjoint・圧縮済みordered multiset・Fenwick range派生・bulk scanner/output済** | 固定i64、flat対照、paradox/empty表 |
| P2 | byte string・trie・基礎数値 | Z/prefix/KMP → flat byte trie → Aho-Corasick/Manacher → gcd/extgcd/isqrt/sieve/factorization →安全なmod算術 | `str`はbyte列、i64中間幅、allocation測定 |
| P3 | flat graph | CSR → BFS/DFS/topological sort → SCC → 2-SAT → Dijkstra → LCA/HLD | graph/resultをflat化、決定的tie-break |
| P4 | 高級構造・flow | wavelet matrix、persistent/rollback構造、Li Chao、maxflow、min-cost flow | memory上限、再帰深さ、overflow、backend差 |
| P5 | 数値・string加速 | modint、組合せ、matrix、NTT/convolution、suffix array/LCP上級版 | wideningまたは安全な純Vaak基準、accelerated backend契約 |

P1内では次の小checkpoint順にする。

1. 完了: `array/i64`のinsertion/heap/merge sort、sorted unique、座標圧縮。
2. 完了: `SparseMinI64` / `SparseMaxI64`のidempotent O(1) queryと、固定sum用disjoint sparse table。
   `gcd`版は`i64::MIN`の符号・identity契約を先に決めるため保留。
3. 完了: sorted uniqueなuniverseを構築時に受ける`OrderedMultisetI64`を、flat Fenwickのprefix countと
   k-th探索で作った。universe外mutationはparadox、空eraseはfalse、未知keyのcount/rankは通常queryとした。
   任意keyをonline追加するbalanced treeや乱数priorityは別候補とする。
4. 完了: rollback DSU、weighted/potential DSU、prefix countのlower-boundに加え、Fenwickの
   range-add/point-get（一配列）とrange-add/range-sum（二配列）を別型にした。全演算はi64で折り返す。
5. 完了: host名を増やさず、`read_n_into`が一つの関数frameからcaller-owned `i64 array`へ埋める。
   `format_range`は結果`str`と20-byte scratchを各一度だけ確保する。`str.reserve`、zero-copy host view、
   stdin/stdout runnerは未決のまま分ける。
6. 完了: UTF-8 JSONをflat documentへparse/serializeし、JSONLを有界chunk readerへした。numberはi64限定、
   error位置とbudget codeを型付きで返す。file capabilityとPraTeX固有schemaは別層に残す。
7. 完了: `DenseBitSetU32`を32-bit packed固定長集合として加えた。TeX文字class、game有限mask、競プロ集合を
   共通fixtureにし、u8-per-bitを実測で棄却、flat u32を性能対照に残した。domain adapterは別sourceとする。

P2のtrie/stringはUnicodeを暗黙に扱わない。最初のtrie候補はnode/edge/label/terminalをparallel arrayにした
byte版で、遷移O(degree)のmutable基準と、build後にedgeをsortしてbinary searchするfrozen版を比較する。
固定26文字版を作る場合も`AsciiLowerTrie`のようにalphabetを名前へ出す。prefix search、語数、erase、
Aho-Corasick failure linkを段階化し、書記素trieとは呼ばない。

基礎数値は、`gcd` / `lcm` / extended gcd、integer sqrt、prime sieve、smallest-prime-factor、factorization、
divisor列挙、pow/mod inverse/CRT/floor sum、固定modulus組合せの順を候補とする。現行Vaakに`u64`/`i128`が
無いため、乗算がi64を越えうるAPIは「折返した積をmodする」実装にしない。加算倍化による安全な
`mul_mod`、入力modulus制限、backend wideningの三案を同じvectorで比較してから公開契約を選ぶ。

### Phase A: 現行機能だけの基礎

- range check、copy/fill/reverse/rotate、linear/binary search、prefix/difference
- 実装済み: insertion/heap/merge sort、sorted unique、coordinate compression
- 候補: partition/select、permutation
- DSU、point/count/range-update Fenwick、min/max heap、deque
- 実装済み: 末尾paddingを0に保つ固定長`DenseBitSetU32`
- 実装済み: 圧縮済みi64 ordered multiset
- gcd/lcm、pow_mod、inv_mod、crt、floor_sum
- byte列のZ algorithm、prefix function
- ASCII i64 scanner/formatter、pure output builder

型ごとのsourceを正直に分け、`i64`版をgenericに見せない。mutation queryは既存DSU/Fenwick同様、
paradoxとstatusをAPIごとに明記する。

### Phase B: 固定演算specialization

- `segtree/i64/sum|min|max`（第二checkpoint済み）、`gcd`（候補）
- `lazy_segtree/i64/range_add_sum`（第二checkpoint済み）、`range_add_min|max`と`range_assign_*`（候補）
- 実装済み: fixed i64 min/max sparse table、sum disjoint sparse table
- sliding min/max、Dijkstra、LCA、heavy-light decomposition

AC Libraryのsegtree/lazysegtreeはmonoid/action callbackをtemplate parameterにする。Vaakではgeneric/callbackが
未完成であり、参照実装のfunction frame費用も大きい。最初は演算をhot loopへ直書きする固定版だけを
比較し、runtime callback APIやmodule specializationを先に確定しない。`max_right`/`min_left`は任意predicate
とidentity条件の表現が決まるまで固定predicate用途版も公開しない。

### Phase C: flat graph結果

- 実装済み: CSR builderとvertex/edge範囲検査
- 実装済み: SCCの`group_count` + `group_of : i64 array`
- 実装済み: SCC上のliteral/implication配置を使う2-SAT
- 実装済み: CSRを共有する単一始点BFSと辞書順最小topological sort
- 次checkpoint: 既存`DsuI64`と無向辺を組み合わせる連結成分・Kruskal用fixture
- その次: 非負i64距離のDijkstra、0/1重み固定の0-1 BFS
- 後続: LCA/HLD。maxflow/min-cost flowはPhase Dで状態契約を別に固定

入れ子配列を必須にせずflat resultを基礎にする。SCC番号のtopological方向と同点tie-break、parallel edge、
self-loop、空graphを明記する。再帰ではなく明示stackを基準にし、参照/VM/STEELで同じgroup assignmentまたは
同値なcanonical normalizationを比較する。

Dijkstraは距離加算overflowと到達不能sentinel、0-1 BFSは重み範囲違反、topological sortはcycle時の
paradox/statusのどちらを公開契約にするかを実装前に明記する。maxflowを急いで同じgraph型へ可変残余辺を
混ぜず、immutable CSRとmutable algorithm stateを分ける。

### Phase D: flow

- maxflow: capacity固定のi64版
- min-cost flow: capacity/costの幅、負cost、potential、overflowを先に固定
- edge inspection/change、複数回flowの状態契約

大規模workloadではpure Vaak参照版とaccelerated backendの両方が必要になりうる。同じfixtureと外向き契約を
持たせるが、backend primitiveをstdlib sourceのふりをさせない。

### Phase E: modint/convolution/string上級

- fixed modulus関数moduleと、値型modintのidentityを分けて検討
- dynamic modulusはrun/module/threadのどこへ属するかを先に決める
- NTT/arbitrary modulus convolutionは中間幅とoverflow契約後
- suffix array、LCP、Z、prefix function、rolling hash

現行Vaakには`u64`/`i128`が無い。modint/convolutionの中間積を`u32`の自然な折返しへ任せず、固定modulusの
i64正規化、widening primitive、backend accelerationを比較する。`str`はC-77どおりbyte列なので、suffix
array等はbyte版とinteger array版を分け、Unicode code point/書記素algorithmとは呼ばない。

## 8. AC Libraryを参照する範囲とlicense

AtCoder公式repositoryはAC Libraryを公式libraryと説明し、`atcoder` headerをCC0で公開している。

- [公式repository](https://github.com/atcoder/ac-library)
- [公式LICENSE](https://github.com/atcoder/ac-library/blob/master/LICENSE)
- [production documentation一覧](https://atcoder.github.io/ac-library/production/document_en/)
- [DSU](https://atcoder.github.io/ac-library/production/document_en/dsu.html)
- [Fenwick tree](https://atcoder.github.io/ac-library/production/document_en/fenwicktree.html)
- [Segtree](https://atcoder.github.io/ac-library/production/document_en/segtree.html)
- [Lazy segtree](https://atcoder.github.io/ac-library/production/document_en/lazysegtree.html)
- [SCC](https://atcoder.github.io/ac-library/production/document_en/scc.html)
- [Two-SAT](https://atcoder.github.io/ac-library/production/document_en/twosat.html)
- [Max flow](https://atcoder.github.io/ac-library/production/document_en/maxflow.html)
- [Min-cost flow](https://atcoder.github.io/ac-library/production/document_en/mincostflow.html)
- [Modint](https://atcoder.github.io/ac-library/production/document_en/modint.html)
- [Convolution](https://atcoder.github.io/ac-library/production/document_en/convolution.html)
- [String algorithms](https://atcoder.github.io/ac-library/production/document_en/string.html)
- [Math](https://atcoder.github.io/ac-library/production/document_en/math.html)

借りるのは分類、操作の分け方、constraint・empty・計算量を公開契約に含める慣行だけである。C++ header、
内部algorithm、test、document本文を転写・翻訳しない。今回のheap/deque/I/O sourceは公式仕様を見て独立に
書いたもので、AC Libraryにheap/deque/scanner APIがあると主張もしない。

ACLはordered multiset、trie、scanner、rollback/weighted DSUのAPIを提供する資料として扱わない。それらは
Vaakの既存配列・Fenwick・byte列から契約を独立に定義し、Rust等の標準containerを使う場合もtest oracleに
限定する。今後の各checkpointは、実装前に「URL、版またはcommit、取得日、license、参照した契約、転写して
いない範囲」を専用の
[`competitive-stdlib-license-ledger.md`](competitive-stdlib-license-ledger.md)へ追記する。license不明の
snippetや競プロ解説codeを入力sourceにしない。

CC0であっても、どの契約を参照したかをこの文書に残す。production docsは制約違反をundefinedとするが、
Vaak libraryはそれをそのまま採らず、paradox/status/runtime errorを各APIで明記する。Vaak repository自体の
licenseは`docs/LICENSING.md`どおりMITである。

## 9. 意味論所有者へ返す未決事項

1. module/import/export/re-exportの構文、source identity、named type identity。
2. named struct fieldをopaque/privateにするか。heap/deque不変条件をどこで守るか。
3. generic/callback/module specializationと固定演算source generationの境界。`max_right` / `min_left`の
   predicate、range assign action、折返し順序上のrange-add-min/maxを含む。
4. query/mutationごとのinvalid range、empty、resource failureの共通表。
5. stdlib sourceの配布・version・prepare/cache key・到達可能性単位。
6. `DequeI64`、flat ring、core place最適化後のどれを永続APIにするか。
7. 標準競技runnerのinput/output host名、型、streaming、error、capability。
8. modintのmodulus identityとconvolutionのwidening数値設計。
9. accelerated backendとpure reference sourceの選択・feature query。
10. ACL風のall-in-one facadeを持つか。未使用module除去と再export決定前には置かない。
11. 圧縮済みordered multiset候補へonline arbitrary key構造も併設するか。今回の固定universe契約は変更しない。
12. byte trieのmutable/frozen表現、alphabet固定版の数、public field不変条件をどこまで保証するか。
13. bulk scanner/output scratchの所有者と大きさ。host streaming、reserve、標準stdin/stdoutとは別に決める。
14. SCCの決定的なgroup番号まで永続APIにするか、同一成分partitionと位相方向だけを保証するか。
15. BFS/topological sortの到達不能・cycle、Dijkstraのoverflowを共通result型なしでどう表すか。

どれもこのcheckpointでは新しいS-nや言語意味へ昇格させない。
