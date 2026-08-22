# LISP の token 位置を `wrap` する

**arena の採否を決める文書ではない。** `wrap` の比較・添字が未決定である現状のまま、
既存の [`06-LISP.vaak`](../../examples/vaak/06-LISP.vaak) の token 位置だけを
`TokenPos` に変えた対照実験である。core の意味論と `decisions.md` は変えていない。

## 結論

- `TokenPos` と環境配列などの一般の `i64` 添字は、関数の境界で混ざらなくなった。
- **実際に LISP や Forth を書いたとき、この取り違えで落ちた記録は無い。**
  ここで確認したのは将来の誤りを静的に止める利得であって、既存事故の修復ではない。
- この LISP は token 列を直接評価し、Node を確保しない。したがって arena の
  `.alloc`、Node handle、slot 更新の必要性を測る材料にはならない。
- 現行機能だけでも全体を型付けできるが、剥がし 43 回・包み 21 回が要る。
- 比較と添字だけを透過させても、位置の加算に剥がし 19 回が残る。
- STEEL は wrap 前後で LLVM IR が byte 単位に一致した。一方、参照実装と VM は
  wrap を一欄の構造体として実行するため、高頻度 cursor には実費がある。

## 何を包んだか

元の `Step.next` と `eval`、`skip`、`close_after` の位置はすべて `i64` だった。
実験版では次のようにした。

```vaak
wrap TokenPos = i64;

struct Step {
    let ok : u1 := false;
    let value : i64 := 0;
    let next : TokenPos := new TokenPos(0);
};

fn eval (tokens : str array alias, pos : TokenPos, ...) { ... } -> Step;
```

これにより、たとえば環境の添字をそのまま `eval` へ渡すコードは静的に落ちる。

```vaak
let environment_index : i64 := 3;
eval(tokens, environment_index, names, values);  % 型が合わない
```

[`tests/wrap_lisp.rs`](../../tests/wrap_lisp.rs) は、この拒否と三実行経路の答え 42 を
回帰試験にしている。ただし、これは起きた事故の再現ではなく意図的に作った反例である。

## 現行仕様で書いた量

基準は実験枝の親 `codex/main` にある同じ例である。

| 指標 | 基準 | `TokenPos` 版 |
|---|---:|---:|
| 全行数 | 318 | 352 |
| 差分 | — | 81 行追加、47 行削除、正味 +34 行 |
| `TokenPos -> i64` の剥がし | 0 | **43 回** |
| `new TokenPos(...)` の包み | 0 | **21 回** |
| wrap に関係する行 | 0 | 51 行 |

現在は比較・添字・算術のいずれにも包み型を直接使えないため、次の三つが繰り返される。

```vaak
if ((cursor -> i64) < tokens.len()) ... fi;       % 比較
let token := tokens[cursor -> i64];               % 添字
cursor := new TokenPos((cursor -> i64) + 1);       % 算術
```

型の境界は明瞭になったが、token cursor のように「比べる・読む・一つ進める」を繰り返す
値では、`wrap-vs-arena.md` の NodeId より剥がしの密度が高い。

## 比較と添字が透過しても残るもの

「`i64` を包む値は比較と配列添字に直接使える」とだけ仮定し、算術は今のままにすると、
43 回の剥がしのうち 24 回は消える。しかし次の **19 回**は位置の加算なので残る。

- 新しい `TokenPos` を作る加算: 15 箇所
- `pos + 1` や `pos + 2` を、そのまま比較・添字に使う加算: 4 箇所

包みは 21 回すべて残る。内訳は、加算結果を包む 15 回と、既定値・失敗番兵・開始位置を
作る 6 回である。したがって、**比較と添字の透過は読み味をかなり直すが、参照実装と VM の
動的な包み費用はほぼ直さない。**

位置ごとに次の自由関数を置けば、算術の剥がしと包みは実装一箇所へ寄せられる。

```vaak
fn advance (pos : TokenPos, amount : i64) {
    new TokenPos((pos -> i64) + amount)
} -> TokenPos;
```

比較・添字が透過する仮定では、19 箇所は `advance(pos, n)` になり、明示的な算術の剥がしは
この関数内の 1 回まで減る。これは `Meters + i64` のような演算をすべての wrap に暗黙許可せず、
「位置を進める」という領域固有の演算だけを名前で与える方法である。ただし今の参照実装と VM
では、呼び出すたびに一欄構造体を作る実費は残る。

## 実行費用

[`bench_wrap_lisp.rs`](../../examples/bench_wrap_lisp.rs) は、基準版を
`git show codex/main:examples/vaak/06-LISP.vaak` から読み、両方を先に解析・翻訳してから
実行だけを測る。release build、7 標本の中央値で、参照実装は各標本 20 回、VM は 1000 回である。

Windows の同一機上で二度続けて測った結果は次の通りだった。

| 経路 | 基準 | `TokenPos` | 読み方 |
|---|---:|---:|---|
| 参照実装 1 | 6.103 ms | 6.027 ms | -1% |
| 参照実装 2 | 6.056 ms | 7.180 ms | +19% |
| VM 1 | 1.338 ms | 2.568 ms | +92% |
| VM 2 | 1.258 ms | 1.690 ms | +34% |
| STEEL LLVM IR | 129,990 bytes | 129,990 bytes | **完全一致** |

時間は Windows の割り当て器と同居負荷で揺れるので、上の百分率を性能保証には使えない。
確かに言えるのは次の二点である。

1. STEEL では包みが基底 `i64` に解決され、同じ IR になった。コンパイル後の実行費用はゼロである。
2. VM の静的命令形は `(総命令, MakeStruct, Coerce)` が
   `(3023, 11, 58)` から `(3228, 33, 101)` へ増えた。追加の **22 MakeStruct と 43 Coerce** は
   現在の包み・剥がしを実行しており、VM の差は単なる型検査だけではない。

参照実装も `Value::Struct` 一欄で wrap を持つ。つまり S-2 の「置き場は変えない」は STEEL では
そのまま実現できるが、木辿りと VM における高頻度の wrap は、現実には zero-cost abstraction
ではない。cursor や handle を広く勧めるなら、比較・添字の構文だけでなく実行時表現も検討対象になる。

## この LISP は arena の根拠になるか

**ならない。** 少なくとも、この例から arena を足す理由は出ない。

- 構文木を作らず、`str array` と `TokenPos` を再帰関数で読み進めている。
- tokenizer の `tokens.push` は 4 箇所あるが、どれも返された位置をその場で必要としない。
  `.alloc(value) -> handle` にまとめる利得は 0 箇所である。
- 位置は「安定した object identity」ではなく、隣へ加算できる cursor である。
  arena handle より、範囲つき整数に近い。
- この実装の実際の費用は `let` ごとの環境の深い複製と名前の線形探索である。
  token arena はどちらも直さない。

将来、LISP を「種類・値・最初の子・次の兄弟」の Node 配列へ変えれば、その時点では
NodeId、atomic な allocation、直接 slot 更新を測れる。現在の token-stream evaluator を
arena の賛否に数えると、Node arena が解く問題をまだ持たない実装で評価することになる。

## 判断

1. `TokenPos` のような領域型は API 境界の安全性には効く。ただし、現状に実事故の記録は無い。
2. wrap の同型比較と整数添字は、NodeId と TokenPos の両方で剥がしを減らす。
3. cursor 算術は一律の暗黙演算より、`advance` のような領域ごとの名前付き操作が安全である。
4. interpreter/VM で高頻度に使うなら、包みを一欄構造体として都度作る実装費を無視できない。
5. arena は、Node を実際に確保・更新する LISP/self-host 実験で改めて判定すべきである。

## 再現

```bash
cargo test --release --test wrap_lisp
cargo run --release --example bench_wrap_lisp
```

