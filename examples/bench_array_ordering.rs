//! 固定i64 sortと座標圧縮を同一入力で小さく比較する。
//!
//! `ROUNDS=3 cargo run --release --locked --example bench_array_ordering`。
//! parse、静的検査、VM compileは計測外に置く。

use std::hint::black_box;
use std::time::{Duration, Instant};
use vaak::interp::Eval;

const BINARY: &str = include_str!("../stdlib/array/i64/search_binary.vaak");
const INSERTION: &str = include_str!("../stdlib/array/i64/sort/insertion.vaak");
const HEAP: &str = include_str!("../stdlib/array/i64/sort/heap.vaak");
const MERGE: &str = include_str!("../stdlib/array/i64/sort/merge.vaak");
const SORTED_UNIQUE: &str = include_str!("../stdlib/array/i64/partition/sorted_unique.vaak");
const COMPRESS: &str = include_str!("../stdlib/array/i64/compress.vaak");

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

fn generated_values(length: i64) -> String {
    format!(
        r#"
            var xs : i64 array := new i64 array({length}, 0);
            nfor (i, 0, xs.len()) {{
                xs[i] := ((i * 1009 + 97) mod 521) - 260;
            }};
        "#
    )
}

fn checksum() -> &'static str {
    r#"
        var checksum := 0;
        nfor (i, 0, xs.len()) {
            checksum += (xs[i] ?? 0) * ((i mod 31) + 1);
        };
        checksum mod 251
    "#
}

fn sort_case(name: &'static str, library: &str, function: &str) -> Case {
    Case {
        name,
        source: format!(
            "{library}\n{}\n{function}(xs) ?? false;\n{}",
            generated_values(256),
            checksum()
        ),
    }
}

fn main() {
    let rounds = std::env::var("ROUNDS")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|rounds| *rounds > 0)
        .unwrap_or(3);

    let compression_library = format!("{BINARY}\n{MERGE}\n{SORTED_UNIQUE}\n{COMPRESS}");
    let cases = [
        Case {
            name: "入力生成と順序checksum",
            source: format!("{}\n{}", generated_values(256), checksum()),
        },
        sort_case("insertion sort 256", INSERTION, "array_i64_insertion_sort"),
        sort_case("heap sort 256", HEAP, "array_i64_heap_sort"),
        sort_case("merge sort 256", MERGE, "array_i64_merge_sort"),
        Case {
            name: "座標圧縮 512",
            source: format!(
                r#"{compression_library}
                    {}
                    let compressed := array_i64_coordinate_compress(xs) ??
                        new CoordinateCompressionI64(unique_values := [0], ranks := [0]);
                    var checksum := coordinate_compression_i64_len(compressed);
                    nfor (i, 0, compressed.ranks.len()) {{
                        checksum += (compressed.ranks[i] ?? 0) * ((i mod 31) + 1);
                    }};
                    checksum mod 251
                "#,
                generated_values(512)
            ),
        },
    ];

    println!("rounds={rounds}（parse/check/VM compileは計測外）");
    println!("{:<30} {:>12} {:>12} {:>8}", "case", "tree", "VM", "value");
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
            "{:<30} {:>12?} {:>12?} {:>8}",
            case.name, tree, vm_time, reference_value
        );
    }
}
