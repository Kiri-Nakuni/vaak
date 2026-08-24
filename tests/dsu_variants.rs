//! pure Vaak DSU派生型の参照実装・VM・STEEL差分試験。

use std::fmt::Write as _;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use vaak::interp::Eval;

const ROLLBACK: &str = include_str!("../stdlib/ds/rollback_dsu_i64.vaak");
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
    let program = vaak::parser::parse(&source).expect("DSU派生型を含むソースを解析できる");
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
        std::env::temp_dir().join(format!("vaak-dsu-variants-{}-{id}", std::process::id()));
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
fn rollback_dsuの零長と容量境界を定義する() {
    reference_and_vm(
        &[ROLLBACK],
        r#"var dsu := rollback_dsu_i64_new(0) ??
               new RollbackDsuI64(parent_or_size := [0], history := [0]);
           let restored := rollback_dsu_i64_rollback_to(dsu, 0) ?? false;
           if (restored && rollback_dsu_i64_len(dsu) == 0 &&
               rollback_dsu_i64_snapshot(dsu) == 0 &&
               rollback_dsu_i64_group_count(dsu) == 0) 42 else 0 fi"#,
        "値 42",
    );
    reference_and_vm(&[ROLLBACK], "rollback_dsu_i64_new(-1)", "paradox");
    reference_and_vm(
        &[ROLLBACK],
        "rollback_dsu_i64_new(2305843009213693953)",
        "paradox",
    );
    reference_and_vm(
        &[ROLLBACK],
        "let dsu := rollback_dsu_i64_new(0) ?? new RollbackDsuI64(parent_or_size := [0], history := [0]); rollback_dsu_i64_leader(dsu, 0)",
        "paradox",
    );
    reference_and_vm(
        &[ROLLBACK],
        "var dsu := rollback_dsu_i64_new(0) ?? new RollbackDsuI64(parent_or_size := [0], history := [0]); rollback_dsu_i64_undo(dsu)",
        "paradox",
    );
}

#[test]
fn rollback_dsuは成功mergeだけをsnapshotへ積む() {
    let body = r#"
        var dsu := rollback_dsu_i64_new(5) ??
            new RollbackDsuI64(parent_or_size := [0], history := [0]);
        rollback_dsu_i64_merge(dsu, 0, 1) ?? -1;
        let first := rollback_dsu_i64_snapshot(dsu) ?? -1;
        rollback_dsu_i64_merge(dsu, 1, 2) ?? -1;
        rollback_dsu_i64_merge(dsu, 0, 2) ?? -1;
        let second := rollback_dsu_i64_snapshot(dsu) ?? -1;
        let before := rollback_dsu_i64_size(dsu, 2) ?? -1;
        rollback_dsu_i64_rollback_to(dsu, first) ?? false;
        let separated := ! rollback_dsu_i64_same(dsu, 0, 2);
        rollback_dsu_i64_undo(dsu) ?? false;
        if (first == 1 && second == 2 && before == 3 && separated &&
            rollback_dsu_i64_group_count(dsu) == 5 &&
            rollback_dsu_i64_snapshot(dsu) == 0) 42 else 0 fi
    "#;
    reference_and_vm(&[ROLLBACK], body, "値 42");
    steel_native(&[ROLLBACK], body, 42);
}

#[test]
fn rollback_dsuの範囲外と壊れたhistoryはparadoxになる() {
    for body in [
        "var dsu := rollback_dsu_i64_new(2) ?? new RollbackDsuI64(parent_or_size := [0], history := [0]); rollback_dsu_i64_merge(dsu, 0, 2)",
        "var dsu := rollback_dsu_i64_new(2) ?? new RollbackDsuI64(parent_or_size := [0], history := [0]); rollback_dsu_i64_rollback_to(dsu, -1)",
        "var dsu := rollback_dsu_i64_new(2) ?? new RollbackDsuI64(parent_or_size := [0], history := [0]); rollback_dsu_i64_rollback_to(dsu, 1)",
        "let dsu := new RollbackDsuI64(parent_or_size := [-1], history := [9]); rollback_dsu_i64_snapshot(dsu)",
    ] {
        reference_and_vm(&[ROLLBACK], body, "paradox");
    }
}

#[derive(Clone)]
struct Model {
    parent: Vec<usize>,
    size: Vec<usize>,
}

impl Model {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
            size: vec![1; n],
        }
    }

    fn leader(&self, mut node: usize) -> usize {
        while self.parent[node] != node {
            node = self.parent[node];
        }
        node
    }

    fn merge(&mut self, a: usize, b: usize) -> bool {
        let mut x = self.leader(a);
        let mut y = self.leader(b);
        if x == y {
            return false;
        }
        if self.size[x] < self.size[y] {
            std::mem::swap(&mut x, &mut y);
        }
        self.parent[y] = x;
        self.size[x] += self.size[y];
        true
    }

    fn same(&self, a: usize, b: usize) -> bool {
        self.leader(a) == self.leader(b)
    }

    fn component_size(&self, a: usize) -> usize {
        self.size[self.leader(a)]
    }

    fn group_count(&self) -> usize {
        (0..self.parent.len())
            .filter(|&node| self.parent[node] == node)
            .count()
    }
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

#[test]
fn rollback_dsuの決定的random列を独立snapshot_oracleと三backendで照合する() {
    const N: usize = 13;
    let mut random = Deterministic(0x5641_414b_524f_4c4c);
    let mut states = vec![Model::new(N)];
    let mut body = String::from(
        "var dsu := rollback_dsu_i64_new(13) ??\n\
         new RollbackDsuI64(parent_or_size := [0], history := [0]);\n\
         var ok := true;\n",
    );

    for step in 0..160 {
        let a = (random.next() % N as u64) as usize;
        let b = (random.next() % N as u64) as usize;
        match random.next() % 8 {
            0..=3 => {
                let mut next = states.last().unwrap().clone();
                let changed = next.merge(a, b);
                if changed {
                    states.push(next);
                }
                writeln!(body, "rollback_dsu_i64_merge(dsu, {a}, {b}) ?? -1;").unwrap();
            }
            4 => {
                let expected = states.last().unwrap().same(a, b);
                writeln!(
                    body,
                    "if ((rollback_dsu_i64_same(dsu, {a}, {b}) ?? false) != {expected}) ok := false; fi;"
                )
                .unwrap();
            }
            5 => {
                let expected = states.last().unwrap().component_size(a);
                writeln!(
                    body,
                    "if ((rollback_dsu_i64_size(dsu, {a}) ?? -1) != {expected}) ok := false; fi;"
                )
                .unwrap();
            }
            6 => {
                let target = (random.next() % states.len() as u64) as usize;
                states.truncate(target + 1);
                writeln!(
                    body,
                    "if (! (rollback_dsu_i64_rollback_to(dsu, {target}) ?? false)) ok := false; fi;"
                )
                .unwrap();
            }
            _ => {
                if states.len() > 1 {
                    states.pop();
                    body.push_str(
                        "if (! (rollback_dsu_i64_undo(dsu) ?? false)) ok := false; fi;\n",
                    );
                } else {
                    body.push_str(
                        "if ((rollback_dsu_i64_snapshot(dsu) ?? -1) != 0) ok := false; fi;\n",
                    );
                }
            }
        }
        if step % 11 == 0 {
            let expected_snapshot = states.len() - 1;
            let expected_groups = states.last().unwrap().group_count();
            writeln!(
                body,
                "if ((rollback_dsu_i64_snapshot(dsu) ?? -1) != {expected_snapshot} ||\n\
                   (rollback_dsu_i64_group_count(dsu) ?? -1) != {expected_groups}) ok := false; fi;"
            )
            .unwrap();
        }
    }
    body.push_str("if (ok) 42 else 0 fi");

    reference_and_vm(&[ROLLBACK], &body, "値 42");
    steel_native(&[ROLLBACK], &body, 42);
}
