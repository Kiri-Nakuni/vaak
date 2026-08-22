//! Vaak-on-Vaak 骨格の end-to-end 実行時間を再現する小さな測定器。

use std::hint::black_box;
use std::time::{Duration, Instant};

const EXAMPLE: &str = "examples/vaak/07-セルフホスト骨格.vaak";
const EXAMPLE_MARKER: &str = "% --- 実行例 ---";

fn guest_program(depth: usize) -> (vaak::ast::Program, i128) {
    let example = std::fs::read_to_string(EXAMPLE).expect("セルフホスト例を読める");
    let core = example
        .split_once(EXAMPLE_MARKER)
        .expect("実行例の境界がある")
        .0;
    let mut expression = "1".to_owned();
    for _ in 0..depth {
        expression = format!("({expression} + 1)");
    }
    let source = format!(
        "{core}\nlet input := {expression:?};\n\
         let result := compile_and_run(input);\n\
         if (result.ok) result.value else 0 fi"
    );
    let program = vaak::parser::parse(&source).expect("生成した例を解析できる");
    let errors: Vec<_> = vaak::check::check(&program)
        .into_iter()
        .chain(vaak::types::check_types(&program))
        .map(|error| error.msg)
        .collect();
    assert!(errors.is_empty(), "生成した例: {errors:#?}");
    (program, depth as i128 + 1)
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn integer(result: vaak::interp::Eval) -> Option<i128> {
    match result {
        vaak::interp::Eval::Value(value) => value.as_int(),
        _ => None,
    }
}

fn main() {
    let depth = std::env::args()
        .nth(1)
        .map(|value| value.parse().expect("深さは非負整数"))
        .unwrap_or(64usize);
    let sample_count = std::env::args()
        .nth(2)
        .map(|value| value.parse().expect("標本数は正整数"))
        .unwrap_or(7usize);
    assert!(sample_count > 0, "標本数は正整数");

    let (program, expected) = guest_program(depth);
    let bytecode = vaak::vm::compile(&program).expect("VM へ翻訳できる");

    let mut reference_samples = Vec::with_capacity(sample_count);
    let mut vm_samples = Vec::with_capacity(sample_count);
    for _ in 0..sample_count {
        let started = Instant::now();
        let result = vaak::interp::Interp::new()
            .run(black_box(&program))
            .expect("参照実装で走る");
        reference_samples.push(started.elapsed());
        assert_eq!(integer(result), Some(expected));

        let started = Instant::now();
        let result = vaak::vm::run_program(black_box(&bytecode)).expect("VM で走る");
        vm_samples.push(started.elapsed());
        assert_eq!(integer(result), Some(expected));
    }

    let reference = median(reference_samples);
    let vm = median(vm_samples);
    let tokens = depth * 4 + 1;
    let nodes = depth * 2 + 1;
    let visits = depth * 3 + 1;
    println!("左深さ: {depth}");
    println!("tokens / nodes / visits: {tokens} / {nodes} / {visits}");
    println!(
        "参照実装 中央値: {:.3} ms",
        reference.as_secs_f64() * 1_000.0
    );
    println!("VM       中央値: {:.3} ms", vm.as_secs_f64() * 1_000.0);
    println!(
        "VM / 参照実装: {:.3}",
        vm.as_secs_f64() / reference.as_secs_f64()
    );
}
