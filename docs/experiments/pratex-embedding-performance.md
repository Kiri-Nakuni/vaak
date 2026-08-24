# PraTeX prepared embedding の性能実験

更新日: 2026-08-25

これは非規範的な実装・測定記録である。Vaak の言語意味論、参照実装、C-n/S-n、
PraTeX 固有の TeX 意味論を変更しない。

## 対象

PraTeX が現在使っている `PreparedProgram` / `HostLayout` / `EmbeddingRunner` の形を模し、
prepare、host 値検査、検査済み token の実行、毎回の再束縛、raw VM を分けて測った。
再現用 program は [`bench_embedding_api.rs`](../../examples/bench_embedding_api.rs) である。

配列 fixture は `count` と `dimen` の二つを `i32 array[256]` として渡し、
`count[5] * 2 + count[6]` を実行する。これは PraTeX の register array と同じ値の形であり、
PraTeX のコードや TeX の処理は写していない。

| case | 測る境界 |
|---|---|
| `prepare-array` | parse/check/type-check/VM compile |
| `validate-array` | raw `Vec<Value>` から検査済み `HostValues` を作って戻す |
| `run-token-array` | 一度検査した token と runner を再利用する |
| `run-rebind-array` | PraTeX の現行形と同様、run ごとに token を作り直す |
| `run-raw-array` | 公開 embedding wrapper を通さない reusable VM |
| `run-token-scalar` | `i64` 一値の最小 warm run |

## 環境と手順

- Linux 7.0.0-30-generic x86-64
- Intel Core i7-8650U、CPU 3 に固定
- rustc 1.94.0、LLVM 21.1.8、release/locked build
- perf 7.0.12、`perf_event_paranoid=1`
- 基準: `36c28a0f6138aa223813378c3556165cae6effbd`（測定 program だけを追加）
- 各 wall time は 9 標本の中央値
- hardware counter は同じ 9 run の `perf stat` 集計値を反復数で割った

```text
cargo build --release --locked --example bench_embedding_api
taskset -c 3 perf stat -r 9 -e task-clock,cycles,instructions \
  env VAAK_BENCH_NO_ALLOC_STATS=1 \
  target/release/examples/bench_embedding_api CASE ITERATIONS
```

allocation は `VAAK_BENCH_NO_ALLOC_STATS` を付けず、同じ executable の counting allocator で測る。
wall time は同時負荷と CPU frequency の影響を受けるため、合否条件にはせず、命令数と allocation も併記する。

## 採用した変更

1. 子を持たない `ValueType` の妥当性検査と同値比較では、一要素の作業 `Vec` を作らない。
2. leaf scalar の平坦な array は、全要素を反復検査しつつ task `Vec` へ積まない。
3. 定数添字だけを読む leaf array も、選択した要素を直接反復検査する。
4. VM が正常 return または脱出 return で frame を外すとき、cell buffer を runner の pool へ戻す。

深い array/map/hash/名付き型は従来どおり明示 stack で検査する。深さ・node 数上限、内部型、
非有限浮動小数点、壊れた hash index を拒む検査は残している。公開 API は変更していない。

## 結果

| case | wall 中央値 before → after | instructions/iter before → after | allocation/iter before → after | bytes/iter before → after |
|---|---:|---:|---:|---:|
| `validate-array` | 6,078 ns → 3,814 ns (-37.2%) | 47,700 → 31,555 (-33.8%) | 8 → 0 | 16,512 → 0 |
| `run-rebind-array` | 7,284 ns → 4,424 ns (-39.3%) | 52,292 → 35,920 (-31.3%) | 9 → 0 | 16,528 → 0 |
| `run-token-array` | 569 ns → 559 ns (-1.9%) | 4,599 → 4,376 (-4.9%) | 1 → 0 | 16 → 0 |
| `run-token-scalar` | 317 ns → 253 ns (-20.3%) | 2,500 → 2,284 (-8.7%) | 1 → 0 | 16 → 0 |

基準の flat `perf record` では `validate_value_tasks` が 52.70%、`value_matches_type` が 30.97%、
`validate_finite_scalar` が 6.54% だった。16.5 KiB の大半は、二つの256要素 array を task stack へ
積むための領域だった。採用変更後は平坦な host 値検査と warm VM run の heap allocation が0になった。

永続 token は変更前から raw VM とほぼ同じ速さだった。したがって layout identity の照合を外す API は
必要ない。PraTeX の現行再束縛形では、VM 実行より全 array の再検査が支配的である。長寿命 token を
保持できる host はそれを再利用し、raw 値へ戻す必要がある host も今回の非割り当て検査を使える。

## 不採用にした実験

### leaf 値を専用 match へ分ける

平坦な array の各 scalar について、汎用 `value_matches_type` / `validate_finite_scalar` を通さず、
整数・文字列・有限 float を一つの専用 `match` で照合する案を試した。deep/malformed な集合体だけは
従来経路へ戻す形だった。

しかし、`validate-array` 300,000反復、`perf stat -r 5` で instructions が
9.46 billion から 10.07 billion へ **6.5%増えた**。同じ標本の task-clock も約1.07秒から
約1.32秒へ悪化した。型が固定された hot loop に追加した分岐と大きい match の code shape が、
汎用関数を呼ぶ費用より重くなったと推測する。

この案は撤回した。今後 leaf 検査を再検討する場合は、match の書き換えだけを繰り返さず、生成assembly、
PGO、型ごとの単相化、SIMD可能な別表現のいずれかを伴う独立実験にする。

## 結論

- parse/check/type-check/compile は従来どおり一度だけ行う。
- 現行 API のまま PraTeX 型の再束縛費用を約39%下げ、heap allocation を0にできる。
- 検査済み token の identity check はボトルネックではない。
- warm scalar run の allocation も0になったので、新しい unchecked API や runner scratch 公開契約は要らない。
- 性能改善は host 値検査と VM buffer lifecycle の実装変更であり、意味論上の決定を追加しない。
