//! pure Vaakの固定i64 sort・sorted unique・座標圧縮を三backendで照合する。

use std::fmt::Write as _;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use vaak::interp::Eval;

const BINARY: &str = include_str!("../stdlib/array/i64/search_binary.vaak");
const INSERTION: &str = include_str!("../stdlib/array/i64/sort/insertion.vaak");
const HEAP: &str = include_str!("../stdlib/array/i64/sort/heap.vaak");
const MERGE: &str = include_str!("../stdlib/array/i64/sort/merge.vaak");
const SORTED_UNIQUE: &str = include_str!("../stdlib/array/i64/partition/sorted_unique.vaak");
const COMPRESS: &str = include_str!("../stdlib/array/i64/compress.vaak");
static TEMP_ID: AtomicU64 = AtomicU64::new(0);

fn source(parts: &[&str], body: &str) -> String {
    let mut source = String::new();
    for part in parts {
        source.push_str(part);
        source.push('\n');
    }
    source.push_str(body);
    source
}

#[track_caller]
fn checked(parts: &[&str], body: &str) -> String {
    let source = source(parts, body);
    let program = vaak::parser::parse(&source).expect("orderingライブラリを含むsourceを解析できる");
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
        std::env::temp_dir().join(format!("vaak-array-ordering-{}-{id}", std::process::id()));
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

fn sort_oracle_body(function: &str, input: &[i64]) -> String {
    let mut expected = input.to_vec();
    expected.sort();
    let mut body = format!(
        "var xs := {};\n{function}(xs) ?? false;\nvar result := 42;\nif (xs.len() != {}) result := 99; fi;\n",
        array_literal(input),
        expected.len()
    );
    for (index, value) in expected.iter().enumerate() {
        writeln!(
            body,
            "if (result == 42 && (xs[{index}] ?? 0) != {}) result := {}; fi;",
            vaak_i64(*value),
            100 + index
        )
        .unwrap();
    }
    body.push_str("result");
    body
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
    let mut random = Deterministic(0x5641_414b_534f_5254);
    let mut values = Vec::with_capacity(length);
    for index in 0..length {
        let value = match index {
            0 => i64::MIN,
            1 => i64::MAX,
            _ => (random.next() % 41) as i64 - 20,
        };
        values.push(value);
    }
    values
}

#[test]
fn orderingの各sourceは依存を明示して同時に前置きできる() {
    reference_and_vm(
        &[INSERTION, HEAP, MERGE, SORTED_UNIQUE, BINARY, COMPRESS],
        "42",
        "値 42",
    );
    let source = checked(
        &[INSERTION, HEAP, MERGE, SORTED_UNIQUE, BINARY, COMPRESS],
        "42",
    );
    let program = vaak::parser::parse(&source).expect("構文");
    vaak::steel::compile(&program).unwrap_or_else(|error| panic!("STEEL: {}", error.msg));
}

#[test]
fn 三種sortは半開区間だけを昇順へ変える() {
    for (library, function) in [
        (INSERTION, "array_i64_insertion_sort_range"),
        (HEAP, "array_i64_heap_sort_range"),
        (MERGE, "array_i64_merge_sort_range"),
    ] {
        let body = format!(
            r#"var xs := [99, 5, 3, 3, -1, 7, 88];
               {function}(xs, 1, 6) ?? false;
               if (xs[0] == 99 && xs[1] == -1 && xs[2] == 3 &&
                   xs[3] == 3 && xs[4] == 5 && xs[5] == 7 && xs[6] == 88) 42 else 0 fi"#
        );
        reference_and_vm(&[library], &body, "値 42");
        steel_native(&[library], &body, 42);
    }
}

#[test]
fn 三種sortは空と一要素を許し不正範囲では変更しない() {
    for (library, function) in [
        (INSERTION, "array_i64_insertion_sort_range"),
        (HEAP, "array_i64_heap_sort_range"),
        (MERGE, "array_i64_merge_sort_range"),
    ] {
        let body = format!(
            r#"var empty : i64 array := new i64 array(0, 0);
               var xs := [3, 2, 1];
               let empty_ok := {function}(empty, 0, 0) ?? false;
               let one_ok := {function}(xs, 1, 2) ?? false;
               let invalid := {function}(xs, -1, 2) ?? false;
               if (empty_ok && one_ok && ! invalid &&
                   xs[0] == 3 && xs[1] == 2 && xs[2] == 1) 42 else 0 fi"#
        );
        reference_and_vm(&[library], &body, "値 42");
        steel_native(&[library], &body, 42);
    }
}

#[test]
fn insertion_sortをrust_oracleと三backendで照合する() {
    let body = sort_oracle_body("array_i64_insertion_sort", &random_values(72));
    reference_and_vm(&[INSERTION], &body, "値 42");
    steel_native(&[INSERTION], &body, 42);
}

#[test]
fn heap_sortをrust_oracleと三backendで照合する() {
    let body = sort_oracle_body("array_i64_heap_sort", &random_values(96));
    reference_and_vm(&[HEAP], &body, "値 42");
    steel_native(&[HEAP], &body, 42);
}

#[test]
fn merge_sortをrust_oracleと三backendで照合する() {
    let body = sort_oracle_body("array_i64_merge_sort", &random_values(96));
    reference_and_vm(&[MERGE], &body, "値 42");
    steel_native(&[MERGE], &body, 42);
}

#[test]
fn sorted_uniqueは昇順を検査してから重複を縮める() {
    let body = r#"
        var xs := [
            (0 - 9223372036854775807 - 1),
            (0 - 9223372036854775807 - 1), -2, -2, 0,
            9223372036854775807, 9223372036854775807
        ];
        let length := array_i64_sorted_unique_in_place(xs) ?? -1;
        var empty : i64 array := new i64 array(0, 0);
        let empty_length := array_i64_sorted_unique_in_place(empty) ?? -1;
        var unsorted := [2, 1, 1];
        let rejected := array_i64_sorted_unique_in_place(unsorted) ?? -1;
        if (length == 4 && xs.len() == 4 &&
            xs[0] == (0 - 9223372036854775807 - 1) && xs[1] == -2 &&
            xs[2] == 0 && xs[3] == 9223372036854775807 &&
            empty_length == 0 && empty.len() == 0 && rejected == -1 &&
            unsorted[0] == 2 && unsorted[1] == 1 && unsorted[2] == 1) 42 else 0 fi
    "#;
    reference_and_vm(&[SORTED_UNIQUE], body, "値 42");
    steel_native(&[SORTED_UNIQUE], body, 42);
}

fn compression_parts() -> [&'static str; 4] {
    [BINARY, MERGE, SORTED_UNIQUE, COMPRESS]
}

#[test]
fn 座標圧縮は元配列を保ちunique_valuesとrankを往復する() {
    let body = r#"
        let xs := [9223372036854775807, -7, 9223372036854775807, 0, -7,
                   (0 - 9223372036854775807 - 1)];
        let compressed := array_i64_coordinate_compress(xs) ??
            new CoordinateCompressionI64(unique_values := [0], ranks := [0]);
        if (coordinate_compression_i64_is_valid(compressed) &&
            coordinate_compression_i64_len(compressed) == 4 &&
            compressed.unique_values[0] == (0 - 9223372036854775807 - 1) &&
            compressed.unique_values[1] == -7 && compressed.unique_values[2] == 0 &&
            compressed.unique_values[3] == 9223372036854775807 &&
            compressed.ranks[0] == 3 && compressed.ranks[1] == 1 &&
            compressed.ranks[2] == 3 && compressed.ranks[3] == 2 &&
            compressed.ranks[4] == 1 && compressed.ranks[5] == 0 &&
            coordinate_compression_i64_rank_of(compressed, 0) == 2 &&
            coordinate_compression_i64_value_at(compressed, 1) == -7 &&
            xs[0] == 9223372036854775807 && xs[1] == -7) 42 else 0 fi
    "#;
    reference_and_vm(&compression_parts(), body, "値 42");
    steel_native(&compression_parts(), body, 42);
}

#[test]
fn 座標圧縮は空を通常結果にし欠損値と範囲外rankをparadoxにする() {
    let parts = compression_parts();
    reference_and_vm(
        &parts,
        r#"let xs : i64 array := new i64 array(0, 0);
            let c := array_i64_coordinate_compress(xs) ??
                new CoordinateCompressionI64(unique_values := [0], ranks := [0]);
            if (coordinate_compression_i64_is_valid(c) &&
                c.unique_values.len() == 0 && c.ranks.len() == 0) 42 else 0 fi"#,
        "値 42",
    );
    reference_and_vm(
        &parts,
        "let c := new CoordinateCompressionI64(unique_values := [1, 3], ranks := [0]); coordinate_compression_i64_rank_of(c, 2)",
        "paradox",
    );
    reference_and_vm(
        &parts,
        "let c := new CoordinateCompressionI64(unique_values := [1, 3], ranks := [0]); coordinate_compression_i64_value_at(c, -1)",
        "paradox",
    );
    reference_and_vm(
        &parts,
        "let c := new CoordinateCompressionI64(unique_values := [3, 1], ranks := [0]); coordinate_compression_i64_is_valid(c)",
        "値 0",
    );
    steel_native(
        &parts,
        r#"let c := new CoordinateCompressionI64(unique_values := [1, 3], ranks := [0]);
            let missing := coordinate_compression_i64_rank_of(c, 4) ?? -1;
            let outside := coordinate_compression_i64_value_at(c, -1) ?? -1;
            let broken := new CoordinateCompressionI64(unique_values := [3, 1], ranks := [0]);
            if (missing == -1 && outside == -1 &&
                ! coordinate_compression_i64_is_valid(broken)) 42 else 0 fi"#,
        42,
    );
}

#[test]
fn 座標圧縮の決定的列をrust_oracleと三backendで照合する() {
    let input = random_values(80);
    let mut unique = input.clone();
    unique.sort();
    unique.dedup();
    let ranks: Vec<usize> = input
        .iter()
        .map(|value| unique.binary_search(value).expect("元要素はuniqueにある"))
        .collect();

    let mut body = format!(
        "let xs := {};\nlet c := array_i64_coordinate_compress(xs) ?? new CoordinateCompressionI64(unique_values := [], ranks := []);\nvar ok := coordinate_compression_i64_is_valid(c) && c.unique_values.len() == {} && c.ranks.len() == {};\n",
        array_literal(&input),
        unique.len(),
        ranks.len()
    );
    for (index, value) in unique.iter().enumerate() {
        writeln!(
            body,
            "if ((c.unique_values[{index}] ?? 0) != {}) ok := false; fi;",
            vaak_i64(*value)
        )
        .unwrap();
    }
    for (index, rank) in ranks.iter().enumerate() {
        writeln!(
            body,
            "if ((c.ranks[{index}] ?? -1) != {rank}) ok := false; fi;"
        )
        .unwrap();
    }
    body.push_str("if (ok) 42 else 0 fi");

    reference_and_vm(&compression_parts(), &body, "値 42");
    steel_native(&compression_parts(), &body, 42);
}
