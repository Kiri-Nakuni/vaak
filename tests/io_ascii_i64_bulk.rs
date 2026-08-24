//! host-owned str上のbulk i64 scanner/formatterを独立Rust oracleと三backendで照合する。

use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use vaak::interp::Eval;

const ASCII_I64: &str = include_str!("../stdlib/io/ascii_i64.vaak");
static TEMP_ID: AtomicU64 = AtomicU64::new(0);

fn source(body: &str) -> String {
    format!("{ASCII_I64}\n{body}")
}

#[track_caller]
fn checked(body: &str) -> String {
    let source = source(body);
    let program = vaak::parser::parse(&source).expect("bulk ASCII i64を含むsourceを解析できる");
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
        std::env::temp_dir().join(format!("vaak-io-ascii-bulk-{}-{id}", std::process::id()));
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
fn read_n_intoはcaller配列の指定範囲を連続して埋める() {
    let body = r#"
        let input := "\t-42 +17\n0 99 ";
        var at := 0;
        var values := [8, 8, 8, 8, 8];
        let filled := io_ascii_i64_read_n_into(input, at, values, 1, 3) ?? false;
        let more := io_ascii_i64_has_next(input, at) ?? false;
        let tail := io_ascii_i64_read(input, at) ?? 0;
        if (filled && more && tail == 99 &&
            values[0] == 8 && values[1] == -42 && values[2] == 17 &&
            values[3] == 0 && values[4] == 8 &&
            ! io_ascii_i64_has_next(input, at)) 42 else 0 fi
    "#;
    reference_and_vm(body, "値 42");
    steel_native(body, 42);

    let whitespace = r#"
        let bytes : u8 array := [
            0x31, 0x09, 0x32, 0x0a, 0x33, 0x0b,
            0x34, 0x0c, 0x35, 0x0d, 0x36, 0x20
        ];
        let input := new str(bytes);
        var at := 0;
        var values : i64 array := new i64 array(6, 0);
        io_ascii_i64_read_n_into(input, at, values, 0, 6) ?? false;
        if (values[0] == 1 && values[1] == 2 && values[2] == 3 &&
            values[3] == 4 && values[4] == 5 && values[5] == 6 &&
            ! io_ascii_i64_has_next(input, at)) 42 else 0 fi
    "#;
    reference_and_vm(whitespace, "値 42");
    steel_native(whitespace, 42);
}

#[test]
fn read_n_intoのtoken失敗は成功prefixだけを確定する() {
    let body = r#"
        let input := "1 2x 3";
        var at := 0;
        var values := [9, 9, 9];
        let filled := io_ascii_i64_read_n_into(input, at, values, 0, 3) ?? false;
        if (! filled && at == 1 && values[0] == 1 &&
            values[1] == 9 && values[2] == 9) 42 else 0 fi
    "#;
    reference_and_vm(body, "値 42");
    steel_native(body, 42);

    let invalid_range = r#"
        let input := "1 2";
        var at := 0;
        var values := [7, 8];
        let filled := io_ascii_i64_read_n_into(input, at, values, 1, 2) ?? false;
        if (! filled && at == 0 && values[0] == 7 && values[1] == 8) 42 else 0 fi
    "#;
    reference_and_vm(invalid_range, "値 42");
    steel_native(invalid_range, 42);
}

#[test]
fn format_rangeはscratchを一度確保して区切りを値間だけへ置く() {
    let body = r#"
        let values := [(0 - 9223372036854775807 - 1), 0, 9223372036854775807];
        let text := io_ascii_i64_format_range(values, 0, 3, 0x20) ?? "x";
        let tail := io_ascii_i64_format_range(values, 1, 3, 0x0a) ?? "x";
        let empty := io_ascii_i64_format_range(values, 2, 2, 0x20) ?? "x";
        if (text.len() == 42 && text[0] == 0x2d && text[1] == 0x39 &&
            text[19] == 0x38 && text[20] == 0x20 && text[21] == 0x30 &&
            text[22] == 0x20 && text[41] == 0x37 &&
            tail.len() == 21 && tail[0] == 0x30 && tail[1] == 0x0a &&
            tail[20] == 0x37 && empty.len() == 0) 42 else 0 fi
    "#;
    reference_and_vm(body, "値 42");
    steel_native(body, 42);
}

#[test]
fn bulk_apiは負数範囲外と非ascii区切りをparadoxにする() {
    for body in [
        "let input := \"1\"; var at := 0; var values := [0]; io_ascii_i64_read_n_into(input, at, values, -1, 1)",
        "let input := \"1\"; var at := 0; var values := [0]; io_ascii_i64_read_n_into(input, at, values, 0, -1)",
        "let input := \"1\"; var at := -1; var values := [0]; io_ascii_i64_read_n_into(input, at, values, 0, 1)",
        "let values := [1]; io_ascii_i64_format_range(values, -1, 1, 0x20)",
        "let values := [1]; io_ascii_i64_format_range(values, 1, 0, 0x20)",
        "let values := [1]; io_ascii_i64_format_range(values, 0, 2, 0x20)",
        "let values := [1]; io_ascii_i64_format_range(values, 0, 1, 0x80)",
    ] {
        reference_and_vm(body, "paradox");
    }
}

fn vaak_i64(value: i64) -> String {
    if value == i64::MIN {
        "(0 - 9223372036854775807 - 1)".into()
    } else {
        value.to_string()
    }
}

fn hash_values(values: &[i64]) -> i64 {
    values.iter().fold(0_i64, |hash, value| {
        hash.wrapping_mul(1_000_003).wrapping_add(*value)
    })
}

fn hash_bytes(bytes: &[u8]) -> i64 {
    bytes.iter().fold(0_i64, |hash, byte| {
        hash.wrapping_mul(257).wrapping_add(i64::from(*byte))
    })
}

#[test]
fn 決定的整数列をrust_parse_format_oracleと三backendで照合する() {
    let mut values: Vec<i64> = (0..192)
        .map(|index| ((index * 104_729 + 97) % 200_003) as i64 - 100_001)
        .collect();
    values[0] = i64::MIN;
    values[1] = i64::MAX;
    values[2] = 0;

    let mut input = String::new();
    for (index, value) in values.iter().enumerate() {
        input.push_str(match index % 3 {
            0 => "\t",
            1 => " ",
            _ => "\n",
        });
        if *value >= 0 && index % 3 == 0 {
            input.push('+');
        }
        input.push_str(&value.to_string());
    }
    input.push_str(" \n");
    let formatted = values
        .iter()
        .map(i64::to_string)
        .collect::<Vec<_>>()
        .join("|");

    let body = format!(
        r#"
            let input := {input:?};
            var at := 0;
            var values : i64 array := new i64 array({length}, 0);
            let filled := io_ascii_i64_read_n_into(input, at, values, 0, values.len()) ?? false;
            var value_hash := 0;
            nfor (index, 0, values.len()) {{
                value_hash := value_hash * 1000003 + (values[index] ?? 0);
            }};
            let output := io_ascii_i64_format_range(values, 0, values.len(), 0x7c) ?? "";
            var byte_hash := 0;
            nfor (index, 0, output.len()) {{
                byte_hash := byte_hash * 257 + ((output[index] ?? 0) -> i64);
            }};
            if (filled && ! io_ascii_i64_has_next(input, at) &&
                value_hash == {value_hash} && output.len() == {output_length} &&
                byte_hash == {byte_hash}) 42 else 0 fi
        "#,
        length = values.len(),
        value_hash = vaak_i64(hash_values(&values)),
        output_length = formatted.len(),
        byte_hash = vaak_i64(hash_bytes(formatted.as_bytes())),
    );

    reference_and_vm(&body, "値 42");
    steel_native(&body, 42);
}
