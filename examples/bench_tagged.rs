//! sum 型と match 構文を足さず、tag + switch の配置だけを比べる。
//! 加えて `array[i].field := value` の既知の二乗経路を独立に測る。

use std::hint::black_box;
use std::time::{Duration, Instant};

use vaak::ast::Program;
use vaak::interp::{Eval, Interp};
use vaak::vm::{self, Program2, Runner};

const CASES: &[(&str, &str)] = &[
    (
        "inline switch",
        include_str!("experiments/tagged_inline.vaak"),
    ),
    (
        "named function",
        include_str!("experiments/tagged_function.vaak"),
    ),
    (
        "parallel arrays",
        include_str!("experiments/tagged_parallel_arrays.vaak"),
    ),
    (
        "struct array",
        include_str!("experiments/tagged_struct_array.vaak"),
    ),
];

fn prepare(src: &str) -> (Program, Program2) {
    let prog = vaak::parser::parse(src).expect("構文");
    let mut errs = vaak::check::check(&prog);
    errs.extend(vaak::types::check_types(&prog));
    assert!(
        errs.is_empty(),
        "静的検査: {:?}",
        errs.iter().map(|e| &e.msg).collect::<Vec<_>>()
    );
    let bytecode = vm::compile(&prog).expect("VM 組み立て");
    (prog, bytecode)
}

fn instantiate(src: &str, item_count: usize, pass_count: usize) -> String {
    src.replace(
        "let item_count := 200;",
        &format!("let item_count := {item_count};"),
    )
    .replace(
        "let pass_count := 2;",
        &format!("let pass_count := {pass_count};"),
    )
}

fn value(ev: Eval) -> String {
    match ev {
        Eval::Value(v) => v.show(),
        Eval::Paradox(_) => "paradox".into(),
        Eval::Akasha => "akasha".into(),
        Eval::Escape(_) => "escape".into(),
    }
}

fn median(mut xs: Vec<Duration>) -> Duration {
    xs.sort_unstable();
    xs[xs.len() / 2]
}

fn sample(mut f: impl FnMut() -> String, samples: usize) -> (Duration, String) {
    let warm = f();
    let mut answer = warm;
    let mut times = Vec::with_capacity(samples);
    for _ in 0..samples {
        let started = Instant::now();
        answer = black_box(f());
        times.push(started.elapsed());
    }
    (median(times), answer)
}

fn run_pair(prog: &Program, bytecode: &Program2, samples: usize) -> (Duration, Duration, String) {
    let (tree, tree_answer) = sample(
        || value(Interp::new().run(black_box(prog)).expect("tree runtime")),
        samples,
    );
    let mut runner = Runner::new();
    let (vm, vm_answer) = sample(
        || {
            value(
                runner
                    .run(black_box(bytecode), Vec::new())
                    .expect("VM runtime")
                    .0,
            )
        },
        samples,
    );
    assert_eq!(tree_answer, vm_answer, "backend の答えが違う");
    (tree, vm, tree_answer)
}

fn nested_write_source(item_count: usize) -> String {
    format!(
        r#"
        struct Cell {{ var tag : i64 := 0; }};
        var cells := new Cell array({item_count}, new Cell());
        nfor (i, 0, cells.len()) {{ cells[i].tag := i; }};
        cells[cells.len() - 1].tag
        "#
    )
}

fn scalar_write_source(item_count: usize) -> String {
    format!(
        r#"
        var cells := new i64 array({item_count}, 0);
        nfor (i, 0, cells.len()) {{ cells[i] := i; }};
        cells[cells.len() - 1]
        "#
    )
}

fn parse_arg(index: usize, default: usize) -> usize {
    std::env::args()
        .nth(index)
        .map(|x| x.parse().expect("引数は正整数"))
        .unwrap_or(default)
}

fn main() {
    let item_count = parse_arg(1, 1_000);
    let pass_count = parse_arg(2, 8);
    let samples = parse_arg(3, 5);
    assert!(item_count > 0 && pass_count > 0 && samples > 0);

    println!(
        "tag + switch: {item_count} 要素を {pass_count} 回走査。parse/check/VM compile は測定外。中央値。\n"
    );
    println!(
        "{:<18} {:>14} {:>14} {:>12}",
        "配置", "tree ms", "VM ms", "VM/tree"
    );

    let mut expected = None;
    for (name, template) in CASES {
        let src = instantiate(template, item_count, pass_count);
        let (prog, bytecode) = prepare(&src);
        let (tree, vm, answer) = run_pair(&prog, &bytecode, samples);
        match &expected {
            None => expected = Some(answer.clone()),
            Some(want) => assert_eq!(want, &answer, "配置で答えが違う: {name}"),
        }
        println!(
            "{name:<18} {:>14.3} {:>14.3} {:>12.3}",
            tree.as_secs_f64() * 1e3,
            vm.as_secs_f64() * 1e3,
            vm.as_secs_f64() / tree.as_secs_f64(),
        );
    }
    println!("\nchecksum {}", expected.unwrap());

    println!("\n入れ子左辺の診断: scalar `a[i] := i` と struct `a[i].tag := i`。各 1 回構築。");
    println!(
        "{:>8} {:>14} {:>14} {:>14} {:>14}",
        "N", "tree scalar", "tree nested", "VM scalar", "VM nested"
    );
    for n in [100usize, 200, 400, 800] {
        let (scalar_prog, scalar_vm) = prepare(&scalar_write_source(n));
        let (nested_prog, nested_vm) = prepare(&nested_write_source(n));
        let (tree_scalar, vm_scalar, scalar_answer) = run_pair(&scalar_prog, &scalar_vm, samples);
        let (tree_nested, vm_nested, nested_answer) = run_pair(&nested_prog, &nested_vm, samples);
        assert_eq!(scalar_answer, (n - 1).to_string());
        assert_eq!(nested_answer, scalar_answer);
        println!(
            "{n:>8} {:>11.3} ms {:>11.3} ms {:>11.3} ms {:>11.3} ms",
            tree_scalar.as_secs_f64() * 1e3,
            tree_nested.as_secs_f64() * 1e3,
            vm_scalar.as_secs_f64() * 1e3,
            vm_nested.as_secs_f64() * 1e3,
        );
    }
}
