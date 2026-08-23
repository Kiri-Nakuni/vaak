//! 可変配列をデータスタックとして使う費用を測る小さなベンチ。
//!
//! `cargo run --release --example bench_stack -- 20000`

use std::time::{Duration, Instant};
use vaak::interp::Eval;

fn answer<E: std::fmt::Debug>(result: Result<Eval, E>) -> i128 {
    match result {
        Ok(Eval::Value(value)) => value.as_int().expect("整数を返す"),
        other => panic!("実行に失敗した: {other:?}"),
    }
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn measure(mut run: impl FnMut() -> i128, expected: i128) -> Duration {
    assert_eq!(run(), expected, "ウォームアップの答え");
    let mut samples = Vec::with_capacity(7);
    for _ in 0..7 {
        let start = Instant::now();
        let value = run();
        samples.push(start.elapsed());
        assert_eq!(value, expected);
        std::hint::black_box(value);
    }
    median(samples)
}

fn main() {
    let mut arguments = std::env::args().skip(1);
    let operations = arguments
        .next()
        .map(|arg| arg.parse::<i64>().expect("操作数は i64"))
        .unwrap_or(4_000);
    let mode = arguments.next().unwrap_or_else(|| "roundtrip".to_string());
    assert!(operations >= 0);
    let (source, expected) = match mode.as_str() {
        "push" => (
            format!(
                "var stack : i64 array := new i64 array(0, 0);
                 nfor (i, 0, {operations}) {{ stack.push(i); }};
                 stack.len()"
            ),
            i128::from(operations),
        ),
        "roundtrip" => (
            format!(
                "var stack : i64 array := new i64 array(0, 0);
                 nfor (i, 0, {operations}) {{ stack.push(i); }};
                 var total := 0;
                 nfor (i, 0, {operations}) {{ total += stack.pop() ?? 0; }};
                 total"
            ),
            i128::from(operations) * i128::from(operations - 1) / 2,
        ),
        _ => panic!("測定は `push` または `roundtrip`"),
    };
    let program = vaak::parser::parse(&source).expect("ベンチを解析できる");
    let errors: Vec<_> = vaak::check::check(&program)
        .into_iter()
        .chain(vaak::types::check_types(&program))
        .map(|error| error.msg)
        .collect();
    assert!(errors.is_empty(), "ベンチの静的エラー: {errors:#?}");
    let bytecode = vaak::vm::compile(&program).expect("VM へ翻訳できる");
    let reference = measure(
        || answer(vaak::interp::Interp::new().run(&program)),
        expected,
    );
    let vm = measure(|| answer(vaak::vm::run_program(&bytecode)), expected);

    println!("{mode}: {operations} 要素（解析・翻訳を除く、7 回の中央値）");
    println!("参照実装: {:.3} ms", reference.as_secs_f64() * 1_000.0);
    println!("VM:       {:.3} ms", vm.as_secs_f64() * 1_000.0);
}
