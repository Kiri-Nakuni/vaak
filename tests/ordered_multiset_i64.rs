//! 圧縮済みuniverseのi64 ordered multisetを独立vector oracleと三backendで照合する。

use std::fmt::Write as _;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use vaak::interp::Eval;

const MULTISET: &str = include_str!("../stdlib/ds/ordered_multiset_i64.vaak");
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
    let program = vaak::parser::parse(&source).expect("ordered multisetを含むsourceを解析できる");
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
        std::env::temp_dir().join(format!("vaak-ordered-multiset-{}-{id}", std::process::id()));
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

fn total(counts: &[i64]) -> i64 {
    counts.iter().sum()
}

fn order_of_key(keys: &[i64], counts: &[i64], value: i64) -> i64 {
    keys.iter()
        .zip(counts)
        .take_while(|(key, _)| **key < value)
        .map(|(_, count)| *count)
        .sum()
}

fn kth(keys: &[i64], counts: &[i64], index: i64) -> i64 {
    let mut remaining = index;
    for (&key, &count) in keys.iter().zip(counts) {
        if remaining < count {
            return key;
        }
        remaining -= count;
    }
    panic!("indexは総数未満")
}

#[test]
fn sourceは単独で前置きできる() {
    reference_and_vm(&[MULTISET], "42", "値 42");
    let source = checked(&[MULTISET], "42");
    let program = vaak::parser::parse(&source).expect("構文");
    vaak::steel::compile(&program).unwrap_or_else(|error| panic!("STEEL: {}", error.msg));
}

#[test]
fn ordered_multisetはuniverseを複製し重複と順位を保つ() {
    let body = r#"
        var keys := [(0 - 9223372036854775807 - 1), -2, 5, 9223372036854775807];
        var set := ordered_multiset_i64_new(keys) ?? new OrderedMultisetI64(
            keys := [], fenwick := [], total := 0
        );
        keys[1] := 0;
        ordered_multiset_i64_insert(set, -2) ?? false;
        ordered_multiset_i64_insert(set, -2) ?? false;
        ordered_multiset_i64_insert(set, 5) ?? false;
        ordered_multiset_i64_insert(set, 9223372036854775807) ?? false;
        ordered_multiset_i64_insert(set, 9223372036854775807) ?? false;
        ordered_multiset_i64_insert(set, 9223372036854775807) ?? false;
        let erased := ordered_multiset_i64_erase_one(set, -2) ?? false;
        let absent := ordered_multiset_i64_erase_one(
            set, (0 - 9223372036854775807 - 1)
        ) ?? true;
        if (ordered_multiset_i64_is_valid(set) &&
            ordered_multiset_i64_universe_len(set) == 4 &&
            ordered_multiset_i64_len(set) == 5 && ! ordered_multiset_i64_is_empty(set) &&
            erased && ! absent && ordered_multiset_i64_count(set, -2) == 1 &&
            ordered_multiset_i64_count(set, 4) == 0 &&
            ordered_multiset_i64_contains(set, 5) &&
            ! ordered_multiset_i64_contains(set, 4) &&
            ordered_multiset_i64_order_of_key(set, -2) == 0 &&
            ordered_multiset_i64_order_of_key(set, 5) == 1 &&
            ordered_multiset_i64_order_of_key(set, 6) == 2 &&
            ordered_multiset_i64_kth(set, 0) == -2 &&
            ordered_multiset_i64_kth(set, 1) == 5 &&
            ordered_multiset_i64_kth(set, 4) == 9223372036854775807) 42 else 0 fi
    "#;
    reference_and_vm(&[MULTISET], body, "値 42");
    steel_native(&[MULTISET], body, 42);
}

#[test]
fn 空universeと不正universeと範囲外kthを分ける() {
    let valid = r#"
        let keys : i64 array := new i64 array(0, 0);
        let set := ordered_multiset_i64_new(keys) ?? new OrderedMultisetI64(
            keys := [0], fenwick := [1], total := 1
        );
        if (ordered_multiset_i64_is_valid(set) &&
            ordered_multiset_i64_universe_len(set) == 0 &&
            ordered_multiset_i64_len(set) == 0 && ordered_multiset_i64_is_empty(set) &&
            ordered_multiset_i64_count(set, 7) == 0 &&
            ordered_multiset_i64_order_of_key(set, 7) == 0) 42 else 0 fi
    "#;
    reference_and_vm(&[MULTISET], valid, "値 42");
    steel_native(&[MULTISET], valid, 42);

    for body in [
        "let keys := [1, 1]; ordered_multiset_i64_new(keys)",
        "let keys := [2, 1]; ordered_multiset_i64_new(keys)",
        "let keys := [1, 3]; var set := ordered_multiset_i64_new(keys) ?? new OrderedMultisetI64(keys := [], fenwick := [], total := 0); ordered_multiset_i64_insert(set, 2)",
        "let keys := [1, 3]; var set := ordered_multiset_i64_new(keys) ?? new OrderedMultisetI64(keys := [], fenwick := [], total := 0); ordered_multiset_i64_erase_one(set, 2)",
        "let keys := [1, 3]; let set := ordered_multiset_i64_new(keys) ?? new OrderedMultisetI64(keys := [], fenwick := [], total := 0); ordered_multiset_i64_kth(set, 0)",
        "let keys := [1]; var set := ordered_multiset_i64_new(keys) ?? new OrderedMultisetI64(keys := [], fenwick := [], total := 0); ordered_multiset_i64_insert(set, 1) ?? false; ordered_multiset_i64_kth(set, -1)",
        "let keys := [1]; var set := ordered_multiset_i64_new(keys) ?? new OrderedMultisetI64(keys := [], fenwick := [], total := 0); ordered_multiset_i64_insert(set, 1) ?? false; ordered_multiset_i64_kth(set, 1)",
    ] {
        reference_and_vm(&[MULTISET], body, "paradox");
    }
}

#[test]
fn 失敗mutationは個数を変えず公開欄破損を検出する() {
    let body = r#"
        let keys := [1, 3];
        var set := ordered_multiset_i64_new(keys) ?? new OrderedMultisetI64(
            keys := [], fenwick := [], total := 0
        );
        ordered_multiset_i64_insert(set, 1) ?? false;
        let outside := ordered_multiset_i64_insert(set, 2) ?? false;
        let outside_erase := ordered_multiset_i64_erase_one(set, 2) ?? false;
        var full := new OrderedMultisetI64(
            keys := [9],
            fenwick := [9223372036854775807], total := 9223372036854775807
        );
        let overflow := ordered_multiset_i64_insert(full, 9) ?? false;
        let full_valid := ordered_multiset_i64_is_valid(full);
        var broken_keys := new OrderedMultisetI64(
            keys := [2, 1], fenwick := [0, 0], total := 0
        );
        var broken_counts := new OrderedMultisetI64(
            keys := [1, 2], fenwick := [0, -1], total := 0
        );
        if (! outside && ! outside_erase && ordered_multiset_i64_len(set) == 1 &&
            ordered_multiset_i64_count(set, 1) == 1 && ! overflow && full_valid &&
            ordered_multiset_i64_len(full) == 9223372036854775807 &&
            ! ordered_multiset_i64_is_valid(broken_keys) &&
            ! ordered_multiset_i64_is_valid(broken_counts)) 42 else 0 fi
    "#;
    reference_and_vm(&[MULTISET], body, "値 42");
    steel_native(&[MULTISET], body, 42);
}

#[test]
fn 決定的操作列をrust_vector_oracleと三backendで照合する() {
    let keys: Vec<i64> = (0..17).map(|index| index * 3 - 24).collect();
    let mut counts = vec![0_i64; keys.len()];
    let mut random = Deterministic(0x4f52_4445_5245_444d);
    let mut body = format!(
        "let keys := [{}];\nvar set := ordered_multiset_i64_new(keys) ?? new OrderedMultisetI64(keys := [], fenwick := [], total := 0);\nvar ok := true;\n",
        keys.iter().map(i64::to_string).collect::<Vec<_>>().join(", ")
    );

    for step in 0..280 {
        let rank = (random.next() % keys.len() as u64) as usize;
        match random.next() % 8 {
            0..=2 => {
                counts[rank] += 1;
                writeln!(
                    body,
                    "if (! (ordered_multiset_i64_insert(set, {}) ?? false)) ok := false; fi;",
                    keys[rank]
                )
                .unwrap();
            }
            3 => {
                let expected = counts[rank] > 0;
                if expected {
                    counts[rank] -= 1;
                }
                writeln!(
                    body,
                    "if ((ordered_multiset_i64_erase_one(set, {}) ?? false) != {}) ok := false; fi;",
                    keys[rank],
                    bool_literal(expected)
                )
                .unwrap();
            }
            4 => {
                let value = (random.next() % 67) as i64 - 33;
                let expected = keys
                    .iter()
                    .position(|key| *key == value)
                    .map_or(0, |index| counts[index]);
                writeln!(
                    body,
                    "if ((ordered_multiset_i64_count(set, {value}) ?? -1) != {expected}) ok := false; fi;"
                )
                .unwrap();
            }
            5 => {
                let value = (random.next() % 67) as i64 - 33;
                let expected = order_of_key(&keys, &counts, value);
                writeln!(
                    body,
                    "if ((ordered_multiset_i64_order_of_key(set, {value}) ?? -1) != {expected}) ok := false; fi;"
                )
                .unwrap();
            }
            6 if total(&counts) > 0 => {
                let index = (random.next() % total(&counts) as u64) as i64;
                let expected = kth(&keys, &counts, index);
                writeln!(
                    body,
                    "if ((ordered_multiset_i64_kth(set, {index}) ?? 999) != {expected}) ok := false; fi;"
                )
                .unwrap();
            }
            _ => {
                writeln!(
                    body,
                    "if (ordered_multiset_i64_insert(set, 1000) ?? false) ok := false; fi;"
                )
                .unwrap();
            }
        }
        if step % 11 == 0 {
            writeln!(
                body,
                "if (! ordered_multiset_i64_is_valid(set) || ordered_multiset_i64_len(set) != {}) ok := false; fi;",
                total(&counts)
            )
            .unwrap();
        }
    }

    for (rank, (&key, &count)) in keys.iter().zip(&counts).enumerate() {
        writeln!(
            body,
            "if ((ordered_multiset_i64_count(set, {key}) ?? -1) != {count}) ok := false; fi;"
        )
        .unwrap();
        let order = counts[..rank].iter().sum::<i64>();
        writeln!(
            body,
            "if ((ordered_multiset_i64_order_of_key(set, {key}) ?? -1) != {order}) ok := false; fi;"
        )
        .unwrap();
    }
    body.push_str("if (ok) 42 else 0 fi");

    reference_and_vm(&[MULTISET], &body, "値 42");
    steel_native(&[MULTISET], &body, 42);
}
