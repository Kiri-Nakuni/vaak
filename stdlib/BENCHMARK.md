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
