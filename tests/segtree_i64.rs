//! pure Vaak固定i64 segment treeの参照実装・VM・STEEL差分試験。

use std::fmt::Write as _;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use vaak::interp::Eval;

const SEGTREE: &str = include_str!("../stdlib/ds/segtree_i64.vaak");
const LAZY: &str = include_str!("../stdlib/ds/lazy_segtree_i64.vaak");
const MISSING: i64 = -9_000_000_000_000_000_000;
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
    let program = vaak::parser::parse(&source).expect("segment treeを含むソースを解析できる");
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
    let directory = std::env::temp_dir().join(format!("vaak-segtree-{}-{id}", std::process::id()));
    std::fs::create_dir(&directory).expect("専用一時ディレクトリを作れる");
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
    std::fs::remove_dir_all(&directory).expect("専用一時ディレクトリを片付けられる");
    assert_eq!(status.code(), Some(expected_exit));
}

#[test]
fn 零長木はidentityを返し負長と容量overflowはparadoxになる() {
    reference_and_vm(
        &[SEGTREE],
        r#"
            let empty : i64 array := new i64 array(0, 0);
            let sum := sum_segtree_i64_from(empty) ?? new SumSegtreeI64(length := 1, size := 1, data := [9, 9]);
            let min := min_segtree_i64_from(empty) ?? new MinSegtreeI64(length := 1, size := 1, data := [9, 9]);
            let max := max_segtree_i64_from(empty) ?? new MaxSegtreeI64(length := 1, size := 1, data := [9, 9]);
            if (sum_segtree_i64_len(sum) == 0 && min_segtree_i64_len(min) == 0 &&
                max_segtree_i64_len(max) == 0 && sum_segtree_i64_all_prod(sum) == 0 &&
                min_segtree_i64_all_prod(min) == 9223372036854775807 &&
                max_segtree_i64_all_prod(max) == (0 - 9223372036854775807 - 1) &&
                sum_segtree_i64_prod(sum, 0, 0) == 0 &&
                min_segtree_i64_prod(min, 0, 0) == 9223372036854775807 &&
                max_segtree_i64_prod(max, 0, 0) == (0 - 9223372036854775807 - 1)) 42 else 0 fi
        "#,
        "値 42",
    );
    reference_and_vm(&[SEGTREE], "sum_segtree_i64_new(-1)", "paradox");
    reference_and_vm(
        &[SEGTREE],
        "min_segtree_i64_new(4611686018427387904)",
        "paradox",
    );
    reference_and_vm(
        &[LAZY],
        r#"var tree := range_add_sum_segtree_i64_new(0) ??
               new RangeAddSumSegtreeI64(length := 1, size := 1, data := [9, 9], lazy := [9, 9]);
           let updated := range_add_sum_segtree_i64_range_add(tree, 0, 0, 7) ?? false;
           if (updated && range_add_sum_segtree_i64_len(tree) == 0 &&
               range_add_sum_segtree_i64_all_prod(tree) == 0 &&
               range_add_sum_segtree_i64_prod(tree, 0, 0) == 0) 42 else 0 fi"#,
        "値 42",
    );
    reference_and_vm(&[LAZY], "range_add_sum_segtree_i64_new(-1)", "paradox");
}

#[test]
fn point_set_getと半開区間prodは三演算のidentityを保つ() {
    let body = r#"
        let values := [3, -2, 5, 7, -1];
        var sum := sum_segtree_i64_from(values) ?? new SumSegtreeI64(length := 0, size := 1, data := [0, 0]);
        var min := min_segtree_i64_from(values) ?? new MinSegtreeI64(length := 0, size := 1, data := [0, 0]);
        var max := max_segtree_i64_from(values) ?? new MaxSegtreeI64(length := 0, size := 1, data := [0, 0]);
        let before := sum_segtree_i64_prod(sum, 1, 4) ?? -99;
        sum_segtree_i64_set(sum, 1, 9) ?? false;
        min_segtree_i64_set(min, 1, 9) ?? false;
        max_segtree_i64_set(max, 1, 9) ?? false;
        if (before == 10 && sum_segtree_i64_all_prod(sum) == 23 &&
            sum_segtree_i64_get(sum, 1) == 9 && min_segtree_i64_all_prod(min) == -1 &&
            max_segtree_i64_all_prod(max) == 9 &&
            min_segtree_i64_prod(min, 2, 2) == 9223372036854775807 &&
            max_segtree_i64_prod(max, 2, 2) == (0 - 9223372036854775807 - 1)) 42 else 0 fi
    "#;
    reference_and_vm(&[SEGTREE], body, "値 42");
    steel_native(&[SEGTREE], body, 42);
}

#[test]
fn 範囲外pointと逆転区間はparadoxになりsumはi64で折り返す() {
    for body in [
        "var t := sum_segtree_i64_new(2) ?? new SumSegtreeI64(length := 0, size := 1, data := [0, 0]); sum_segtree_i64_set(t, 2, 1)",
        "let t := min_segtree_i64_new(2) ?? new MinSegtreeI64(length := 0, size := 1, data := [0, 0]); min_segtree_i64_get(t, -1)",
        "let t := max_segtree_i64_new(2) ?? new MaxSegtreeI64(length := 0, size := 1, data := [0, 0]); max_segtree_i64_prod(t, 2, 1)",
        "let t := sum_segtree_i64_new(2) ?? new SumSegtreeI64(length := 0, size := 1, data := [0, 0]); sum_segtree_i64_prod(t, 0, 3)",
    ] {
        reference_and_vm(&[SEGTREE], body, "paradox");
    }
    reference_and_vm(
        &[SEGTREE],
        r#"let values := [9223372036854775807, 1];
           let t := sum_segtree_i64_from(values) ?? new SumSegtreeI64(length := 0, size := 1, data := [0, 0]);
           sum_segtree_i64_all_prod(t)"#,
        "値 -9223372036854775808",
    );
}

#[test]
fn range_add_sumは重なる更新と空区間を処理する() {
    let body = r#"
        let values := [1, 2, 3, 4, 5];
        var tree := range_add_sum_segtree_i64_from(values) ??
            new RangeAddSumSegtreeI64(length := 0, size := 1, data := [0, 0], lazy := [0, 0]);
        range_add_sum_segtree_i64_range_add(tree, 1, 4, 10) ?? false;
        range_add_sum_segtree_i64_range_add(tree, 2, 2, 99) ?? false;
        let middle := range_add_sum_segtree_i64_prod(tree, 1, 4) ?? -1;
        range_add_sum_segtree_i64_range_add(tree, 0, 5, -2) ?? false;
        if (middle == 39 && range_add_sum_segtree_i64_all_prod(tree) == 35 &&
            range_add_sum_segtree_i64_prod(tree, 0, 0) == 0 &&
            range_add_sum_segtree_i64_get(tree, 3) == 12 &&
            range_add_sum_segtree_i64_len(tree) == 5) 42 else 0 fi
    "#;
    reference_and_vm(&[LAZY], body, "値 42");
    steel_native(&[LAZY], body, 42);
}

#[test]
fn range_add_sumの範囲外はparadoxで加算は折り返す() {
    for body in [
        "var t := range_add_sum_segtree_i64_new(2) ?? new RangeAddSumSegtreeI64(length := 0, size := 1, data := [0, 0], lazy := [0, 0]); range_add_sum_segtree_i64_range_add(t, -1, 1, 3)",
        "let t := range_add_sum_segtree_i64_new(2) ?? new RangeAddSumSegtreeI64(length := 0, size := 1, data := [0, 0], lazy := [0, 0]); range_add_sum_segtree_i64_prod(t, 1, 3)",
        "let t := range_add_sum_segtree_i64_new(2) ?? new RangeAddSumSegtreeI64(length := 0, size := 1, data := [0, 0], lazy := [0, 0]); range_add_sum_segtree_i64_get(t, 2)",
    ] {
        reference_and_vm(&[LAZY], body, "paradox");
    }
    reference_and_vm(
        &[LAZY],
        r#"let values := [9223372036854775807, 0];
           var t := range_add_sum_segtree_i64_from(values) ??
               new RangeAddSumSegtreeI64(length := 0, size := 1, data := [0, 0], lazy := [0, 0]);
           range_add_sum_segtree_i64_range_add(t, 0, 2, 1) ?? false;
           range_add_sum_segtree_i64_all_prod(t)"#,
        "値 -9223372036854775807",
    );
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

    fn value(&mut self) -> i64 {
        (self.next() % 2001) as i64 - 1000
    }
}

fn array_literal(values: &[i64]) -> String {
    let mut result = String::from("[");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            result.push_str(", ");
        }
        write!(result, "{value}").unwrap();
    }
    result.push(']');
    result
}

fn check_statement(source: &mut String, expression: &str, expected: i64) {
    writeln!(
        source,
        "if ((({expression}) ?? {MISSING}) != {expected}) ok := false; fi;"
    )
    .unwrap();
}

#[test]
fn 固定三演算の決定的ランダム列をrust_oracleと三backendで照合する() {
    const N: usize = 19;
    let mut random = Deterministic(0x5641_414b_5345_4754);
    let mut values: Vec<i64> = (0..N).map(|_| random.value()).collect();
    let literal = array_literal(&values);
    let mut body = format!(
        "let initial := {literal};\n\
         var sum := sum_segtree_i64_from(initial) ?? new SumSegtreeI64(length := 0, size := 1, data := [0, 0]);\n\
         var min := min_segtree_i64_from(initial) ?? new MinSegtreeI64(length := 0, size := 1, data := [0, 0]);\n\
         var max := max_segtree_i64_from(initial) ?? new MaxSegtreeI64(length := 0, size := 1, data := [0, 0]);\n\
         var ok := true;\n"
    );

    for step in 0..144 {
        if random.next() % 5 < 2 {
            let index = (random.next() % N as u64) as usize;
            let value = random.value();
            values[index] = value;
            writeln!(
                body,
                "if (! (sum_segtree_i64_set(sum, {index}, {value}) ?? false) ||\n\
                   ! (min_segtree_i64_set(min, {index}, {value}) ?? false) ||\n\
                   ! (max_segtree_i64_set(max, {index}, {value}) ?? false)) ok := false; fi;"
            )
            .unwrap();
        } else {
            let a = (random.next() % (N as u64 + 1)) as usize;
            let b = (random.next() % (N as u64 + 1)) as usize;
            let (first, last) = if a <= b { (a, b) } else { (b, a) };
            let slice = &values[first..last];
            let sum = slice
                .iter()
                .fold(0i64, |acc, value| acc.wrapping_add(*value));
            let min = slice.iter().copied().min().unwrap_or(i64::MAX);
            let max = slice.iter().copied().max().unwrap_or(i64::MIN);
            check_statement(
                &mut body,
                &format!("sum_segtree_i64_prod(sum, {first}, {last})"),
                sum,
            );
            check_statement(
                &mut body,
                &format!("min_segtree_i64_prod(min, {first}, {last})"),
                min,
            );
            check_statement(
                &mut body,
                &format!("max_segtree_i64_prod(max, {first}, {last})"),
                max,
            );
        }
        if step % 17 == 0 {
            let index = (random.next() % N as u64) as usize;
            check_statement(
                &mut body,
                &format!("sum_segtree_i64_get(sum, {index})"),
                values[index],
            );
            check_statement(
                &mut body,
                &format!("min_segtree_i64_get(min, {index})"),
                values[index],
            );
            check_statement(
                &mut body,
                &format!("max_segtree_i64_get(max, {index})"),
                values[index],
            );
        }
    }
    let sum = values
        .iter()
        .fold(0i64, |acc, value| acc.wrapping_add(*value));
    check_statement(&mut body, "sum_segtree_i64_all_prod(sum)", sum);
    check_statement(
        &mut body,
        "min_segtree_i64_all_prod(min)",
        *values.iter().min().unwrap(),
    );
    check_statement(
        &mut body,
        "max_segtree_i64_all_prod(max)",
        *values.iter().max().unwrap(),
    );
    body.push_str("if (ok) 42 else 0 fi");

    reference_and_vm(&[SEGTREE], &body, "値 42");
    steel_native(&[SEGTREE], &body, 42);
}

#[test]
fn lazy加算の決定的ランダム列をrust_oracleと三backendで照合する() {
    const N: usize = 17;
    let mut random = Deterministic(0x5641_414b_4c41_5a59);
    let mut values: Vec<i64> = (0..N).map(|_| random.value()).collect();
    let literal = array_literal(&values);
    let mut body = format!(
        "let initial := {literal};\n\
         var tree := range_add_sum_segtree_i64_from(initial) ??\n\
             new RangeAddSumSegtreeI64(length := 0, size := 1, data := [0, 0], lazy := [0, 0]);\n\
         var ok := true;\n"
    );

    for _ in 0..112 {
        let a = (random.next() % (N as u64 + 1)) as usize;
        let b = (random.next() % (N as u64 + 1)) as usize;
        let (first, last) = if a <= b { (a, b) } else { (b, a) };
        if random.next() % 3 != 0 {
            let delta = random.value();
            for value in &mut values[first..last] {
                *value = value.wrapping_add(delta);
            }
            writeln!(
                body,
                "if (! (range_add_sum_segtree_i64_range_add(tree, {first}, {last}, {delta}) ?? false)) ok := false; fi;"
            )
            .unwrap();
        } else {
            let expected = values[first..last]
                .iter()
                .fold(0i64, |acc, value| acc.wrapping_add(*value));
            check_statement(
                &mut body,
                &format!("range_add_sum_segtree_i64_prod(tree, {first}, {last})"),
                expected,
            );
        }
    }
    let total = values
        .iter()
        .fold(0i64, |acc, value| acc.wrapping_add(*value));
    check_statement(&mut body, "range_add_sum_segtree_i64_all_prod(tree)", total);
    for index in 0..N {
        check_statement(
            &mut body,
            &format!("range_add_sum_segtree_i64_get(tree, {index})"),
            values[index],
        );
    }
    body.push_str("if (ok) 42 else 0 fi");

    reference_and_vm(&[LAZY], &body, "値 42");
    steel_native(&[LAZY], &body, 42);
}
