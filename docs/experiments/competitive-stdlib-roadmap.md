# 競プロ向けpure Vaak標準ライブラリroadmap

> **状態：実験checkpoint。決定ではない。** 新しい構文、意味論、S-n、module API、host capabilityを
> この文書では確定しない。`docs/vaak/decisions.md`と
> [`modules-and-library.md`](modules-and-library.md)を変更するものでもない。

- 調査・実測日: 2026-08-24
- branch: `codex2/stdlib-graph`
- graph checkpoint base: `origin/codex2/stdlib-heap-deque` = `54e05ab938cbd52c1572d0c8afec53600640bead`
- inherited heap/deque base: `origin/codex2/full` = `7c5ccd706e4dc466dad45baf275c0b550c8bc777`
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

### Phase A: 現行機能だけの基礎

- range check、copy/fill/reverse/rotate、linear/binary search、prefix/difference
- insertion/heap/merge sort、partition/select、coordinate compression、permutation
- DSU、Fenwick、min/max heap、deque、bitset
- gcd/lcm、pow_mod、inv_mod、crt、floor_sum
- byte列のZ algorithm、prefix function
- ASCII i64 scanner/formatter、pure output builder

型ごとのsourceを正直に分け、`i64`版をgenericに見せない。mutation queryは既存DSU/Fenwick同様、
paradoxとstatusをAPIごとに明記する。

### Phase B: 固定演算specialization

- `segtree/i64/sum|min|max|gcd`
- `lazy_segtree/i64/range_add_sum|range_add_min|range_assign_min|max`
- sparse/disjoint sparse table
- sliding min/max、Dijkstra、LCA、heavy-light decomposition

AC Libraryのsegtree/lazysegtreeはmonoid/action callbackをtemplate parameterにする。Vaakではgeneric/callbackが
未完成であり、参照実装のfunction frame費用も大きい。最初は演算をhot loopへ直書きする固定版だけを
比較し、runtime callback APIやmodule specializationを先に確定しない。`max_right`/`min_left`も任意predicate
ではなく、単調性とidentityを名前で固定できる用途版から検討する。

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

CC0であっても、どの契約を参照したかをこの文書に残す。production docsは制約違反をundefinedとするが、
Vaak libraryはそれをそのまま採らず、paradox/status/runtime errorを各APIで明記する。Vaak repository自体の
licenseは`docs/LICENSING.md`どおりMITである。

## 9. 意味論所有者へ返す未決事項

1. module/import/export/re-exportの構文、source identity、named type identity。
2. named struct fieldをopaque/privateにするか。heap/deque不変条件をどこで守るか。
3. generic/callback/module specializationと固定演算source generationの境界。
4. query/mutationごとのinvalid range、empty、resource failureの共通表。
5. stdlib sourceの配布・version・prepare/cache key・到達可能性単位。
6. `DequeI64`、flat ring、core place最適化後のどれを永続APIにするか。
7. 標準競技runnerのinput/output host名、型、streaming、error、capability。
8. modintのmodulus identityとconvolutionのwidening数値設計。
9. accelerated backendとpure reference sourceの選択・feature query。
10. ACL風のall-in-one facadeを持つか。未使用module除去と再export決定前には置かない。
11. SCCの決定的なgroup番号まで永続APIにするか、同一成分partitionと位相方向だけを保証するか。
12. BFS/topological sortの到達不能・cycle、Dijkstraのoverflowを共通result型なしでどう表すか。

どれもこのcheckpointでは新しいS-nや言語意味へ昇格させない。
