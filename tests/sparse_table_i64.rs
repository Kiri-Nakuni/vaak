//! fixed i64 sparse tableを独立vector oracleと三backendで照合する。

use std::fmt::Write as _;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use vaak::interp::Eval;

const SPARSE: &str = include_str!("../stdlib/ds/sparse_table_i64.vaak");
const DISJOINT: &str = include_str!("../stdlib/ds/disjoint_sparse_table_i64.vaak");
static TEMP_ID: AtomicU64 = AtomicU64::new(0);

fn source(parts: &[&str], body: &str) -> String {
    let mut source = parts.join("\n");
    source.push('\n');
    source.push_str(body);
    source
}

#[track_caller]
fn checked(parts: &[&str], body: &str) -> String {
    let source = source(parts, body);
    let program = vaak::parser::parse(&source).expect("sparse tableを含むsourceを解析できる");
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
fn reference_and_vm(parts: &[&str], body: &str, expected: &str) {
    let source = checked(parts, body);
    for (name, result) in [
        ("参照", vaak::interp::run(&source)),
        ("VM", vaak::vm::run(&source)),
    ] {
        assert_eq!(shape(result), expected, "{name}: {body}");
    }
}

#[track_caller]
fn steel_native(parts: &[&str], body: &str, expected_exit: i32) {
    let source = checked(parts, body);
    let program = vaak::parser::parse(&source).expect("構文");
    let ir = vaak::steel::compile(&program).unwrap_or_else(|error| panic!("STEEL: {}", error.msg));
    if Command::new("clang").arg("--version").output().is_err() {
        return;
    }

    let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let directory =
        std::env::temp_dir().join(format!("vaak-sparse-table-{}-{id}", std::process::id()));
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

fn vaak_i64(value: i64) -> String {
    if value == i64::MIN {
        "(0 - 9223372036854775807 - 1)".into()
    } else {
        value.to_string()
    }
}

fn array_literal(values: &[i64]) -> String {
    let values = values
        .iter()
        .map(|&value| vaak_i64(value))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{values}]")
}

#[derive(Clone, Copy)]
struct Deterministic(u64);

impl Deterministic {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }
}

fn random_values(length: usize) -> Vec<i64> {
    let mut random = Deterministic(0x5350_4152_5345_4936);
    let mut values = Vec::with_capacity(length);
    for index in 0..length {
        let value = match index {
            0 => i64::MIN,
            1 => i64::MAX,
            _ => (random.next() % 20_001) as i64 - 10_000,
        };
        values.push(value);
    }
    values
}

fn random_ranges(length: usize, count: usize) -> Vec<(usize, usize)> {
    let mut random = Deterministic(0x5241_4e47_4553_4936);
    let mut ranges = vec![(0, 0), (0, length), (length, length)];
    while ranges.len() < count {
        let mut first = (random.next() % (length as u64 + 1)) as usize;
        let mut last = (random.next() % (length as u64 + 1)) as usize;
        if first > last {
            std::mem::swap(&mut first, &mut last);
        }
        ranges.push((first, last));
    }
    ranges
}

#[test]
fn 二種sourceは単独かつ同時に前置きできる() {
    reference_and_vm(&[SPARSE], "42", "値 42");
    reference_and_vm(&[DISJOINT], "42", "値 42");
    reference_and_vm(&[SPARSE, DISJOINT], "42", "値 42");
    let source = checked(&[SPARSE, DISJOINT], "42");
    let program = vaak::parser::parse(&source).expect("構文");
    vaak::steel::compile(&program).unwrap_or_else(|error| panic!("STEEL: {}", error.msg));
}

#[test]
fn min_max_sparseは空区間identityと半開区間を返す() {
    let body = r#"
        let xs := [9223372036854775807, 5, -3, -3, 8,
                   (0 - 9223372036854775807 - 1), 4];
        let minimum := sparse_min_i64_from(xs) ??
            new SparseMinI64(length := 0, levels := 0, data := []);
        let maximum := sparse_max_i64_from(xs) ??
            new SparseMaxI64(length := 0, levels := 0, data := []);
        let empty_xs : i64 array := new i64 array(0, 0);
        let empty_min := sparse_min_i64_from(empty_xs) ??
            new SparseMinI64(length := -1, levels := -1, data := [0]);
        let empty_max := sparse_max_i64_from(empty_xs) ??
            new SparseMaxI64(length := -1, levels := -1, data := [0]);
        if (sparse_min_i64_is_valid(minimum) && sparse_max_i64_is_valid(maximum) &&
            sparse_min_i64_len(minimum) == 7 && sparse_max_i64_len(maximum) == 7 &&
            sparse_min_i64_get(minimum, 1) == 5 && sparse_max_i64_get(maximum, 4) == 8 &&
            sparse_min_i64_prod(minimum, 1, 5) == -3 &&
            sparse_max_i64_prod(maximum, 1, 5) == 8 &&
            sparse_min_i64_all_prod(minimum) == (0 - 9223372036854775807 - 1) &&
            sparse_max_i64_all_prod(maximum) == 9223372036854775807 &&
            sparse_min_i64_prod(minimum, 3, 3) == 9223372036854775807 &&
            sparse_max_i64_prod(maximum, 3, 3) == (0 - 9223372036854775807 - 1) &&
            sparse_min_i64_is_valid(empty_min) && sparse_max_i64_is_valid(empty_max) &&
            sparse_min_i64_all_prod(empty_min) == 9223372036854775807 &&
            sparse_max_i64_all_prod(empty_max) == (0 - 9223372036854775807 - 1)) 42 else 0 fi
    "#;
    reference_and_vm(&[SPARSE], body, "値 42");
    steel_native(&[SPARSE], body, 42);
}

#[test]
fn sparse_tableは範囲違反をparadoxにし壊れた公開欄を検出する() {
    let body = r#"
        let xs := [4, 1, 7, -2];
        var minimum := sparse_min_i64_from(xs) ??
            new SparseMinI64(length := 0, levels := 0, data := []);
        var maximum := sparse_max_i64_from(xs) ??
            new SparseMaxI64(length := 0, levels := 0, data := []);
        let bad_get := sparse_min_i64_get(minimum, -1) ?? 11;
        let bad_left := sparse_max_i64_prod(maximum, -1, 2) ?? 12;
        let bad_order := sparse_min_i64_prod(minimum, 3, 2) ?? 13;
        let bad_right := sparse_max_i64_prod(maximum, 0, 5) ?? 14;
        minimum.data[4] := 99;
        maximum.levels := 99;
        let oversized_min := new SparseMinI64(
            length := 9223372036854775807, levels := 63, data := []
        );
        let oversized_sum := new DisjointSparseSumI64(
            length := 9223372036854775807, levels := 63, data := []
        );
        if (bad_get == 11 && bad_left == 12 && bad_order == 13 && bad_right == 14 &&
            ! sparse_min_i64_is_valid(minimum) && ! sparse_max_i64_is_valid(maximum) &&
            ! sparse_min_i64_is_valid(oversized_min) &&
            ! disjoint_sparse_sum_i64_is_valid(oversized_sum))
            42 else 0 fi
    "#;
    reference_and_vm(&[SPARSE, DISJOINT], body, "値 42");
    steel_native(&[SPARSE, DISJOINT], body, 42);
}

#[test]
fn disjoint_sparse_sumは空単一範囲とi64折返し和を返す() {
    let body = r#"
        let xs := [9223372036854775807, 1, -5, 9, -2];
        var table := disjoint_sparse_sum_i64_from(xs) ??
            new DisjointSparseSumI64(length := 0, levels := 0, data := []);
        let empty_xs : i64 array := new i64 array(0, 0);
        let empty := disjoint_sparse_sum_i64_from(empty_xs) ??
            new DisjointSparseSumI64(length := -1, levels := -1, data := [0]);
        let bad := disjoint_sparse_sum_i64_prod(table, 4, 6) ?? 17;
        let before := disjoint_sparse_sum_i64_is_valid(table);
        table.data[5] := 12345;
        if (before && disjoint_sparse_sum_i64_len(table) == 5 &&
            disjoint_sparse_sum_i64_get(table, 3) == 9 &&
            disjoint_sparse_sum_i64_prod(table, 0, 2) ==
                (0 - 9223372036854775807 - 1) &&
            disjoint_sparse_sum_i64_prod(table, 2, 5) == 2 &&
            disjoint_sparse_sum_i64_prod(table, 2, 2) == 0 && bad == 17 &&
            disjoint_sparse_sum_i64_is_valid(empty) &&
            disjoint_sparse_sum_i64_all_prod(empty) == 0 &&
            ! disjoint_sparse_sum_i64_is_valid(table)) 42 else 0 fi
    "#;
    reference_and_vm(&[DISJOINT], body, "値 42");
    steel_native(&[DISJOINT], body, 42);
}

#[test]
fn min_max_sparseの決定的範囲をrust_oracleと三backendで照合する() {
    let values = random_values(73);
    let ranges = random_ranges(values.len(), 112);
    let mut body = format!(
        "let xs := {};\nlet minimum := sparse_min_i64_from(xs) ?? new SparseMinI64(length := 0, levels := 0, data := []);\nlet maximum := sparse_max_i64_from(xs) ?? new SparseMaxI64(length := 0, levels := 0, data := []);\nvar ok := sparse_min_i64_is_valid(minimum) && sparse_max_i64_is_valid(maximum);\n",
        array_literal(&values)
    );
    for (index, value) in values.iter().enumerate() {
        writeln!(
            body,
            "if ((sparse_min_i64_get(minimum, {index}) ?? 0) != {}) ok := false; fi;",
            vaak_i64(*value)
        )
        .unwrap();
    }
    for &(first, last) in &ranges {
        let (minimum, maximum) = if first == last {
            (i64::MAX, i64::MIN)
        } else {
            (
                *values[first..last].iter().min().expect("非空"),
                *values[first..last].iter().max().expect("非空"),
            )
        };
        writeln!(
            body,
            "if ((sparse_min_i64_prod(minimum, {first}, {last}) ?? 0) != {}) ok := false; fi;",
            vaak_i64(minimum)
        )
        .unwrap();
        writeln!(
            body,
            "if ((sparse_max_i64_prod(maximum, {first}, {last}) ?? 0) != {}) ok := false; fi;",
            vaak_i64(maximum)
        )
        .unwrap();
    }
    body.push_str("if (ok) 42 else 0 fi");

    reference_and_vm(&[SPARSE], &body, "値 42");
    steel_native(&[SPARSE], &body, 42);
}

#[test]
fn disjoint_sparse_sumの決定的範囲をrust_oracleと三backendで照合する() {
    let values = random_values(79);
    let ranges = random_ranges(values.len(), 128);
    let mut body = format!(
        "let xs := {};\nlet table := disjoint_sparse_sum_i64_from(xs) ?? new DisjointSparseSumI64(length := 0, levels := 0, data := []);\nvar ok := disjoint_sparse_sum_i64_is_valid(table);\n",
        array_literal(&values)
    );
    for &(first, last) in &ranges {
        let sum = values[first..last]
            .iter()
            .fold(0_i64, |sum, value| sum.wrapping_add(*value));
        writeln!(
            body,
            "if ((disjoint_sparse_sum_i64_prod(table, {first}, {last}) ?? 1) != {}) ok := false; fi;",
            vaak_i64(sum)
        )
        .unwrap();
    }
    body.push_str("if (ok) 42 else 0 fi");

    reference_and_vm(&[DISJOINT], &body, "値 42");
    steel_native(&[DISJOINT], &body, 42);
}
