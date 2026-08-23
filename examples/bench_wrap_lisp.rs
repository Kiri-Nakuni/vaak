//! `wrap TokenPos = i64` を入れる前後の LISP 例を同じ過程で測る。
//!
//! 基準版は、この実験枝の親である `codex/main` から読む。構文解析と VM 翻訳は
//! 測定の外に置き、実行だけを比べる。

use std::hint::black_box;
use std::process::Command;
use std::time::{Duration, Instant};

const EXAMPLE: &str = "examples/vaak/06-LISP.vaak";

fn parse(source: &str) -> vaak::ast::Program {
    let program = vaak::parser::parse(source).expect("LISP の例を解析できる");
    let errors: Vec<_> = vaak::check::check(&program)
        .into_iter()
        .chain(vaak::types::check_types(&program))
        .map(|error| error.msg)
        .collect();
    assert!(errors.is_empty(), "LISP の例: {errors:#?}");
    program
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn bench(rounds: usize, mut run: impl FnMut()) -> Duration {
    let mut samples = Vec::with_capacity(7);
    for _ in 0..7 {
        let start = Instant::now();
        for _ in 0..rounds {
            run();
        }
        samples.push(start.elapsed() / rounds as u32);
    }
    median(samples)
}

fn vm_shape(program: &vaak::vm::Program2) -> (usize, usize, usize) {
    let mut ops = 0;
    let mut make_struct = 0;
    let mut coerce = 0;
    for chunk in &program.chunks {
        ops += chunk.ops.len();
        for op in &chunk.ops {
            make_struct += usize::from(matches!(op, vaak::vm::Op::MakeStruct(_, _)));
            coerce += usize::from(matches!(op, vaak::vm::Op::Coerce(_)));
        }
    }
    (ops, make_struct, coerce)
}

fn main() {
    let baseline = Command::new("git")
        .args(["show", "codex/main:examples/vaak/06-LISP.vaak"])
        .output()
        .expect("git show を呼べる");
    assert!(baseline.status.success(), "codex/main から基準版を読める");
    let baseline = String::from_utf8(baseline.stdout).expect("基準版は UTF-8");
    let wrapped = std::fs::read_to_string(EXAMPLE).expect("wrap 版を読める");

    let baseline_program = parse(&baseline);
    let wrapped_program = parse(&wrapped);
    let baseline_vm = vaak::vm::compile(&baseline_program).expect("基準版を VM へ翻訳できる");
    let wrapped_vm = vaak::vm::compile(&wrapped_program).expect("wrap 版を VM へ翻訳できる");

    // 一度走らせて遅延初期化と誤った比較を測定から外す。
    for program in [&baseline_program, &wrapped_program] {
        black_box(
            vaak::interp::Interp::new()
                .run(program)
                .expect("参照実装で走る"),
        );
    }
    for program in [&baseline_vm, &wrapped_vm] {
        black_box(vaak::vm::run_program(program).expect("VM で走る"));
    }

    let reference_rounds = 20;
    let vm_rounds = 1_000;
    let reference_baseline = bench(reference_rounds, || {
        black_box(
            vaak::interp::Interp::new()
                .run(black_box(&baseline_program))
                .expect("参照実装で走る"),
        );
    });
    let reference_wrapped = bench(reference_rounds, || {
        black_box(
            vaak::interp::Interp::new()
                .run(black_box(&wrapped_program))
                .expect("参照実装で走る"),
        );
    });
    let vm_baseline = bench(vm_rounds, || {
        black_box(vaak::vm::run_program(black_box(&baseline_vm)).expect("VM で走る"));
    });
    let vm_wrapped = bench(vm_rounds, || {
        black_box(vaak::vm::run_program(black_box(&wrapped_vm)).expect("VM で走る"));
    });

    let baseline_ir = vaak::steel::compile(&baseline_program)
        .unwrap_or_else(|error| panic!("基準版を STEEL へ翻訳できる: {}", error.msg));
    let wrapped_ir = vaak::steel::compile(&wrapped_program)
        .unwrap_or_else(|error| panic!("wrap 版を STEEL へ翻訳できる: {}", error.msg));

    println!("参照実装  基準 {reference_baseline:?} / wrap {reference_wrapped:?}");
    println!("VM        基準 {vm_baseline:?} / wrap {vm_wrapped:?}");
    println!(
        "VM 命令形 基準 {:?} / wrap {:?} (総命令, MakeStruct, Coerce)",
        vm_shape(&baseline_vm),
        vm_shape(&wrapped_vm)
    );
    println!(
        "STEEL IR  基準 {} bytes / wrap {} bytes / 完全一致 {}",
        baseline_ir.len(),
        wrapped_ir.len(),
        baseline_ir == wrapped_ir
    );
}
