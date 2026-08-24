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
