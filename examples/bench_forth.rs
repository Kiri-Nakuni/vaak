//! Vaak で書いた Forth 系処理系を、参照実装と VM で繰り返し走らせる。
//!
//! `cargo run --release --example bench_forth -- 1000`

use std::time::{Duration, Instant};
use vaak::interp::Eval;

const EXAMPLE: &str = "examples/vaak/07-Forth.vaak";

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
    let repetitions = std::env::args()
        .nth(1)
        .map(|arg| arg.parse::<usize>().expect("反復数は usize"))
        .unwrap_or(1_000);
    let source = std::fs::read_to_string(EXAMPLE).expect("Forth の例を読める");
    let program = vaak::parser::parse(&source).expect("Forth の例を解析できる");
    let errors: Vec<_> = vaak::check::check(&program)
        .into_iter()
        .chain(vaak::types::check_types(&program))
        .map(|error| error.msg)
        .collect();
    assert!(errors.is_empty(), "Forth の静的エラー: {errors:#?}");
    let bytecode = vaak::vm::compile(&program).expect("VM へ翻訳できる");
    let expected = 42 * repetitions as i128;

    let reference = measure(
        || {
            (0..repetitions)
                .map(|_| answer(vaak::interp::Interp::new().run(&program)))
                .sum()
        },
        expected,
    );
    let vm = measure(
        || {
            (0..repetitions)
                .map(|_| answer(vaak::vm::run_program(&bytecode)))
                .sum()
        },
        expected,
    );

    println!("Forth 主例を {repetitions} 回（解析・翻訳を除く、7 回の中央値）");
    println!(
        "参照実装: {:.3} ms ({:.1} us/run)",
        reference.as_secs_f64() * 1_000.0,
        reference.as_secs_f64() * 1_000_000.0 / repetitions as f64
    );
    println!(
        "VM:       {:.3} ms ({:.1} us/run)",
        vm.as_secs_f64() * 1_000.0,
        vm.as_secs_f64() * 1_000_000.0 / repetitions as f64
    );
}
