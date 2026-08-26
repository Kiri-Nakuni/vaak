//! 固定長dense bitsetを三用途のfixture・独立oracle・三backendで照合する。

use std::fmt::Write as _;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use vaak::interp::Eval;

const BITSET: &str = include_str!("../stdlib/ds/dense_bitset_u32.vaak");
const NEGATIVE_CONSTRUCTOR_FALLBACK: &str =
    "let negative := dense_bitset_u32_new(-1) ?? new DenseBitSetU32(length := -7, words := []); if (negative.length == -7) 42 else 0 fi";
static TEMP_ID: AtomicU64 = AtomicU64::new(0);

fn source(body: &str) -> String {
    format!("{BITSET}\n{body}")
}

#[track_caller]
fn checked(body: &str) -> String {
    let source = source(body);
    let program = vaak::parser::parse(&source).expect("dense bitsetを含むsourceを解析できる");
    let errors: Vec<_> = vaak::check::check(&program)
        .into_iter()
        .chain(vaak::types::check_types(&program))
        .map(|error| format!("{} @{:?}", error.msg, error.span))
        .collect();
    assert!(errors.is_empty(), "静的検査: {errors:?}\n{body}");
    source
}

fn shape(result: Result<Eval, String>) -> String {
    match result {
        Ok(Eval::Value(value)) => format!("値 {}", value.show()),
        Ok(Eval::Paradox(_)) => "paradox".into(),
        Ok(Eval::Akasha) => "虚無".into(),
        Ok(Eval::Escape(_)) => "脱出".into(),
        Err(error) => format!("エラー {error}"),
    }
}

#[track_caller]
fn reference_and_vm(body: &str, expected: &str) {
    let source = checked(body);
    for (name, result) in [
        ("参照", vaak::interp::run(&source)),
        ("VM", vaak::vm::run(&source)),
    ] {
        assert_eq!(shape(result), expected, "{name}: {body}");
    }
}

#[track_caller]
fn steel_native(body: &str, expected_exit: i32) {
    let source = checked(body);
    let program = vaak::parser::parse(&source).expect("構文");
    let ir = vaak::steel::compile(&program).unwrap_or_else(|error| panic!("STEEL: {}", error.msg));
    if Command::new("clang").arg("--version").output().is_err() {
        return;
    }

    let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let directory =
        std::env::temp_dir().join(format!("vaak-dense-bitset-{}-{id}", std::process::id()));
    std::fs::create_dir(&directory).expect("専用一時directoryを作れる");
    let llvm = directory.join("program.ll");
    let executable = directory.join(if cfg!(windows) {
        "program.exe"
    } else {
        "program"
    });
    std::fs::write(&llvm, ir).expect("LLVM IRを書ける");
    let built = Command::new("clang")
        .arg("-O2")
        .arg("-o")
        .arg(&executable)
        .arg(&llvm)
        .output()
        .expect("clangを起動できる");
    if !built.status.success() {
        let message = String::from_utf8_lossy(&built.stderr).into_owned();
        let _ = std::fs::remove_dir_all(&directory);
        panic!("clang: {message}");
    }
    let status = Command::new(&executable)
        .status()
        .expect("STEEL生成物を実行できる");
    std::fs::remove_dir_all(&directory).expect("専用一時directoryを片付けられる");
    assert_eq!(
        status.code(),
        Some(expected_exit),
        "STEEL生成物の終了状態: {status:?}"
    );
}

#[track_caller]
fn all_backends(body: &str) {
    reference_and_vm(body, "値 42");
    steel_native(body, 42);
}

#[test]
fn sourceは単独で前置きできる() {
    reference_and_vm("42", "値 42");
    let source = checked("42");
    let program = vaak::parser::parse(&source).expect("構文");
    vaak::steel::compile(&program).unwrap_or_else(|error| panic!("STEEL: {}", error.msg));
}

#[test]
fn word境界と深い複製とpaddingを保つ() {
    let body = r#"
        var empty := dense_bitset_u32_new(0) ??
            new DenseBitSetU32(length := -1, words := [0]);
        var one := dense_bitset_u32_new(1) ??
            new DenseBitSetU32(length := -1, words := []);
        var thirty_one := dense_bitset_u32_new(31) ??
            new DenseBitSetU32(length := -1, words := []);
        var thirty_two := dense_bitset_u32_new(32) ??
            new DenseBitSetU32(length := -1, words := []);
        var thirty_three := dense_bitset_u32_new(33) ??
            new DenseBitSetU32(length := -1, words := []);

        dense_bitset_u32_insert(one, 0) ?? false;
        dense_bitset_u32_fill(thirty_one) ?? false;
        dense_bitset_u32_insert(thirty_two, 31) ?? false;
        dense_bitset_u32_insert(thirty_three, 0) ?? false;
        dense_bitset_u32_insert(thirty_three, 32) ?? false;
        var copied := thirty_three;
        dense_bitset_u32_insert(copied, 1) ?? false;
        dense_bitset_u32_complement_into(copied) ?? false;

        if (dense_bitset_u32_is_valid(empty) &&
            dense_bitset_u32_word_len(empty) == 0 &&
            dense_bitset_u32_is_empty(empty) &&
            dense_bitset_u32_is_valid(one) &&
            dense_bitset_u32_count_ones(one) == 1 &&
            dense_bitset_u32_is_valid(thirty_one) &&
            dense_bitset_u32_word_len(thirty_one) == 1 &&
            dense_bitset_u32_count_ones(thirty_one) == 31 &&
            dense_bitset_u32_is_valid(thirty_two) &&
            dense_bitset_u32_contains(thirty_two, 31) &&
            dense_bitset_u32_is_valid(thirty_three) &&
            dense_bitset_u32_word_len(thirty_three) == 2 &&
            dense_bitset_u32_count_ones(thirty_three) == 2 &&
            ! dense_bitset_u32_contains(thirty_three, 1) &&
            dense_bitset_u32_is_valid(copied) &&
            dense_bitset_u32_count_ones(copied) == 30 &&
            ! dense_bitset_u32_contains(copied, 0) &&
            ! dense_bitset_u32_contains(copied, 1) &&
            ! dense_bitset_u32_contains(copied, 32)) 42 else 0 fi
    "#;
    all_backends(body);
}

#[test]
fn 不正入力と長さ違いは変更前に拒否する() {
    let body = r#"
        var target := dense_bitset_u32_new(33) ??
            new DenseBitSetU32(length := -1, words := []);
        var other := dense_bitset_u32_new(34) ??
            new DenseBitSetU32(length := -1, words := []);
        dense_bitset_u32_insert(target, 0) ?? false;
        dense_bitset_u32_insert(target, 32) ?? false;
        dense_bitset_u32_insert(other, 1) ?? false;
        let before0 := target.words[0] ?? 0;
        let before1 := target.words[1] ?? 0;
        let bad_negative := dense_bitset_u32_insert(target, -1) ?? false;
        let bad_end := dense_bitset_u32_remove(target, 33) ?? false;
        let bad_union := dense_bitset_u32_union_into(target, other) ?? false;
        let bad_copy := dense_bitset_u32_copy_from(target, other) ?? false;
        if (! bad_negative && ! bad_end && ! bad_union && ! bad_copy &&
            target.words[0] == before0 && target.words[1] == before1 &&
            dense_bitset_u32_count_ones(target) == 2)
            42 else 0 fi
    "#;
    all_backends(body);

    let broken = r#"
        var set := new DenseBitSetU32(
            length := 33,
            words := [(1 -> u32), (2 -> u32)]
        );
        let before := set.words[0] ?? 0;
        let changed := dense_bitset_u32_insert(set, 2) ?? false;
        if (! dense_bitset_u32_is_valid(set) && ! changed &&
            set.words[0] == before) 42 else 0 fi
    "#;
    reference_and_vm(broken, "値 42");

    reference_and_vm(
        "let set := dense_bitset_u32_new(3) ?? new DenseBitSetU32(length := 0, words := []); dense_bitset_u32_contains(set, 3)",
        "paradox",
    );
    reference_and_vm(NEGATIVE_CONSTRUCTOR_FALLBACK, "値 42");
}

#[test]
fn steelの負長constructor回収は参照実装とvmに一致する() {
    reference_and_vm(NEGATIVE_CONSTRUCTOR_FALLBACK, "値 42");
    steel_native(NEGATIVE_CONSTRUCTOR_FALLBACK, 42);
}

#[test]
fn tex文字classを固定長集合として合成できる() {
    let body = r#"
        var letters := dense_bitset_u32_new(256) ??
            new DenseBitSetU32(length := -1, words := []);
        var digits := dense_bitset_u32_new(256) ??
            new DenseBitSetU32(length := -1, words := []);
        nfor (offset, 0, 26) {
            dense_bitset_u32_insert(letters, 65 + offset) ?? false;
            dense_bitset_u32_insert(letters, 97 + offset) ?? false;
        };
        nfor (offset, 0, 10) {
            dense_bitset_u32_insert(digits, 48 + offset) ?? false;
        };
        var word_class := letters;
        dense_bitset_u32_union_into(word_class, digits) ?? false;

        if (dense_bitset_u32_count_ones(letters) == 52 &&
            dense_bitset_u32_count_ones(digits) == 10 &&
            dense_bitset_u32_count_ones(word_class) == 62 &&
            dense_bitset_u32_is_subset(letters, word_class) &&
            dense_bitset_u32_is_subset(digits, word_class) &&
            ! dense_bitset_u32_intersects(letters, digits) &&
            dense_bitset_u32_contains(word_class, 48) &&
            dense_bitset_u32_contains(word_class, 90) &&
            dense_bitset_u32_contains(word_class, 122) &&
            ! dense_bitset_u32_contains(word_class, 64) &&
            dense_bitset_u32_first_set(word_class) == 48 &&
            dense_bitset_u32_next_set(word_class, 58) == 65 &&
            dense_bitset_u32_kth_set(word_class, 61) == 122) 42 else 0 fi
    "#;
    all_backends(body);
}

#[test]
fn lvminibvs有限解釈maskをsnapshot内で絞れる() {
    let body = r#"
        let remaining_indices := [1, 3, 4, 7, 9, 10];
        let glass_a_indices := [1, 4, 7, 8];
        let glass_b_indices := [3, 4, 9, 10];
        var remaining := dense_bitset_u32_from_indices(12, remaining_indices) ??
            new DenseBitSetU32(length := -1, words := []);
        let glass_a := dense_bitset_u32_from_indices(12, glass_a_indices) ??
            new DenseBitSetU32(length := -1, words := []);
        let glass_b := dense_bitset_u32_from_indices(12, glass_b_indices) ??
            new DenseBitSetU32(length := -1, words := []);

        var after_a := remaining;
        var after_b := remaining;
        dense_bitset_u32_intersection_into(after_a, glass_a) ?? false;
        dense_bitset_u32_intersection_into(after_b, glass_b) ?? false;
        var distinguished := after_a;
        dense_bitset_u32_xor_into(distinguished, after_b) ?? false;
        var eliminated := remaining;
        dense_bitset_u32_difference_into(eliminated, after_a) ?? false;

        if (dense_bitset_u32_count_ones(remaining) == 6 &&
            dense_bitset_u32_count_ones(after_a) == 3 &&
            dense_bitset_u32_count_ones(after_b) == 4 &&
            dense_bitset_u32_count_ones(distinguished) == 5 &&
            dense_bitset_u32_count_ones(eliminated) == 3 &&
            dense_bitset_u32_first_set(after_a) == 1 &&
            dense_bitset_u32_kth_set(after_a, 2) == 7 &&
            dense_bitset_u32_next_set(after_b, 5) == 9 &&
            dense_bitset_u32_is_subset(after_a, remaining) &&
            dense_bitset_u32_is_subset(after_b, remaining)) 42 else 0 fi
    "#;
    all_backends(body);
}

#[test]
fn 競プロ集合演算はword境界を越える() {
    let body = r#"
        var even := dense_bitset_u32_new(70) ??
            new DenseBitSetU32(length := -1, words := []);
        var divisible_by_three := dense_bitset_u32_new(70) ??
            new DenseBitSetU32(length := -1, words := []);
        nfor (value, 0, 70) {
            if (value mod 2 == 0) dense_bitset_u32_insert(even, value) ?? false; fi;
            if (value mod 3 == 0) {
                dense_bitset_u32_insert(divisible_by_three, value) ?? false;
            } fi;
        };
        var intersection := even;
        var union := even;
        var xor := even;
        var difference := even;
        dense_bitset_u32_intersection_into(intersection, divisible_by_three) ?? false;
        dense_bitset_u32_union_into(union, divisible_by_three) ?? false;
        dense_bitset_u32_xor_into(xor, divisible_by_three) ?? false;
        dense_bitset_u32_difference_into(difference, divisible_by_three) ?? false;

        if (dense_bitset_u32_count_ones(even) == 35 &&
            dense_bitset_u32_count_ones(divisible_by_three) == 24 &&
            dense_bitset_u32_count_ones(intersection) == 12 &&
            dense_bitset_u32_count_ones(union) == 47 &&
            dense_bitset_u32_count_ones(xor) == 35 &&
            dense_bitset_u32_count_ones(difference) == 23 &&
            dense_bitset_u32_first_set(intersection) == 0 &&
            dense_bitset_u32_next_set(intersection, 1) == 6 &&
            dense_bitset_u32_kth_set(intersection, 11) == 66 &&
            dense_bitset_u32_contains(union, 69) &&
            dense_bitset_u32_contains(union, 68) &&
            ! dense_bitset_u32_contains(union, 67)) 42 else 0 fi
    "#;
    all_backends(body);
}

#[derive(Clone, Copy)]
struct Deterministic(u64);

impl Deterministic {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 7;
        self.0 ^= self.0 >> 9;
        self.0 ^= self.0 << 8;
        self.0
    }
}

fn bool_literal(value: bool) -> &'static str {
    if value {
        "true"
    } else {
        "false"
    }
}

fn set_count(bits: &[bool]) -> usize {
    bits.iter().filter(|&&value| value).count()
}

fn first_from(bits: &[bool], from: usize) -> Option<usize> {
    bits.iter()
        .enumerate()
        .skip(from)
        .find_map(|(index, &value)| value.then_some(index))
}

#[test]
fn 決定的操作列をrust_vec_bool_oracleと三backendで照合する() {
    const LENGTH: usize = 97;
    let mut left = vec![false; LENGTH];
    let mut right = vec![false; LENGTH];
    let mut random = Deterministic(0x4249_5453_4554_5533);
    let mut body = String::from(
        "var left := dense_bitset_u32_new(97) ?? new DenseBitSetU32(length := -1, words := []);\n\
         var right := dense_bitset_u32_new(97) ?? new DenseBitSetU32(length := -1, words := []);\n\
         var ok := true;\n",
    );

    for step in 0..240 {
        let index = (random.next() % LENGTH as u64) as usize;
        match random.next() % 12 {
            0 | 1 => {
                let changed = !left[index];
                left[index] = true;
                writeln!(
                    body,
                    "if ((dense_bitset_u32_insert(left, {index}) ?? false) != {}) ok := false; fi;",
                    bool_literal(changed)
                )
                .unwrap();
            }
            2 => {
                let changed = left[index];
                left[index] = false;
                writeln!(
                    body,
                    "if ((dense_bitset_u32_remove(left, {index}) ?? false) != {}) ok := false; fi;",
                    bool_literal(changed)
                )
                .unwrap();
            }
            3 | 4 => {
                let value = random.next() & 1 != 0;
                let changed = right[index] != value;
                right[index] = value;
                writeln!(
                    body,
                    "if ((dense_bitset_u32_assign(right, {index}, {}) ?? false) != {}) ok := false; fi;",
                    bool_literal(value),
                    bool_literal(changed)
                )
                .unwrap();
            }
            5 => {
                left.clone_from(&right);
                body.push_str(
                    "if (! (dense_bitset_u32_copy_from(left, right) ?? false)) ok := false; fi;\n",
                );
            }
            6 => {
                for (left, right) in left.iter_mut().zip(&right) {
                    *left |= *right;
                }
                body.push_str(
                    "if (! (dense_bitset_u32_union_into(left, right) ?? false)) ok := false; fi;\n",
                );
            }
            7 => {
                for (left, right) in left.iter_mut().zip(&right) {
                    *left &= *right;
                }
                body.push_str(
                    "if (! (dense_bitset_u32_intersection_into(left, right) ?? false)) ok := false; fi;\n",
                );
            }
            8 => {
                for (left, right) in left.iter_mut().zip(&right) {
                    *left ^= *right;
                }
                body.push_str(
                    "if (! (dense_bitset_u32_xor_into(left, right) ?? false)) ok := false; fi;\n",
                );
            }
            9 => {
                for (left, right) in left.iter_mut().zip(&right) {
                    *left &= !*right;
                }
                body.push_str(
                    "if (! (dense_bitset_u32_difference_into(left, right) ?? false)) ok := false; fi;\n",
                );
            }
            10 => {
                for value in &mut left {
                    *value = !*value;
                }
                body.push_str(
                    "if (! (dense_bitset_u32_complement_into(left) ?? false)) ok := false; fi;\n",
                );
            }
            _ => {
                writeln!(
                    body,
                    "if ((dense_bitset_u32_contains(left, {index}) ?? false) != {}) ok := false; fi;",
                    bool_literal(left[index])
                )
                .unwrap();
            }
        }

        if step % 9 == 0 {
            let left_count = set_count(&left);
            let right_count = set_count(&right);
            let from = (random.next() % (LENGTH as u64 + 1)) as usize;
            let next = first_from(&left, from).map_or(-1, |value| value as i64);
            let first = first_from(&left, 0).map_or(-1, |value| value as i64);
            let rank = if left_count == 0 {
                0
            } else {
                (random.next() % left_count as u64) as usize
            };
            let kth = left
                .iter()
                .enumerate()
                .filter_map(|(index, &value)| value.then_some(index))
                .nth(rank)
                .map_or(-1, |value| value as i64);
            writeln!(
                body,
                "if (! dense_bitset_u32_is_valid(left) || ! dense_bitset_u32_is_valid(right) || dense_bitset_u32_count_ones(left) != {left_count} || dense_bitset_u32_count_ones(right) != {right_count} || (dense_bitset_u32_first_set(left) ?? -1) != {first} || (dense_bitset_u32_next_set(left, {from}) ?? -1) != {next} || (dense_bitset_u32_kth_set(left, {rank}) ?? -1) != {kth}) ok := false; fi;"
            )
            .unwrap();
        }
    }

    body.push_str("if (ok) 42 else 0 fi");
    all_backends(&body);
}
