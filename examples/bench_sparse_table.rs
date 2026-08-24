//! 固定i64 sparse tableを同じrange workloadの線形走査と比較する。
//!
//! `ROUNDS=3 cargo run --release --locked --example bench_sparse_table`。
//! parse、静的検査、VM compileは計測外に置く。

use std::hint::black_box;
use std::time::{Duration, Instant};
use vaak::interp::Eval;

const SPARSE: &str = include_str!("../stdlib/ds/sparse_table_i64.vaak");
const DISJOINT: &str = include_str!("../stdlib/ds/disjoint_sparse_table_i64.vaak");

struct Case {
    name: &'static str,
    source: String,
}

fn prepare(case: &Case) -> (vaak::ast::Program, vaak::vm::Program2) {
    let program = vaak::parser::parse(&case.source)
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

fn values() -> &'static str {
    r#"
        var xs : i64 array := new i64 array(512, 0);
        nfor (i, 0, xs.len()) {
            xs[i] := ((i * 1009 + 97) mod 521) - 260;
        };
    "#
}

fn main() {
    let rounds = std::env::var("ROUNDS")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|rounds| *rounds > 0)
        .unwrap_or(3);

    let cases = [
        Case {
            name: "64幅minを線形走査",
            source: format!(
                r#"{}
                    var checksum := 0;
                    nfor (query, 0, 4096) {{
                        let first := (query * 29) mod 449;
                        let last := first + 64;
                        var minimum := 9223372036854775807;
                        var at := first;
                        while (at < last) {{
                            let current := xs[at] ?? 0;
                            if (current < minimum) minimum := current; fi;
                            at += 1;
                        }};
                        checksum += minimum;
                    }};
                    checksum mod 251
                "#,
                values()
            ),
        },
        Case {
            name: "SparseMinI64で64幅min",
            source: format!(
                r#"{SPARSE}
                    {}
                    let table := sparse_min_i64_from(xs) ??
                        new SparseMinI64(length := 0, levels := 0, data := []);
                    var checksum := 0;
                    nfor (query, 0, 4096) {{
                        let first := (query * 29) mod 449;
                        checksum += sparse_min_i64_prod(table, first, first + 64) ?? 0;
                    }};
                    checksum mod 251
                "#,
                values()
            ),
        },
        Case {
            name: "64幅sumを線形走査",
            source: format!(
                r#"{}
                    var checksum := 0;
                    nfor (query, 0, 4096) {{
                        let first := (query * 29) mod 449;
                        let last := first + 64;
                        var at := first;
                        while (at < last) {{
                            checksum += xs[at] ?? 0;
                            at += 1;
                        }};
                    }};
                    checksum mod 251
                "#,
                values()
            ),
        },
        Case {
            name: "DisjointSparseSumI64で64幅sum",
            source: format!(
                r#"{DISJOINT}
                    {}
                    let table := disjoint_sparse_sum_i64_from(xs) ??
                        new DisjointSparseSumI64(length := 0, levels := 0, data := []);
                    var checksum := 0;
                    nfor (query, 0, 4096) {{
                        let first := (query * 29) mod 449;
                        checksum += disjoint_sparse_sum_i64_prod(table, first, first + 64) ?? 0;
                    }};
                    checksum mod 251
                "#,
                values()
            ),
        },
    ];

    println!("rounds={rounds}（parse/check/VM compileは計測外）");
    println!("{:<38} {:>12} {:>12} {:>8}", "case", "tree", "VM", "value");
    for case in &cases {
        let (program, vm) = prepare(case);
        let (tree, reference_value) = measure(
            || {
                value(
                    vaak::interp::Interp::new()
                        .run(&program)
                        .map_err(|error| error.msg),
                )
            },
            rounds,
        );
        let (vm_time, vm_value) = measure(|| value(vaak::vm::run_program(&vm)), rounds);
        assert_eq!(reference_value, vm_value, "{}: 参照とVM", case.name);
        println!(
            "{:<38} {:>12?} {:>12?} {:>8}",
            case.name, tree, vm_time, reference_value
        );
    }
}
