//! pure Vaak Fenwick派生型の参照実装・VM・STEEL差分試験。

use std::fmt::Write as _;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use vaak::interp::Eval;

const FENWICK: &str = include_str!("../stdlib/ds/fenwick_i64.vaak");
const COUNT: &str = include_str!("../stdlib/ds/fenwick_count_i64.vaak");
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
    let program = vaak::parser::parse(&source).expect("Fenwick派生型を含むソースを解析できる");
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
        std::env::temp_dir().join(format!("vaak-fenwick-variants-{}-{id}", std::process::id()));
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
fn 通常fenwickはpoint加算と半開区間和をi64折返しで保つ() {
    let body = r#"
        var tree := fenwick_i64_new(4) ?? new FenwickI64(data := [0]);
        fenwick_i64_add(tree, 1, 9223372036854775807) ?? false;
        fenwick_i64_add(tree, 1, 1) ?? false;
        fenwick_i64_add(tree, 3, 7) ?? false;
        if (fenwick_i64_get(tree, 1) == (0 - 9223372036854775807 - 1) &&
            fenwick_i64_prefix_sum(tree, 2) == (0 - 9223372036854775807 - 1) &&
            fenwick_i64_range_sum(tree, 2, 4) == 7 &&
            fenwick_i64_range_sum(tree, 2, 2) == 0) 42 else 0 fi
    "#;
    reference_and_vm(&[FENWICK], body, "値 42");
    steel_native(&[FENWICK], body, 42);
}

#[test]
fn count_fenwickは非負countから累積順位を選ぶ() {
    let body = r#"
        let values := [0, 3, 0, 2, 5];
        var tree := fenwick_count_i64_from_counts(values) ??
            new FenwickCountI64(data := [0], total := 0);
        let initial := fenwick_count_i64_total(tree) == 10 &&
            fenwick_count_i64_prefix_sum(tree, 3) == 3 &&
            fenwick_count_i64_range_sum(tree, 1, 4) == 5 &&
            fenwick_count_i64_lower_bound(tree, 1) == 1 &&
            fenwick_count_i64_lower_bound(tree, 3) == 1 &&
            fenwick_count_i64_lower_bound(tree, 4) == 3 &&
            fenwick_count_i64_lower_bound(tree, 5) == 3 &&
            fenwick_count_i64_lower_bound(tree, 6) == 4 &&
            fenwick_count_i64_lower_bound(tree, 10) == 4;
        fenwick_count_i64_add(tree, 2, 4) ?? false;
        fenwick_count_i64_add(tree, 1, -2) ?? false;
        if (initial && fenwick_count_i64_total(tree) == 12 &&
            fenwick_count_i64_get(tree, 1) == 1 &&
            fenwick_count_i64_get(tree, 2) == 4 &&
            fenwick_count_i64_lower_bound(tree, 1) == 1 &&
            fenwick_count_i64_lower_bound(tree, 2) == 2 &&
            fenwick_count_i64_lower_bound(tree, 12) == 4) 42 else 0 fi
    "#;
    reference_and_vm(&[COUNT], body, "値 42");
    steel_native(&[COUNT], body, 42);
}

#[test]
fn count_fenwickは失敗した更新を原子的に捨てる() {
    reference_and_vm(
        &[COUNT],
        r#"let values := [9223372036854775807, 0];
           var tree := fenwick_count_i64_from_counts(values) ??
               new FenwickCountI64(data := [0], total := 0);
           let overflow := fenwick_count_i64_add(tree, 1, 1) ?? false;
           let underflow := fenwick_count_i64_add(tree, 0, 0 - 9223372036854775807 - 1) ?? false;
           if (! overflow && ! underflow &&
               fenwick_count_i64_total(tree) == 9223372036854775807 &&
               fenwick_count_i64_get(tree, 0) == 9223372036854775807 &&
               fenwick_count_i64_get(tree, 1) == 0) 42 else 0 fi"#,
        "値 42",
    );
    reference_and_vm(
        &[COUNT],
        r#"let values := [2];
           var tree := fenwick_count_i64_from_counts(values) ??
               new FenwickCountI64(data := [0], total := 0);
           let failed := fenwick_count_i64_add(tree, 0, -3) ?? false;
           if (! failed && fenwick_count_i64_total(tree) == 2 &&
               fenwick_count_i64_get(tree, 0) == 2) 42 else 0 fi"#,
        "値 42",
    );
}

#[test]
fn count_fenwickの零長と範囲外と不正初期値はparadoxになる() {
    reference_and_vm(
        &[COUNT],
        r#"let tree := fenwick_count_i64_new(0) ??
               new FenwickCountI64(data := [0], total := 1);
           if (fenwick_count_i64_len(tree) == 0 &&
               fenwick_count_i64_total(tree) == 0 &&
               fenwick_count_i64_prefix_sum(tree, 0) == 0 &&
               fenwick_count_i64_range_sum(tree, 0, 0) == 0) 42 else 0 fi"#,
        "値 42",
    );
    for body in [
        "fenwick_count_i64_new(-1)",
        "let xs := [1, -1]; fenwick_count_i64_from_counts(xs)",
        "let xs := [9223372036854775807, 1]; fenwick_count_i64_from_counts(xs)",
        "let tree := fenwick_count_i64_new(0) ?? new FenwickCountI64(data := [0], total := 1); fenwick_count_i64_lower_bound(tree, 1)",
        "let xs := [0, 2]; let tree := fenwick_count_i64_from_counts(xs) ?? new FenwickCountI64(data := [0], total := 0); fenwick_count_i64_lower_bound(tree, 0)",
        "let xs := [0, 2]; let tree := fenwick_count_i64_from_counts(xs) ?? new FenwickCountI64(data := [0], total := 0); fenwick_count_i64_lower_bound(tree, 3)",
        "var tree := fenwick_count_i64_new(2) ?? new FenwickCountI64(data := [0], total := 0); fenwick_count_i64_add(tree, 2, 1)",
        "let tree := fenwick_count_i64_new(2) ?? new FenwickCountI64(data := [0], total := 0); fenwick_count_i64_range_sum(tree, 2, 1)",
    ] {
        reference_and_vm(&[COUNT], body, "paradox");
    }
}

struct Deterministic(u64);

impl Deterministic {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 7;
        self.0 ^= self.0 >> 9;
        self.0 ^= self.0 << 8;
        self.0
    }
}

fn prefix(model: &[i64], last: usize) -> i64 {
    model[..last].iter().copied().sum()
}

fn lower_bound(model: &[i64], target: i64) -> usize {
    let mut total = 0;
    model
        .iter()
        .position(|&value| {
            total += value;
            total >= target
        })
        .expect("targetは総数以下")
}

#[test]
fn count_fenwickの決定的random列を独立vec_oracleと三backendで照合する() {
    const N: usize = 17;
    let mut random = Deterministic(0x5641_414b_434f_554e);
    let mut model = vec![0i64; N];
    let mut body = String::from(
        "var tree := fenwick_count_i64_new(17) ??\n\
         new FenwickCountI64(data := [0], total := 0);\n\
         var ok := true;\n",
    );

    for step in 0..220 {
        let index = (random.next() % N as u64) as usize;
        match random.next() % 7 {
            0..=2 => {
                let delta = if model[index] > 0 && random.next() % 3 == 0 {
                    -((random.next() % model[index] as u64) as i64 + 1)
                } else {
                    (random.next() % 11) as i64
                };
                model[index] += delta;
                writeln!(
                    body,
                    "if (! (fenwick_count_i64_add(tree, {index}, {delta}) ?? false)) ok := false; fi;"
                )
                .unwrap();
            }
            3 => {
                let last = (random.next() % (N as u64 + 1)) as usize;
                let expected = prefix(&model, last);
                writeln!(
                    body,
                    "if ((fenwick_count_i64_prefix_sum(tree, {last}) ?? -1) != {expected}) ok := false; fi;"
                )
                .unwrap();
            }
            4 => {
                let a = (random.next() % (N as u64 + 1)) as usize;
                let b = (random.next() % (N as u64 + 1)) as usize;
                let (first, last) = if a <= b { (a, b) } else { (b, a) };
                let expected = prefix(&model, last) - prefix(&model, first);
                writeln!(
                    body,
                    "if ((fenwick_count_i64_range_sum(tree, {first}, {last}) ?? -1) != {expected}) ok := false; fi;"
                )
                .unwrap();
            }
            5 => {
                writeln!(
                    body,
                    "if ((fenwick_count_i64_get(tree, {index}) ?? -1) != {}) ok := false; fi;",
                    model[index]
                )
                .unwrap();
            }
            _ => {
                let total = prefix(&model, N);
                if total > 0 {
                    let target = (random.next() % total as u64) as i64 + 1;
                    let expected = lower_bound(&model, target);
                    writeln!(
                        body,
                        "if ((fenwick_count_i64_lower_bound(tree, {target}) ?? -1) != {expected}) ok := false; fi;"
                    )
                    .unwrap();
                }
            }
        }
        if step % 19 == 0 {
            let total = prefix(&model, N);
            writeln!(
                body,
                "if ((fenwick_count_i64_total(tree) ?? -1) != {total}) ok := false; fi;"
            )
            .unwrap();
        }
    }
    body.push_str("if (ok) 42 else 0 fi");

    reference_and_vm(&[COUNT], &body, "値 42");
    steel_native(&[COUNT], &body, 42);
}
