//! pure Vaak graphの反復SCCと複合place費用を小さく測る。
//!
//! `cargo run --release --example bench_graph_library`。`ROUNDS`で反復数を変えられる。

use std::hint::black_box;
use std::time::{Duration, Instant};
use vaak::interp::Eval;

const GRAPH: &str = include_str!("../stdlib/graph/csr_scc_two_sat_i64.vaak");
const CSR_SHAPE: &str = r#"
    struct CsrI64 {
        var start : i64 array;
        var to : i64 array;
    };
"#;

struct Case {
    name: &'static str,
    library: &'static str,
    body: &'static str,
}

fn prepare(case: &Case) -> (vaak::ast::Program, vaak::vm::Program2) {
    let source = format!("{}\n{}", case.library, case.body);
    let program = vaak::parser::parse(&source)
        .unwrap_or_else(|error| panic!("{}: 構文: {}", case.name, error.msg));
    let errors: Vec<_> = vaak::check::check(&program)
        .into_iter()
        .chain(vaak::types::check_types(&program))
        .map(|error| error.msg)
        .collect();
    assert!(errors.is_empty(), "{}: {errors:?}", case.name);
    let vm = vaak::vm::compile(&program)
        .unwrap_or_else(|error| panic!("{}: VM: {}", case.name, error.msg));
    (program, vm)
}

fn value(result: Result<Eval, impl std::fmt::Debug>) -> i128 {
    match result {
        Ok(Eval::Value(value)) => value.as_int().expect("整数結果"),
        Ok(other) => panic!("値でない結果: {other:?}"),
        Err(error) => panic!("実行時: {error:?}"),
    }
}

fn measure(mut run: impl FnMut() -> i128, rounds: u32) -> (Duration, i128) {
    let expected = black_box(run());
    let start = Instant::now();
    for _ in 0..rounds {
        assert_eq!(black_box(run()), expected);
    }
    (start.elapsed() / rounds, expected)
}

fn main() {
    let rounds = std::env::var("ROUNDS")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|rounds| *rounds > 0)
        .unwrap_or(5);

    let cases = [
        Case {
            name: "CSR構築と反復SCC",
            library: GRAPH,
            body: r#"
                let n := 512;
                var builder := csr_i64_builder_new(n) ?? new CsrBuilderI64(vertex_count := 0, from := [], to := []);
                nfor (i, 0, n) {
                    csr_i64_builder_add_directed(builder, i, (i + 1) mod n) ?? false;
                    csr_i64_builder_add_directed(builder, i, (i * 37 + 11) mod n) ?? false;
                    if (i mod 7 == 0) csr_i64_builder_add_directed(builder, i, i) ?? false; fi;
                };
                let graph := csr_i64_build(builder) ?? new CsrI64(start := [0], to := []);
                let result := scc_i64(graph) ?? new SccResultI64(group_count := -1, group_of := []);
                result.group_count
            "#,
        },
        Case {
            name: "2048頂点の有向路SCC",
            library: GRAPH,
            body: r#"
                let n := 2048;
                var builder := csr_i64_builder_new(n) ?? new CsrBuilderI64(vertex_count := 0, from := [], to := []);
                nfor (i, 0, n - 1) {
                    csr_i64_builder_add_directed(builder, i, i + 1) ?? false;
                };
                let graph := csr_i64_build(builder) ?? new CsrI64(start := [0], to := []);
                let result := scc_i64(graph) ?? new SccResultI64(group_count := -1, group_of := []);
                result.group_count
            "#,
        },
        Case {
            name: "CsrI64.to複合place更新",
            // graph全関数の実行時束縛費用を混ぜず、実際と同じstruct形だけを前置きする。
            library: CSR_SHAPE,
            body: r#"
                let n := 1024;
                var graph := new CsrI64(start := new i64 array(n + 1, 0), to := new i64 array(n, 0));
                nfor (round, 0, 32) {
                    nfor (i, 0, n) { graph.to[i] += (i + round) & 7; };
                };
                var sum := 0;
                nfor (i, 0, n) { sum += graph.to[i] ?? 0; };
                sum mod 251
            "#,
        },
        Case {
            name: "生配列flat更新",
            library: "",
            body: r#"
                let n := 1024;
                var to := new i64 array(n, 0);
                nfor (round, 0, 32) {
                    nfor (i, 0, n) { to[i] += (i + round) & 7; };
                };
                var sum := 0;
                nfor (i, 0, n) { sum += to[i] ?? 0; };
                sum mod 251
            "#,
        },
    ];

    println!("rounds={rounds}（parse/check/VM compileは計測外）");
    println!("{:<30} {:>12} {:>12} {:>8}", "case", "tree", "VM", "value");
    for case in &cases {
        let (program, vm) = prepare(case);
        let (tree, a) = measure(
            || {
                value(
                    vaak::interp::Interp::new()
                        .run(&program)
                        .map_err(|error| error.msg),
                )
            },
            rounds,
        );
        let (vm_time, b) = measure(|| value(vaak::vm::run_program(&vm)), rounds);
        assert_eq!(a, b, "{}: 参照とVM", case.name);
        println!("{:<30} {:>12?} {:>12?} {:>8}", case.name, tree, vm_time, a);
    }
}
