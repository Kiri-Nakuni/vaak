//! pure Vaak range-update Fenwickを独立vector oracleと三backendで照合する。

use std::fmt::Write as _;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use vaak::interp::Eval;

const RANGE_FENWICK: &str = include_str!("../stdlib/ds/fenwick_range_i64.vaak");
static TEMP_ID: AtomicU64 = AtomicU64::new(0);

fn source(body: &str) -> String {
    format!("{RANGE_FENWICK}\n{body}")
}

#[track_caller]
fn checked(body: &str) -> String {
    let source = source(body);
    let program = vaak::parser::parse(&source).expect("range Fenwickを含むsourceを解析できる");
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
        std::env::temp_dir().join(format!("vaak-fenwick-range-{}-{id}", std::process::id()));
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
    assert_eq!(status.code(), Some(expected_exit));
}

#[test]
fn sourceは単独で三backendへ前置きできる() {
    reference_and_vm("42", "値 42");
    let source = checked("42");
    let program = vaak::parser::parse(&source).expect("構文");
    vaak::steel::compile(&program).unwrap_or_else(|error| panic!("STEEL: {}", error.msg));
}

#[test]
fn range_add_pointは入力を複製し折返しながら一点を返す() {
    let body = r#"
        var values := [9223372036854775807, -2, 3, (0 - 9223372036854775807 - 1)];
        var tree := range_add_point_fenwick_i64_from(values) ??
            new RangeAddPointFenwickI64(data := []);
        values[1] := 99;
        range_add_point_fenwick_i64_range_add(tree, 1, 4, 5) ?? false;
        range_add_point_fenwick_i64_range_add(tree, 0, 1, 1) ?? false;
        let empty := range_add_point_fenwick_i64_range_add(
            tree, 2, 2, (0 - 9223372036854775807 - 1)
        ) ?? false;
        if (empty && range_add_point_fenwick_i64_len(tree) == 4 &&
            range_add_point_fenwick_i64_get(tree, 0) ==
                (0 - 9223372036854775807 - 1) &&
            range_add_point_fenwick_i64_get(tree, 1) == 3 &&
            range_add_point_fenwick_i64_get(tree, 2) == 8 &&
            range_add_point_fenwick_i64_get(tree, 3) ==
                (0 - 9223372036854775807 - 1 + 5)) 42 else 0 fi
    "#;
    reference_and_vm(body, "値 42");
    steel_native(body, 42);
}

#[test]
fn range_add_sumは二係数で半開区間和を返す() {
    let body = r#"
        var values := [3, -1, 4, 1, 5];
        var tree := range_add_sum_fenwick_i64_from(values) ??
            new RangeAddSumFenwickI64(delta := [], weighted := []);
        values[0] := 99;
        range_add_sum_fenwick_i64_range_add(tree, 1, 4, 7) ?? false;
        let empty := range_add_sum_fenwick_i64_range_add(tree, 3, 3, 19) ?? false;
        if (empty && range_add_sum_fenwick_i64_is_valid(tree) &&
            range_add_sum_fenwick_i64_len(tree) == 5 &&
            range_add_sum_fenwick_i64_prefix_sum(tree, 0) == 0 &&
            range_add_sum_fenwick_i64_prefix_sum(tree, 3) == 20 &&
            range_add_sum_fenwick_i64_range_sum(tree, 1, 4) == 25 &&
            range_add_sum_fenwick_i64_range_sum(tree, 2, 2) == 0 &&
            range_add_sum_fenwick_i64_get(tree, 4) == 5 &&
            range_add_sum_fenwick_i64_prefix_sum(tree, 5) == 33) 42 else 0 fi
    "#;
    reference_and_vm(body, "値 42");
    steel_native(body, 42);

    let wrapping = r#"
        var tree := range_add_sum_fenwick_i64_new(3) ??
            new RangeAddSumFenwickI64(delta := [], weighted := []);
        range_add_sum_fenwick_i64_range_add(tree, 0, 3, 9223372036854775807) ?? false;
        range_add_sum_fenwick_i64_range_add(tree, 0, 3, 1) ?? false;
        if (range_add_sum_fenwick_i64_is_valid(tree) &&
            range_add_sum_fenwick_i64_get(tree, 1) ==
                (0 - 9223372036854775807 - 1) &&
            range_add_sum_fenwick_i64_prefix_sum(tree, 3) ==
                (0 - 9223372036854775807 - 1)) 42 else 0 fi
    "#;
    reference_and_vm(wrapping, "値 42");
    steel_native(wrapping, 42);
}

#[test]
fn 零長と不正範囲と公開欄破損を分ける() {
    let empty = r#"
        var point := range_add_point_fenwick_i64_new(0) ??
            new RangeAddPointFenwickI64(data := [1]);
        var sum := range_add_sum_fenwick_i64_new(0) ??
            new RangeAddSumFenwickI64(delta := [1], weighted := [1]);
        if (range_add_point_fenwick_i64_len(point) == 0 &&
            (range_add_point_fenwick_i64_range_add(point, 0, 0, 7) ?? false) &&
            range_add_sum_fenwick_i64_is_valid(sum) &&
            range_add_sum_fenwick_i64_len(sum) == 0 &&
            (range_add_sum_fenwick_i64_range_add(sum, 0, 0, 7) ?? false) &&
            range_add_sum_fenwick_i64_prefix_sum(sum, 0) == 0 &&
            range_add_sum_fenwick_i64_range_sum(sum, 0, 0) == 0) 42 else 0 fi
    "#;
    reference_and_vm(empty, "値 42");
    steel_native(empty, 42);

    for body in [
        "range_add_point_fenwick_i64_new(-1)",
        "range_add_sum_fenwick_i64_new(-1)",
        "var tree := range_add_point_fenwick_i64_new(2) ?? new RangeAddPointFenwickI64(data := []); range_add_point_fenwick_i64_range_add(tree, -1, 1, 3)",
        "var tree := range_add_point_fenwick_i64_new(2) ?? new RangeAddPointFenwickI64(data := []); range_add_point_fenwick_i64_range_add(tree, 2, 1, 3)",
        "let tree := range_add_point_fenwick_i64_new(2) ?? new RangeAddPointFenwickI64(data := []); range_add_point_fenwick_i64_get(tree, 2)",
        "var tree := range_add_sum_fenwick_i64_new(2) ?? new RangeAddSumFenwickI64(delta := [], weighted := []); range_add_sum_fenwick_i64_range_add(tree, 0, 3, 1)",
        "let tree := range_add_sum_fenwick_i64_new(2) ?? new RangeAddSumFenwickI64(delta := [], weighted := []); range_add_sum_fenwick_i64_range_sum(tree, 2, 1)",
        "let tree := range_add_sum_fenwick_i64_new(2) ?? new RangeAddSumFenwickI64(delta := [], weighted := []); range_add_sum_fenwick_i64_get(tree, -1)",
        "var tree := new RangeAddSumFenwickI64(delta := [0], weighted := []); range_add_sum_fenwick_i64_range_add(tree, 0, 0, 1)",
        "let tree := new RangeAddSumFenwickI64(delta := [0], weighted := []); range_add_sum_fenwick_i64_prefix_sum(tree, 0)",
    ] {
        reference_and_vm(body, "paradox");
    }

    reference_and_vm(
        r#"
            let wrong_shape := new RangeAddSumFenwickI64(delta := [0], weighted := []);
            let wrong_coefficient := new RangeAddSumFenwickI64(
                delta := [1, 2], weighted := [0, 0]
            );
            if (! range_add_sum_fenwick_i64_is_valid(wrong_shape) &&
                ! range_add_sum_fenwick_i64_is_valid(wrong_coefficient)) 42 else 0 fi
        "#,
        "値 42",
    );

    let atomic = r#"
        let values := [1, 2, 3];
        var point := range_add_point_fenwick_i64_from(values) ??
            new RangeAddPointFenwickI64(data := []);
        var sum := range_add_sum_fenwick_i64_from(values) ??
            new RangeAddSumFenwickI64(delta := [], weighted := []);
        let point_failed := range_add_point_fenwick_i64_range_add(
            point, -1, 2, 9
        ) ?? false;
        let sum_failed := range_add_sum_fenwick_i64_range_add(
            sum, 0, 4, 9
        ) ?? false;
        if (! point_failed && ! sum_failed &&
            range_add_point_fenwick_i64_get(point, 0) == 1 &&
            range_add_point_fenwick_i64_get(point, 2) == 3 &&
            range_add_sum_fenwick_i64_is_valid(sum) &&
            range_add_sum_fenwick_i64_prefix_sum(sum, 3) == 6) 42 else 0 fi
    "#;
    reference_and_vm(atomic, "値 42");
    steel_native(atomic, 42);
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

fn range_sum(values: &[i64], first: usize, last: usize) -> i64 {
    values[first..last]
        .iter()
        .fold(0_i64, |sum, value| sum.wrapping_add(*value))
}

#[test]
fn 決定的操作列をrust_vector_oracleと三backendで照合する() {
    const N: usize = 19;
    let mut random = Deterministic(0x5241_4e47_4546_454e);
    let mut model: Vec<i64> = (0..N).map(|index| index as i64 * 7 - 31).collect();
    let mut body = format!(
        "let values := [{}];\nvar point := range_add_point_fenwick_i64_from(values) ?? new RangeAddPointFenwickI64(data := []);\nvar sum := range_add_sum_fenwick_i64_from(values) ?? new RangeAddSumFenwickI64(delta := [], weighted := []);\nvar ok := true;\n",
        model
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    );

    for step in 0..260 {
        match random.next() % 5 {
            0..=1 => {
                let a = (random.next() % (N as u64 + 1)) as usize;
                let b = (random.next() % (N as u64 + 1)) as usize;
                let (first, last) = if a <= b { (a, b) } else { (b, a) };
                let value = (random.next() % 101) as i64 - 50;
                for item in &mut model[first..last] {
                    *item = item.wrapping_add(value);
                }
                writeln!(
                    body,
                    "if (! (range_add_point_fenwick_i64_range_add(point, {first}, {last}, {value}) ?? false)) ok := false; fi;"
                )
                .unwrap();
                writeln!(
                    body,
                    "if (! (range_add_sum_fenwick_i64_range_add(sum, {first}, {last}, {value}) ?? false)) ok := false; fi;"
                )
                .unwrap();
            }
            2 => {
                let index = (random.next() % N as u64) as usize;
                writeln!(
                    body,
                    "if ((range_add_point_fenwick_i64_get(point, {index}) ?? 999) != {}) ok := false; fi;",
                    model[index]
                )
                .unwrap();
                writeln!(
                    body,
                    "if ((range_add_sum_fenwick_i64_get(sum, {index}) ?? 999) != {}) ok := false; fi;",
                    model[index]
                )
                .unwrap();
            }
            3 => {
                let last = (random.next() % (N as u64 + 1)) as usize;
                let expected = range_sum(&model, 0, last);
                writeln!(
                    body,
                    "if ((range_add_sum_fenwick_i64_prefix_sum(sum, {last}) ?? 999) != {expected}) ok := false; fi;"
                )
                .unwrap();
            }
            _ => {
                let a = (random.next() % (N as u64 + 1)) as usize;
                let b = (random.next() % (N as u64 + 1)) as usize;
                let (first, last) = if a <= b { (a, b) } else { (b, a) };
                let expected = range_sum(&model, first, last);
                writeln!(
                    body,
                    "if ((range_add_sum_fenwick_i64_range_sum(sum, {first}, {last}) ?? 999) != {expected}) ok := false; fi;"
                )
                .unwrap();
            }
        }
        if step % 17 == 0 {
            writeln!(
                body,
                "if (! range_add_sum_fenwick_i64_is_valid(sum) || range_add_point_fenwick_i64_len(point) != {N} || (range_add_sum_fenwick_i64_len(sum) ?? -1) != {N}) ok := false; fi;"
            )
            .unwrap();
        }
    }

    for (index, expected) in model.iter().enumerate() {
        writeln!(
            body,
            "if ((range_add_point_fenwick_i64_get(point, {index}) ?? 999) != {expected} || (range_add_sum_fenwick_i64_get(sum, {index}) ?? 999) != {expected}) ok := false; fi;"
        )
        .unwrap();
    }
    let total = range_sum(&model, 0, N);
    writeln!(
        body,
        "if ((range_add_sum_fenwick_i64_prefix_sum(sum, {N}) ?? 999) != {total}) ok := false; fi;"
    )
    .unwrap();
    body.push_str("if (ok) 42 else 0 fi");

    reference_and_vm(&body, "値 42");
    steel_native(&body, 42);
}
