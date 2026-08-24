//! range-update Fenwickを同じworkloadの生配列線形処理と比較する。
//!
//! `ROUNDS=3 cargo run --release --locked --example bench_fenwick_range`。
//! parse、静的検査、VM compileは計測外に置く。

use std::collections::BTreeMap;
use std::hint::black_box;
use std::time::{Duration, Instant};
use vaak::interp::Eval;

const RANGE_FENWICK: &str = include_str!("../stdlib/ds/fenwick_range_i64.vaak");

struct Case {
    name: &'static str,
    source: String,
    paired_checksum: &'static str,
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

fn main() {
    let rounds = std::env::var("ROUNDS")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|rounds| *rounds > 0)
        .unwrap_or(3);

    let cases = [
        Case {
            name: "生配列へrange addしてpoint get",
            source: r#"
                var values : i64 array := new i64 array(256, 0);
                nfor (update, 0, 2048) {
                    let first := (update * 29) mod 193;
                    let last := first + 64;
                    let value := (update * 17) mod 31 - 15;
                    var at := first;
                    while (at < last) { values[at] += value; at += 1; };
                };
                var checksum := 0;
                nfor (query, 0, 4096) {
                    checksum += values[(query * 73) mod 256] ?? 0;
                };
                checksum mod 251
            "#
            .into(),
            paired_checksum: "point",
        },
        Case {
            name: "RangeAddPointFenwickI64でpoint get",
            source: format!(
                r#"{RANGE_FENWICK}
                    var tree := range_add_point_fenwick_i64_new(256) ??
                        new RangeAddPointFenwickI64(data := []);
                    nfor (update, 0, 2048) {{
                        let first := (update * 29) mod 193;
                        let last := first + 64;
                        let value := (update * 17) mod 31 - 15;
                        range_add_point_fenwick_i64_range_add(
                            tree, first, last, value
                        ) ?? false;
                    }};
                    var checksum := 0;
                    nfor (query, 0, 4096) {{
                        checksum += range_add_point_fenwick_i64_get(
                            tree, (query * 73) mod 256
                        ) ?? 0;
                    }};
                    checksum mod 251
                "#
            ),
            paired_checksum: "point",
        },
        Case {
            name: "生配列へrange addしてrange sum",
            source: r#"
                var values : i64 array := new i64 array(256, 0);
                nfor (update, 0, 2048) {
                    let first := (update * 29) mod 193;
                    let last := first + 64;
                    let value := (update * 17) mod 31 - 15;
                    var at := first;
                    while (at < last) { values[at] += value; at += 1; };
                };
                var checksum := 0;
                nfor (query, 0, 2048) {
                    let first := (query * 41) mod 209;
                    var at := first;
                    while (at < first + 48) {
                        checksum += values[at] ?? 0;
                        at += 1;
                    };
                };
                checksum mod 251
            "#
            .into(),
            paired_checksum: "sum",
        },
        Case {
            name: "RangeAddSumFenwickI64でrange sum",
            source: format!(
                r#"{RANGE_FENWICK}
                    var tree := range_add_sum_fenwick_i64_new(256) ??
                        new RangeAddSumFenwickI64(delta := [], weighted := []);
                    nfor (update, 0, 2048) {{
                        let first := (update * 29) mod 193;
                        let last := first + 64;
                        let value := (update * 17) mod 31 - 15;
                        range_add_sum_fenwick_i64_range_add(
                            tree, first, last, value
                        ) ?? false;
                    }};
                    var checksum := 0;
                    nfor (query, 0, 2048) {{
                        let first := (query * 41) mod 209;
                        checksum += range_add_sum_fenwick_i64_range_sum(
                            tree, first, first + 48
                        ) ?? 0;
                    }};
                    checksum mod 251
                "#
            ),
            paired_checksum: "sum",
        },
    ];

    println!("rounds={rounds}（parse/check/VM compileは計測外）");
    println!("{:<44} {:>12} {:>12} {:>8}", "case", "tree", "VM", "value");
    let mut paired = BTreeMap::new();
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
        if let Some(previous) = paired.insert(case.paired_checksum, reference_value) {
            assert_eq!(
                previous, reference_value,
                "{}: paired workloadのchecksum",
                case.paired_checksum
            );
        }
        println!(
            "{:<44} {:>12?} {:>12?} {:>8}",
            case.name, tree, vm_time, reference_value
        );
    }
}
