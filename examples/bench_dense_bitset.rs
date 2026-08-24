//! 同じ固定長集合workloadをu8-per-bit、flat u32、named u32で比較する。
//!
//! `ROUNDS=5 cargo run --release --locked --example bench_dense_bitset`。
//! parse、静的検査、VM compileは計測外に置く。

use std::hint::black_box;
use std::time::{Duration, Instant};
use vaak::interp::Eval;

const BITSET: &str = vaak::stdlib::DENSE_BITSET_U32;

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

fn median(mut run: impl FnMut() -> i128, rounds: usize) -> (Duration, i128) {
    let expected = black_box(run());
    let mut samples = Vec::with_capacity(rounds);
    for _ in 0..rounds {
        let started = Instant::now();
        assert_eq!(black_box(run()), expected);
        samples.push(started.elapsed());
    }
    samples.sort_unstable();
    (samples[samples.len() / 2], expected)
}

fn main() {
    let rounds = std::env::var("ROUNDS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|rounds| *rounds > 0)
        .unwrap_or(5);

    let cases = [
        Case {
            name: "u8-per-bit 素朴配列",
            source: r#"
                let length := 2051;
                var left : u8 array := new u8 array(length, 0);
                var right : u8 array := new u8 array(length, 0);
                var target : u8 array := new u8 array(length, 0);
                nfor (index, 0, length) {
                    if (index mod 3 == 0 || index mod 11 == 0) {
                        left[index] := 1 -> u8;
                    } fi;
                    if (index mod 5 == 0 || index mod 7 == 0) {
                        right[index] := 1 -> u8;
                    } fi;
                };
                var checksum := 0;
                nfor (round, 0, 128) {
                    var count := 0;
                    nfor (index, 0, length) {
                        let a := left[index] ?? (0 -> u8);
                        let b := right[index] ?? (0 -> u8);
                        var value : u8 := a;
                        if (round mod 4 == 0) value := a | b;
                        elif (round mod 4 == 1) value := a & b;
                        elif (round mod 4 == 2) value := a ^ b;
                        else value := a & ((255 -> u8) ^ b);
                        fi;
                        target[index] := value;
                        if (value != (0 -> u8)) count += 1; fi;
                    };
                    checksum += count;
                };
                checksum mod 251
            "#
            .into(),
        },
        Case {
            name: "u32 word flat融合loop",
            source: r#"
                let length := 2051;
                let word_count := 65;
                var left : u32 array := new u32 array(word_count, 0);
                var right : u32 array := new u32 array(word_count, 0);
                var target : u32 array := new u32 array(word_count, 0);
                nfor (index, 0, length) {
                    let word_index := index / 32;
                    let mask : u32 := (1 -> u32) << (index mod 32);
                    if (index mod 3 == 0 || index mod 11 == 0) {
                        left[word_index] := (left[word_index] ?? (0 -> u32)) | mask;
                    } fi;
                    if (index mod 5 == 0 || index mod 7 == 0) {
                        right[word_index] := (right[word_index] ?? (0 -> u32)) | mask;
                    } fi;
                };
                var checksum := 0;
                nfor (round, 0, 128) {
                    var count := 0;
                    nfor (word_index, 0, word_count) {
                        let a := left[word_index] ?? (0 -> u32);
                        let b := right[word_index] ?? (0 -> u32);
                        var value : u32 := a;
                        if (round mod 4 == 0) value := a | b;
                        elif (round mod 4 == 1) value := a & b;
                        elif (round mod 4 == 2) value := a ^ b;
                        else value := a & ((4294967295 -> u32) ^ b);
                        fi;
                        target[word_index] := value;
                        count += value.count_ones();
                    };
                    checksum += count;
                };
                checksum mod 251
            "#
            .into(),
        },
        Case {
            name: "u32 word flat三走査",
            source: r#"
                let length := 2051;
                let word_count := 65;
                var left : u32 array := new u32 array(word_count, 0);
                var right : u32 array := new u32 array(word_count, 0);
                var target : u32 array := new u32 array(word_count, 0);
                nfor (index, 0, length) {
                    let word_index := index / 32;
                    let mask : u32 := (1 -> u32) << (index mod 32);
                    if (index mod 3 == 0 || index mod 11 == 0) {
                        left[word_index] := (left[word_index] ?? (0 -> u32)) | mask;
                    } fi;
                    if (index mod 5 == 0 || index mod 7 == 0) {
                        right[word_index] := (right[word_index] ?? (0 -> u32)) | mask;
                    } fi;
                };
                var checksum := 0;
                nfor (round, 0, 128) {
                    nfor (word_index, 0, word_count) {
                        target[word_index] := left[word_index] ?? (0 -> u32);
                    };
                    nfor (word_index, 0, word_count) {
                        let a := target[word_index] ?? (0 -> u32);
                        let b := right[word_index] ?? (0 -> u32);
                        if (round mod 4 == 0) target[word_index] := a | b;
                        elif (round mod 4 == 1) target[word_index] := a & b;
                        elif (round mod 4 == 2) target[word_index] := a ^ b;
                        else target[word_index] := a & ((4294967295 -> u32) ^ b);
                        fi;
                    };
                    var count := 0;
                    nfor (word_index, 0, word_count) {
                        count += (target[word_index] ?? (0 -> u32)).count_ones();
                    };
                    checksum += count;
                };
                checksum mod 251
            "#
            .into(),
        },
        Case {
            name: "DenseBitSetU32 named型",
            source: format!(
                r#"{BITSET}
                    let length := 2051;
                    var left := dense_bitset_u32_new(length) ??
                        new DenseBitSetU32(length := -1, words := []);
                    var right := dense_bitset_u32_new(length) ??
                        new DenseBitSetU32(length := -1, words := []);
                    var target := dense_bitset_u32_new(length) ??
                        new DenseBitSetU32(length := -1, words := []);
                    nfor (index, 0, length) {{
                        if (index mod 3 == 0 || index mod 11 == 0) {{
                            dense_bitset_u32_insert(left, index) ?? false;
                        }} fi;
                        if (index mod 5 == 0 || index mod 7 == 0) {{
                            dense_bitset_u32_insert(right, index) ?? false;
                        }} fi;
                    }};
                    var checksum := 0;
                    nfor (round, 0, 128) {{
                        dense_bitset_u32_copy_from(target, left) ?? false;
                        if (round mod 4 == 0) {{
                            dense_bitset_u32_union_into(target, right) ?? false;
                        }} elif (round mod 4 == 1) {{
                            dense_bitset_u32_intersection_into(target, right) ?? false;
                        }} elif (round mod 4 == 2) {{
                            dense_bitset_u32_xor_into(target, right) ?? false;
                        }} else {{
                            dense_bitset_u32_difference_into(target, right) ?? false;
                        }} fi;
                        checksum += dense_bitset_u32_count_ones(target) ?? 0;
                    }};
                    checksum mod 251
                "#
            ),
        },
    ];

    println!(
        "length=2051 operation_rounds=128 rounds={rounds} median（parse/check/VM compileは計測外）"
    );
    println!("{:<30} {:>12} {:>12} {:>8}", "case", "tree", "VM", "value");
    let mut expected = None;
    for case in &cases {
        let (program, vm) = prepare(case);
        let (tree, reference_value) = median(
            || {
                value(
                    vaak::interp::Interp::new()
                        .run(&program)
                        .map_err(|error| error.msg),
                )
            },
            rounds,
        );
        let (vm_time, vm_value) = median(|| value(vaak::vm::run_program(&vm)), rounds);
        assert_eq!(reference_value, vm_value, "{}: 参照とVM", case.name);
        if let Some(expected) = expected {
            assert_eq!(reference_value, expected, "{}: workload結果", case.name);
        } else {
            expected = Some(reference_value);
        }
        println!(
            "{:<30} {:>12?} {:>12?} {:>8}",
            case.name, tree, vm_time, reference_value
        );
    }
}
