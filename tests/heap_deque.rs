//! pure Vaak heap/dequeの参照実装・VM・STEEL差分試験。

use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};
use std::fmt::Write as _;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use vaak::interp::Eval;

const HEAP: &str = include_str!("../stdlib/ds/heap_i64.vaak");
const DEQUE: &str = include_str!("../stdlib/ds/deque_i64.vaak");
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
    let program = vaak::parser::parse(&source).expect("heap/dequeを含むソースを解析できる");
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
        std::env::temp_dir().join(format!("vaak-heap-deque-{}-{id}", std::process::id()));
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
fn heapの空操作はparadoxで検査は成功する() {
    reference_and_vm(
        &[HEAP],
        r#"let min_h := min_heap_i64_new() ?? new MinHeapI64(data := [0]);
           let max_h := max_heap_i64_new() ?? new MaxHeapI64(data := [0]);
           if (min_heap_i64_is_empty(min_h) && max_heap_i64_is_empty(max_h) &&
               min_heap_i64_is_valid(min_h) && max_heap_i64_is_valid(max_h) &&
               min_heap_i64_len(min_h) == 0 && max_heap_i64_len(max_h) == 0) 42 else 0 fi"#,
        "値 42",
    );
    reference_and_vm(
        &[HEAP],
        "let h := min_heap_i64_new() ?? new MinHeapI64(data := [0]); min_heap_i64_peek(h)",
        "paradox",
    );
    reference_and_vm(
        &[HEAP],
        "var h := min_heap_i64_new() ?? new MinHeapI64(data := [0]); min_heap_i64_pop(h)",
        "paradox",
    );
    reference_and_vm(
        &[HEAP],
        "var h := max_heap_i64_new() ?? new MaxHeapI64(data := [0]); max_heap_i64_replace(h, 1)",
        "paradox",
    );
}

#[test]
fn heapifyとpush_pop_replaceはmin_maxの順序を保つ() {
    let body = r#"
        let values := [5, -1, 5, 3, -7, 9, 0];
        var min_h := min_heap_i64_from(values) ?? new MinHeapI64(data := [0]);
        var max_h := max_heap_i64_from(values) ?? new MaxHeapI64(data := [0]);
        let old_min := min_heap_i64_replace(min_h, 4) ?? -99;
        let old_max := max_heap_i64_replace(max_h, 4) ?? -99;
        min_heap_i64_push(min_h, 0 - 9223372036854775807 - 1) ?? false;
        max_heap_i64_push(max_h, 9223372036854775807) ?? false;
        let min_first := min_heap_i64_pop(min_h) ?? 0;
        let max_first := max_heap_i64_pop(max_h) ?? 0;
        if (old_min == -7 && old_max == 9 &&
            min_first == (0 - 9223372036854775807 - 1) &&
            max_first == 9223372036854775807 &&
            min_heap_i64_peek(min_h) == -1 && max_heap_i64_peek(max_h) == 5 &&
            min_heap_i64_is_valid(min_h) && max_heap_i64_is_valid(max_h)) 42 else 0 fi
    "#;
    reference_and_vm(&[HEAP], body, "値 42");
    steel_native(&[HEAP], body, 42);
}

#[test]
fn 公開dataを書き換えたheapはheapifyで復元できる() {
    reference_and_vm(
        &[HEAP],
        r#"var h := new MinHeapI64(data := [9, 8, 7, 6, 5, 4, 3]);
           let before := min_heap_i64_is_valid(h);
           min_heap_i64_heapify(h) ?? false;
           if (! before && min_heap_i64_is_valid(h) && min_heap_i64_peek(h) == 3) 42 else 0 fi"#,
        "値 42",
    );
}

#[test]
fn dequeはwrapと成長をまたいで両端順序を保つ() {
    let body = r#"
        var q := deque_i64_with_capacity(2) ?? new DequeI64(data := [0], head := 0, size := 0);
        deque_i64_push_back(q, 1) ?? false;
        deque_i64_push_back(q, 2) ?? false;
        let first := deque_i64_pop_front(q) ?? -1;
        deque_i64_push_back(q, 3) ?? false;
        deque_i64_push_front(q, 0) ?? false;
        deque_i64_push_back(q, 4) ?? false;
        let back := deque_i64_pop_back(q) ?? -1;
        if (first == 1 && back == 4 && deque_i64_len(q) == 3 &&
            deque_i64_get(q, 0) == 0 && deque_i64_get(q, 1) == 2 &&
            deque_i64_get(q, 2) == 3 && deque_i64_capacity(q) == 4 &&
            deque_i64_peek_front(q) == 0 && deque_i64_peek_back(q) == 3 &&
            deque_i64_is_valid(q)) 42 else 0 fi
    "#;
    reference_and_vm(&[DEQUE], body, "値 42");
    steel_native(&[DEQUE], body, 42);
}

#[test]
fn dequeの空と範囲外はparadoxでclearは容量を保つ() {
    reference_and_vm(&[DEQUE], "deque_i64_with_capacity(-1)", "paradox");
    reference_and_vm(
        &[DEQUE],
        "let q := deque_i64_new() ?? new DequeI64(data := [0], head := 0, size := 0); deque_i64_peek_front(q)",
        "paradox",
    );
    reference_and_vm(
        &[DEQUE],
        "var q := deque_i64_new() ?? new DequeI64(data := [0], head := 0, size := 0); deque_i64_pop_back(q)",
        "paradox",
    );
    reference_and_vm(
        &[DEQUE],
        r#"var q := deque_i64_with_capacity(3) ?? new DequeI64(data := [0], head := 0, size := 0);
           deque_i64_push_back(q, 7) ?? false;
           deque_i64_clear(q) ?? false;
           if (deque_i64_len(q) == 0 && deque_i64_capacity(q) == 3 &&
               deque_i64_is_valid(q)) 42 else 0 fi"#,
        "値 42",
    );
    reference_and_vm(
        &[DEQUE],
        "let q := deque_i64_new() ?? new DequeI64(data := [0], head := 0, size := 0); deque_i64_get(q, 0)",
        "paradox",
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

fn check_statement(source: &mut String, expression: &str, expected: i64) {
    writeln!(
        source,
        "if ((({expression}) ?? {MISSING}) != {expected}) ok := false; fi;"
    )
    .unwrap();
}

#[test]
fn heapの決定的ランダム列をrust_oracleと三backendで照合する() {
    let mut random = Deterministic(0x5641_414b_4845_4150);
    let mut min = BinaryHeap::<Reverse<i64>>::new();
    let mut max = BinaryHeap::<i64>::new();
    let mut body = String::from(
        "var min_h := min_heap_i64_new() ?? new MinHeapI64(data := [0]);\n\
         var max_h := max_heap_i64_new() ?? new MaxHeapI64(data := [0]);\n\
         var ok := true;\n",
    );

    for step in 0..128 {
        let value = random.value();
        match random.next() % 10 {
            0 | 1 => {
                min.push(Reverse(value));
                writeln!(
                    body,
                    "if (! (min_heap_i64_push(min_h, {value}) ?? false)) ok := false; fi;"
                )
                .unwrap();
            }
            2 => {
                let expected = min.pop().map(|value| value.0).unwrap_or(MISSING);
                check_statement(&mut body, "min_heap_i64_pop(min_h)", expected);
            }
            3 => {
                let expected = min.peek().map(|value| value.0).unwrap_or(MISSING);
                check_statement(&mut body, "min_heap_i64_peek(min_h)", expected);
            }
            4 => {
                let expected = min.pop().map(|old| {
                    min.push(Reverse(value));
                    old.0
                });
                check_statement(
                    &mut body,
                    &format!("min_heap_i64_replace(min_h, {value})"),
                    expected.unwrap_or(MISSING),
                );
            }
            5 | 6 => {
                max.push(value);
                writeln!(
                    body,
                    "if (! (max_heap_i64_push(max_h, {value}) ?? false)) ok := false; fi;"
                )
                .unwrap();
            }
            7 => {
                let expected = max.pop().unwrap_or(MISSING);
                check_statement(&mut body, "max_heap_i64_pop(max_h)", expected);
            }
            8 => {
                let expected = max.peek().copied().unwrap_or(MISSING);
                check_statement(&mut body, "max_heap_i64_peek(max_h)", expected);
            }
            _ => {
                let expected = max.pop().map(|old| {
                    max.push(value);
                    old
                });
                check_statement(
                    &mut body,
                    &format!("max_heap_i64_replace(max_h, {value})"),
                    expected.unwrap_or(MISSING),
                );
            }
        }
        if step % 8 == 0 {
            body.push_str(
                "if (! min_heap_i64_is_valid(min_h) || ! max_heap_i64_is_valid(max_h)) ok := false; fi;\n",
            );
        }
    }
    writeln!(
        body,
        "if (min_heap_i64_len(min_h) != {} || max_heap_i64_len(max_h) != {}) ok := false; fi;",
        min.len(),
        max.len()
    )
    .unwrap();
    body.push_str(
        "if (ok && min_heap_i64_is_valid(min_h) && max_heap_i64_is_valid(max_h)) 42 else 0 fi",
    );

    reference_and_vm(&[HEAP], &body, "値 42");
    steel_native(&[HEAP], &body, 42);
}

#[test]
fn dequeの決定的ランダム列をrust_oracleと三backendで照合する() {
    let mut random = Deterministic(0x5641_414b_4445_5155);
    let mut model = VecDeque::<i64>::new();
    let mut body = String::from(
        "var q := deque_i64_new() ?? new DequeI64(data := [0], head := 0, size := 0);\n\
         var ok := true;\n",
    );

    for step in 0..128 {
        let value = random.value();
        match random.next() % 9 {
            0 | 1 => {
                model.push_back(value);
                writeln!(
                    body,
                    "if (! (deque_i64_push_back(q, {value}) ?? false)) ok := false; fi;"
                )
                .unwrap();
            }
            2 => {
                model.push_front(value);
                writeln!(
                    body,
                    "if (! (deque_i64_push_front(q, {value}) ?? false)) ok := false; fi;"
                )
                .unwrap();
            }
            3 => check_statement(
                &mut body,
                "deque_i64_pop_front(q)",
                model.pop_front().unwrap_or(MISSING),
            ),
            4 => check_statement(
                &mut body,
                "deque_i64_pop_back(q)",
                model.pop_back().unwrap_or(MISSING),
            ),
            5 => check_statement(
                &mut body,
                "deque_i64_peek_front(q)",
                model.front().copied().unwrap_or(MISSING),
            ),
            6 => check_statement(
                &mut body,
                "deque_i64_peek_back(q)",
                model.back().copied().unwrap_or(MISSING),
            ),
            7 => {
                let index = (random.next() % (model.len() as u64 + 2)) as i64 - 1;
                let expected = usize::try_from(index)
                    .ok()
                    .and_then(|index| model.get(index).copied())
                    .unwrap_or(MISSING);
                check_statement(&mut body, &format!("deque_i64_get(q, {index})"), expected);
            }
            _ if step % 17 == 0 => {
                model.clear();
                body.push_str("if (! (deque_i64_clear(q) ?? false)) ok := false; fi;\n");
            }
            _ => {
                model.push_back(value);
                writeln!(
                    body,
                    "if (! (deque_i64_push_back(q, {value}) ?? false)) ok := false; fi;"
                )
                .unwrap();
            }
        }
        body.push_str("if (! deque_i64_is_valid(q)) ok := false; fi;\n");
    }
    writeln!(
        body,
        "if (deque_i64_len(q) != {}) ok := false; fi;",
        model.len()
    )
    .unwrap();
    for (index, value) in model.iter().enumerate() {
        check_statement(&mut body, &format!("deque_i64_get(q, {index})"), *value);
    }
    body.push_str("if (ok && deque_i64_is_valid(q)) 42 else 0 fi");

    reference_and_vm(&[DEQUE], &body, "値 42");
    steel_native(&[DEQUE], &body, 42);
}
