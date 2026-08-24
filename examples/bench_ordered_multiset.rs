//! 圧縮済みi64 ordered multisetを同じuniverse上の線形count列と比較する。
//!
//! `ROUNDS=3 cargo run --release --locked --example bench_ordered_multiset`。
//! parse、静的検査、VM compileは計測外に置く。

use std::hint::black_box;
use std::time::{Duration, Instant};
use vaak::interp::Eval;

const ORDERED_MULTISET: &str = include_str!("../stdlib/ds/ordered_multiset_i64.vaak");

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

fn universe() -> &'static str {
    r#"
        var keys : i64 array := new i64 array(256, 0);
        nfor (rank, 0, keys.len()) {
            keys[rank] := rank * 2 - 255;
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
            name: "線形count列でrankとk-th",
            source: format!(
                r#"{}
                    var counts : i64 array := new i64 array(keys.len(), 0);
                    nfor (step, 0, 1024) {{
                        let rank := (step * 73 + 19) mod keys.len();
                        counts[rank] += 1;
                    }};
                    var checksum := 0;
                    nfor (query, 0, 2048) {{
                        let value := ((query * 41) mod 289) * 2 - 288;
                        var rank := 0;
                        var less := 0;
                        while (rank < keys.len() && (keys[rank] ?? 0) < value) {{
                            less += counts[rank] ?? 0;
                            rank += 1;
                        }};
                        checksum += less;

                        var remaining := (query * 73) mod 1024;
                        var at := 0;
                        while (at < counts.len()) {{
                            let count := counts[at] ?? 0;
                            if (remaining < count) {{
                                checksum += keys[at] ?? 0;
                                at := counts.len();
                            }} else {{
                                remaining -= count;
                                at += 1;
                            }} fi;
                        }};
                    }};
                    checksum mod 251
                "#,
                universe()
            ),
        },
        Case {
            name: "OrderedMultisetI64でrankとk-th",
            source: format!(
                r#"{ORDERED_MULTISET}
                    {}
                    var set := ordered_multiset_i64_new(keys) ??
                        new OrderedMultisetI64(keys := [], fenwick := [], total := 0);
                    nfor (step, 0, 1024) {{
                        let rank := (step * 73 + 19) mod keys.len();
                        ordered_multiset_i64_insert(set, keys[rank] ?? 0) ?? false;
                    }};
                    var checksum := 0;
                    nfor (query, 0, 2048) {{
                        let value := ((query * 41) mod 289) * 2 - 288;
                        checksum += ordered_multiset_i64_order_of_key(set, value) ?? 0;
                        let index := (query * 73) mod 1024;
                        checksum += ordered_multiset_i64_kth(set, index) ?? 0;
                    }};
                    checksum mod 251
                "#,
                universe()
            ),
        },
    ];

    println!("rounds={rounds}（parse/check/VM compileは計測外）");
    println!("{:<42} {:>12} {:>12} {:>8}", "case", "tree", "VM", "value");
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
            "{:<42} {:>12?} {:>12?} {:>8}",
            case.name, tree, vm_time, reference_value
        );
    }
}
