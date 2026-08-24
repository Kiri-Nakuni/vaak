# 配列ライブラリ試作の軽量測定

決定ではなく、2026-08-22 に `cargo run --release --example bench_array_library` を
同じ Windows 環境で走らせた一標本である。parse、静的検査、VM compile は計測外。
各プログラムを一度暖機した後の 5 回平均を示す。

| case | 木を辿る実装 | VM | 結果 |
|---|---:|---:|---:|
| `len` を直接 20,000 回読む | 10.99 ms | 5.83 ms | 157 |
| alias 関数枠から `len` を 20,000 回読む | 35.02 ms | 11.44 ms | 157 |
| 二分探索をその場に書く | 37.06 ms | 16.19 ms | 170 |
| 二分探索を alias 関数で呼ぶ | 51.02 ms | 20.91 ms | 170 |
| prefix 和を毎回線形走査する | 250.80 ms | 166.81 ms | 5 |
| `FenwickI64` 固定加算で prefix 和 | 244.04 ms | 186.97 ms | 5 |
| 生配列の flat Fenwick で prefix 和 | **62.51 ms** | **12.64 ms** | 5 |

絶対時間は負荷で動くため、同じ実行内の比だけを見る。

- ごく小さい操作を関数にすると、枠の費用が本体を上回る。細粒度 module は細粒度な
  **読み込み単位**であって、要素ごとの callback 単位にしてはならない。
- 二分探索まで仕事をまとめると、関数版の上乗せは木で約 1.38 倍、VM で約 1.29 倍まで薄まる。
- 同じ Fenwick 演算でも `tree.data[i]` と `data[i]` の差が大きい。現行の木を辿る実装は
  代入先の型と複合代入の現在値を得る際に根の値を複製し、VM は `SetIndex` / `SetField` で
  更新済み集合体を根まで組み直す。構造体で一欄包むだけでも hot update の経路に費用が出る。
- したがって「構造体版が遅いから Fenwick が不向き」とはまだ言えない。入れ子経路の
  fast path を直して同じベンチを再実行することが判断条件である。それまでは flat 版が
  現行意味論での性能上限、構造体版が名前付きデータ構造の書き味の対照になる。

固定演算版は combine callback を一度も呼ばない。それでも関数枠と場所の表現だけでこの差が出るため、
segment tree も最初は `sum` / `min` / `max` ごとの module とし、任意 callback 版を基準にしない。

## heap/deque checkpoint

2026-08-24に同じcommandへheap/dequeの1024要素workloadを足して5回平均を測った。

| case | 木を辿る実装 | VM | 結果 |
|---|---:|---:|---:|
| min heapへ1024 pushして全pop | 1.465 s | 295.67 ms | 140 |
| 生配列へ1024 pushして`remove(0)` | 5.65 ms | 2.92 ms | 187 |
| `DequeI64`へ1024 push_backして全pop_front | 591.20 ms | 76.87 ms | 187 |

ring dequeのpush/popはアルゴリズム上は償却O(1)だが、この大きさでは生配列のO(n)先頭removeより遅い。
`DequeI64`の各操作がnamed function枠を通り、`deque.data[i]`、`head`、`size`という複合経路を何度も
読む費用が支配しているためである。この一標本からring方式を棄却も、十分高速とも判断しない。
要素数を増やしたpaired測定、検査関数をhot pathから外した版、flat表現、入れ子place fast path後の
再測定を次の性能gateとする。heapも同じく、二分heapの計算量と現行struct経路の定数費用を分けて扱う。

## segment tree checkpoint

第二checkpointのAPIを加える前に既存caseを再実行し、named functionとcompound placeの費用が残ることを
確認した。その上でflat配列へquery loopをその場書きした対照、`SumSegtreeI64`、配列走査と
`RangeAddSumSegtreeI64`を同じ実行へ足した。2026-08-24、`ROUNDS=3`を設定した
`cargo run --release --locked --example bench_array_library`による、一度暖機後の3回平均である。

| case | 木を辿る実装 | VM | 結果 |
|---|---:|---:|---:|
| `len`を直接20,000回読む | 9.88 ms | 4.71 ms | 157 |
| alias関数枠から`len`を20,000回読む | 60.53 ms | 12.85 ms | 157 |
| 任意range和を毎回走査する | 326.25 ms | 227.95 ms | 112 |
| flat segment和のloopをその場に書く | **61.26 ms** | **28.86 ms** | 112 |
| `SumSegtreeI64.prod`で同じrange和 | 307.33 ms | 89.23 ms | 112 |
| 配列走査で128回のrange add/sum | **21.95 ms** | **7.92 ms** | 10 |
| lazy treeで同じrange add/sum | 338.65 ms | 59.55 ms | 10 |
| min heapへ1024 pushして全pop | 1.004 s | 358.91 ms | 140 |
| 生配列へ1024 pushして`remove(0)` | 4.42 ms | 2.39 ms | 187 |
| `DequeI64`へ1024 push_backして全pop_front | 467.98 ms | 33.63 ms | 187 |

`SumSegtreeI64`は線形走査より木でわずかに速く、VMで約2.55倍速い一方、flat loopより木で約5.0倍、
VMで約3.1倍遅い。現構文では`&=`を`tree.data`のような経路へ張れないため、named型のqueryは
compound readを通る。配列全体を局所値へ複製すればAPI上のO(log n)を失う可能性があるので採用しなかった。

lazy treeはアルゴリズム上O(log n)でも、128要素・固定32要素区間のこの標本では線形走査より木で約15倍、
VMで約7.5倍遅い。再帰named functionと`data`/`lazy`のcompound accessが支配している。これは
range-add-sumの意味・計算量試験を否定しないが、range assign等のvariantを同じ表現のまま増やす根拠にも
しない。第一checkpointのheap/ringの遅さも同じpaired表へ残した。最終判断は要素数crossover、flat表現、
compound place fast path後の再測定を必要とする。

## DSU / Fenwick派生checkpoint

2026-08-24、`ROUNDS=3 cargo run --release --locked --example bench_dsu_fenwick`を同じWindows環境で
実行した一度暖機後の3回平均である。parse、静的検査、VM compileは計測外。各programは構造を一度確保し、
query loop中には新しいVaak arrayを作らない。rollback workloadだけは最初のmerge/rollbackでhistoryを
必要長まで伸ばし、以後8 cycleでその容量を再利用する。この初回伸長も計測値に含む。

| case | 木を辿る実装 | VM | 結果 |
|---|---:|---:|---:|
| 通常DSUを256要素へ構築し4,096 find | 170.46 ms | 18.85 ms | 5 |
| rollback DSUの127 merge/rollbackを8 cycle | 145.68 ms | 93.60 ms | 20 |
| weighted DSUを128要素へ構築し2,048 diff | 169.04 ms | 38.53 ms | 0 |
| 256 countを線形走査して1,024 k-th | **101.18 ms** | 64.72 ms | 145 |
| `FenwickCountI64`で同じ1,024 k-th | 132.06 ms | **29.10 ms** | 145 |

累積順位の二caseは同じchecksumである。256要素ではcount Fenwickが線形走査に対して木で約1.31倍遅く、
VMで約2.22倍速い。O(log n)であってもnamed function/compound placeの定数費用は木を辿る実装で残るため、
これを永続的な性能保証やcrossoverとしない。要素数別のpaired測定とflat表現、place fast path後の再測定を
続ける。測定中allocationが少ないことはAPIのmemory上限を意味せず、各構築の配列とrollback historyはO(n)。
## graph checkpoint

2026-08-24に`cargo run --release --locked --example bench_graph_library`を同じWindows環境で実行した。
parse、静的検査、VM compileを計測外にし、一度暖機後の5回平均である。

| case | 木を辿る実装 | VM | 結果 |
|---|---:|---:|---:|
| 512頂点、約1100辺のCSR構築と反復SCC | 561.35 ms | 174.63 ms | 1成分 |
| 2048頂点の有向路をCSR構築して反復SCC | 2.087 s | 618.48 ms | 2048成分 |
| 1024頂点の有向路をCSR構築してBFS | 339.45 ms | 95.25 ms | 距離1023 |
| 1024頂点、約2045辺を安定topological sort | 906.01 ms | 267.36 ms | 1024頂点 |
| `CsrI64.to[i]`を32×1024回更新 | 110.93 ms | 44.78 ms | 232 |
| 生の`to[i]`を同じ回数更新 | 45.53 ms | 35.17 ms | 232 |

同じ更新workloadで複合place版はflat版に対し、木で約2.44倍、VMで約1.27倍だった。この対照では
graph全関数の実行時束縛費用を混ぜず、実際と同じ`CsrI64(start, to)`のstruct定義だけを前置きした。SCC本体は
`start`/`to`の読み取りが中心で、builderと結果をnamed structにする定数費用とalgorithmの計算量を
この比だけから分離はできない。fixtureではさらに4096頂点の有向路を通し、再帰深さではなく明示配列stackの
容量だけがVに比例することを確認した。同じ4096頂点路をBFSとtopological sortにも通し、いずれも
Vaakの再帰上限へ探索深さを重ねない。

この一標本はAPI採否や大規模性能の結論ではない。次は辺密度、成分形状、backend、flat引数版をpairedにし、
BFS/topological sortを同じCSRへ追加した後も、parse/check/compileと外部I/Oを実行時間へ混ぜず比較する。

## fixed i64 ordering checkpoint

2026-08-25、Ubuntu clang 18.1.3を持つLinux x86_64環境で
`ROUNDS=3 cargo run --release --locked --example bench_array_ordering`を実行した。一度暖機後の3回平均で、
parse、静的検査、VM compileは計測外である。三種のsortは同じ256要素の決定列を実行ごとに生成し、
同じ順序依存checksumを返す。入力生成だけのcaseも同じ表へ残す。

| case | 木を辿る実装 | VM | 結果 |
|---|---:|---:|---:|
| 256要素の入力生成と順序checksum | 1.037 ms | 0.384 ms | 22 |
| insertion sort 256 | 31.178 ms | 28.854 ms | 123 |
| heap sort 256 | 12.249 ms | 9.594 ms | 123 |
| merge sort 256 | **9.453 ms** | **7.264 ms** | 123 |
| 512要素の座標圧縮 | 37.070 ms | 27.475 ms | 101 |

この規模と分布ではmerge sortが三種で最短だったが、allocationを含む一標本であり、crossoverや永続的な
性能保証ではない。insertion sortはO(n²)、heap/mergeはO(n log n)で、heapだけ追加領域O(1)、mergeと
座標圧縮はO(n)の作業配列を持つ。要素数、既整列率、重複率を変えたpaired測定を次の判断材料にする。

## static sparse table checkpoint

2026-08-25、同じLinux x86_64環境で
`ROUNDS=3 cargo run --release --locked --example bench_sparse_table`を実行した。一度暖機後の3回平均で、
parse、静的検査、VM compileは計測外。全caseが同じ512要素を生成し、4096本の64幅rangeを読む。
table caseは各実行の先頭でO(n log n) buildを一度行い、その費用も含む。

| case | 木を辿る実装 | VM | 結果 |
|---|---:|---:|---:|
| 64幅minを線形走査 | 310.738 ms | 239.097 ms | 233 |
| `SparseMinI64`で同じmin | **288.312 ms** | **30.142 ms** | 233 |
| 64幅sumを線形走査 | **245.277 ms** | 234.269 ms | 86 |
| `DisjointSparseSumI64`で同じsum | 285.548 ms | **33.435 ms** | 86 |

VMではbuild込みでもminが約7.9倍、sumが約7.0倍速かった。木を辿る実装ではminが約1.08倍速い一方、sumは
約1.16倍遅く、O(1) queryでもnamed function枠と`table.data[index]`の複合読み取り費用が残る。
これはstatic tableの計算量試験を否定しないが、単一規模での採否やcrossoverを保証しない。query本数、幅、
要素数、buildを償却する回数を変え、flat引数版やcompound place fast path後とpairedにする。

## compressed ordered multiset checkpoint

2026-08-25、同じLinux x86_64環境で
`ROUNDS=3 cargo run --release --locked --example bench_ordered_multiset`を実行した。一度暖機後の3回平均で、
parse、静的検査、VM compileは計測外。両caseとも同じ256個のsorted unique universeを作り、同じ1024回の
insert後に`order_of_key`と0-based k-thを各2048回実行する。構築とinsertの費用も計測へ含む。

| case | 木を辿る実装 | VM | 結果 |
|---|---:|---:|---:|
| 線形count列でrankとk-th | 1.264 s | 1.157 s | 177 |
| `OrderedMultisetI64`で同じrankとk-th | **1.046 s** | **376.788 ms** | 177 |

この標本ではordered multisetが線形count列に対して木で約1.21倍、VMで約3.07倍速かった。固定universeの
Fenwick queryはO(log n)だが、named functionと`set.fenwick[index]`の複合place費用は残る。単一の256-key分布、
重複率、query比だけからcrossoverや永続的な性能を保証しない。universe規模、insert/erase比、偏った個数、
flat引数版、compound place fast path後を同じchecksumでpairedにして再測定する。

## range-update Fenwick checkpoint

2026-08-25、同じLinux x86_64環境で
`ROUNDS=3 cargo run --release --locked --example bench_fenwick_range`を実行した。一度暖機後の3回平均で、
parse、静的検査、VM compileは計測外。全caseは256要素へ同じ2048本の64幅range addを行う。point pairは
4096 point get、sum pairは2048本の48幅range sumを続け、構築・更新も計測へ含む。

| case | 木を辿る実装 | VM | 結果 |
|---|---:|---:|---:|
| 生配列へrange addしてpoint get | **246.822 ms** | 203.986 ms | 242 |
| `RangeAddPointFenwickI64`で同じpoint get | 584.952 ms | **180.567 ms** | 242 |
| 生配列へrange addしてrange sum | **567.481 ms** | 382.244 ms | 157 |
| `RangeAddSumFenwickI64`で同じrange sum | 820.061 ms | **253.662 ms** | 157 |

現行の木を辿る実装ではpoint型が線形処理の約2.37倍、sum型が約1.45倍の時間を要した。VMでは逆にpoint型が
約1.13倍、sum型が約1.51倍速い。FenwickのO(log n)操作でもnamed functionと一つまたは二つのcompound field
更新費用は消えないため、これを永続的なcrossoverやbackend保証にしない。要素数、range幅、update/query比、
flat引数版、compound place fast path後を同じchecksumでpairedにして再測定する。
