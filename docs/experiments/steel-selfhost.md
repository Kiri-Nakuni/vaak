# STEEL self-hosting と pure Vaak interpreter 実験

実施日: 2026-08-25

枝: `codex3/steel-selfhost`

状態: full self-host compiler は未完成。arithmetic compiler sliceは成立し、指定どおり
numeric bytecode interpreter sliceへ切り替えた。

これは言語決定ではない。`src/`、C-n、S-n、参照実装の意味を変更していない。

## 1. STEELをVaakで書く試み

[`steel_subset_compiler.vaak`](../../examples/experiments/steel_subset_compiler.vaak) は
ASCIIの非負i64 literal、空白、括弧、単項`-`、二項`+ - *`を再帰下降で読み、LLVM IRの
`main`を`str`として返す。Vaakのi64と同じ折返し演算であり、file入出力は持たない。
埋め込みhostが入力sourceをセルとして渡し、返ったIRを書く契約を想定した。

成立した範囲:

- 参照実装とVMがbyte単位で同じIRを返す。
- 返ったIRをclangで実行し、優先順位、単項負号、i64 overflowを確認した。
- compiler source自身を現行Rust STEELでLLVM IRへ落とし、nativeで一回実行した。
- `str`を返す最小の関数・alias・一byte pushは三実装で一致した。

再現:

```bash
cargo test --release --locked --test steel_selfhost -- --test-threads=1
```

### full Rust STEEL相当と呼ばない理由

Rust `src/steel.rs`は約5,000行あり、全Vaak lexer/parser、名前・型検査済みAST、関数、
構造体・配列・str・map/hash、paradox、領域、静的/動的escape、`outward`、runtime preludeを
扱う。今回完成したのはarithmetic expression一領域だけで、置換物ではない。したがって
「self-host STEEL完成」とせず、compiler性能比較も採用判断に使わない。

表現能力について、現時点でcore上の不可能性は証明されなかった。

- 再帰構造体が無くても、既存の
  [`selfhost-arena.md`](selfhost-arena.md)とJSON codec同様のtag＋平行配列でASTを表せる。
- sum型は`kind : i64`とpayload配列、callbackは`switch` dispatchで代用できる。
- module/generic/第一級関数が無いため実装量と重複は増えるが、表現不能ではない。
- pure Vaakはfile/argv/stdout capabilityを持たないためCLI全体にはhostが必須だが、
  `source str -> IR str`というcompiler library自体は表現できる。

## 2. 棄却したnested compiler実行

compilerをtop-levelから直接一回呼ぶ形は成功する。しかし同じcompilerを別のVaak関数から
一回呼ぶだけのfixtureは、参照実装とVMがchecksum **83**を返す一方、現行Rust STEELの
native生成物が **signal 11 (SIGSEGV)** で終了した。loop回数や性能測定以前の問題なので、
「native内部でcompilerを反復してns/compileを測る」案を棄却した。

最小driverは
[`steel_subset_compile_nested_probe.vaak`](../../examples/experiments/steel_subset_compile_nested_probe.vaak)
であり、compiler sourceを前置きする。失敗を通常test suiteへ混ぜず、手動reproducerを
ignored testとして残した。

```bash
cargo test --release --locked --test steel_selfhost \
  compilerを別のvaak関数から呼ぶnative_probe -- --ignored --exact --nocapture
```

2026-08-25のraw結果:

```text
nested-compiler-probe: native signal=Some(11)
left: None
right: Some(83)
```

これは「Vaakにcompilerを書けない」という意味論上の結論ではなく、現行STEEL backendの
nested heap return/lifetime経路にある実装上の阻害である。`src/steel.rs`は意味論・STEEL
担当者の範囲なので、この枝では修正せずfixtureと観測だけを残した。

## 3. fallback: Rust STEELでnative化するpure Vaak interpreter

[`steel_vaak_bytecode_interpreter.vaak`](../../examples/experiments/steel_vaak_bytecode_interpreter.vaak)
はVaak sourceで書かれ、現行Rust STEELでnative化できる。Rust実装の転記ではなく、固定の
numeric bytecodeを`while`で辿る独立実装である。

範囲:

- `(ok, i64)`平行stack。`ok=false`はparadox。
- `const/paradox/load/store`、i64 `add/sub/mul/div/mod/eq/lt`。
- 絶対`jump`、`jump_if_false`、値層の`coalesce`、`pop`、`halt`。
- slot数とstep数をcallerが上限指定する。
- 不正budget、pc、opcode、operand、stack、slot、jump、step上限、halt shapeを
  stable error code 1〜9で返す。

これはfull Vaak interpreterではない。source parser、型検査、関数、集合体、領域・escapeは
未実装であり、numeric control-flow vertical sliceと呼ぶ。

同じbytecode/inputを、test内の独立Rust oracle、木を辿る参照実装上のpure Vaak、bytecode VM
上のpure Vaak、Rust STEEL native生成物の四経路へ渡した。fixtureは算術14、slot更新36、
分岐11、paradox coalesce 42、1000周の和499500、およびerror 1〜9で一致した。

```bash
cargo test --release --locked --test steel_selfhost \
  numeric_bytecode -- --test-threads=1 --nocapture
```

## 4. 同一bytecode性能

同じ34-cell bytecodeで`0..1,000,000`を加算する。実行opcodeは **13,000,010**。
Rust側はtestと同じ独立oracle、Vaak側は上のpure Vaak interpreterをRust STEEL＋clang `-O2`
でnative化したもの。source parse、check/type-check、Rust STEEL compile、clangは全て測定外。
native側だけprocess起動中央値を別の空STEEL executableで測って差し引いた。

再現command（CPU 2へ固定）:

```bash
taskset -c 2 cargo run --quiet --release --locked \
  --example bench_steel_selfhost -- 1000000
```

連続3回のraw値:

| run | Rust oracle | Vaak/STEEL raw | process起動 | Vaak/STEEL補正後 | 表示ratio |
|---:|---:|---:|---:|---:|---:|
| 1 | 62,293,770 ns | 91,290,692 ns | 457,167 ns | 90,833,525 ns | 1.46 |
| 2 | 63,073,315 ns | 87,861,384 ns | 804,579 ns | 87,056,805 ns | 1.38 |
| 3 | 60,867,536 ns | 88,311,949 ns | 655,095 ns | 87,656,854 ns | 1.44 |

列ごとの中央値はRust **62.294 ms**、Vaak/STEEL補正後 **87.657 ms**、比 **1.41倍**。
opcode一つあたり約 **4.79 ns** 対 **6.74 ns** である。これはfull Vaak interpreterや
Rust `src/interp.rs`全体の比較ではなく、同じnumeric bytecode dispatch sliceだけの値である。

## 結論

- Vaak自身でLLVM IRを構築し、そのcompiler自身をSTEEL native化する最小縦切りは成立した。
- full STEEL置換は未完成なので完成扱い・性能優位の主張をしない。
- nested compiler呼出しはSIGSEGVのため棄却し、同じ測定を繰り返さないよう残した。
- fallback interpreterは四経路一致し、numeric dispatchでは独立Rust oracleの約1.41倍だった。
- 次にfull化する前提は、nested heap return fixtureをSTEEL担当枝で直すことと、flat AST・
  diagnostic・host I/O境界を別checkpointで固定することである。
