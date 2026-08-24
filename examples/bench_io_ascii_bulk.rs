//! ASCII i64のper-token APIとbulk APIを同じ入出力で比較する。
//!
//! `ROUNDS=3 cargo run --release --locked --example bench_io_ascii_bulk`。
//! parse、静的検査、VM compileは計測外に置く。

use std::collections::BTreeMap;
use std::hint::black_box;
use std::time::{Duration, Instant};
use vaak::interp::Eval;

const ASCII_I64: &str = include_str!("../stdlib/io/ascii_i64.vaak");

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

fn benchmark_input() -> String {
    let mut input = String::new();
    for index in 0..2048_i64 {
        input.push_str(match index % 3 {
            0 => " ",
            1 => "\n",
            _ => "\t",
        });
        let value = (index * 104_729 + 97) % 200_003 - 100_001;
        input.push_str(&value.to_string());
    }
    input.push('\n');
    input
}

fn values() -> &'static str {
    r#"
        var values : i64 array := new i64 array(1024, 0);
        nfor (index, 0, values.len()) {
            values[index] := (index * 104729 + 97) mod 200003 - 100001;
        };
    "#
}

fn main() {
    let rounds = std::env::var("ROUNDS")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|rounds| *rounds > 0)
        .unwrap_or(3);
    let input = benchmark_input();

    let cases = [
        Case {
            name: "2048整数をper-token read",
            source: format!(
                r#"{ASCII_I64}
                    let input := {input:?};
                    var at := 0;
                    var values : i64 array := new i64 array(2048, 0);
                    nfor (index, 0, values.len()) {{
                        values[index] := io_ascii_i64_read(input, at) ?? 0;
                    }};
                    var checksum := 0;
                    nfor (index, 0, values.len()) {{
                        checksum := checksum * 1000003 + (values[index] ?? 0);
                    }};
                    checksum mod 251
                "#
            ),
            paired_checksum: "read",
        },
        Case {
            name: "2048整数をread_n_into",
            source: format!(
                r#"{ASCII_I64}
                    let input := {input:?};
                    var at := 0;
                    var values : i64 array := new i64 array(2048, 0);
                    io_ascii_i64_read_n_into(
                        input, at, values, 0, values.len()
                    ) ?? false;
                    var checksum := 0;
                    nfor (index, 0, values.len()) {{
                        checksum := checksum * 1000003 + (values[index] ?? 0);
                    }};
                    checksum mod 251
                "#
            ),
            paired_checksum: "read",
        },
        Case {
            name: "1024整数をper-value append",
            source: format!(
                r#"{ASCII_I64}
                    {}
                    var output := "";
                    nfor (index, 0, values.len()) {{
                        if (index > 0) io_ascii_i64_append_space(output) ?? false; fi;
                        io_ascii_i64_append(output, values[index] ?? 0) ?? false;
                    }};
                    var checksum := 0;
                    nfor (index, 0, output.len()) {{
                        checksum := checksum * 257 + ((output[index] ?? 0) -> i64);
                    }};
                    checksum mod 251
                "#,
                values()
            ),
            paired_checksum: "format",
        },
        Case {
            name: "1024整数をformat_range",
            source: format!(
                r#"{ASCII_I64}
                    {}
                    let output := io_ascii_i64_format_range(
                        values, 0, values.len(), 0x20
                    ) ?? "";
                    var checksum := 0;
                    nfor (index, 0, output.len()) {{
                        checksum := checksum * 257 + ((output[index] ?? 0) -> i64);
                    }};
                    checksum mod 251
                "#,
                values()
            ),
            paired_checksum: "format",
        },
    ];

    println!("rounds={rounds}（parse/check/VM compileは計測外）");
    println!("{:<38} {:>12} {:>12} {:>8}", "case", "tree", "VM", "value");
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
            "{:<38} {:>12?} {:>12?} {:>8}",
            case.name, tree, vm_time, reference_value
        );
    }
}
