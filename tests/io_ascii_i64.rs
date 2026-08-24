//! hostから一括で渡すstr向けASCII i64補助の差分試験。

use std::process::Command;
use vaak::interp::Eval;

const ASCII_I64: &str = include_str!("../stdlib/io/ascii_i64.vaak");

fn source(body: &str) -> String {
    format!("{ASCII_I64}\n{body}")
}

#[track_caller]
fn checked(body: &str) -> String {
    let source = source(body);
    let program = vaak::parser::parse(&source).expect("ASCII i64補助を含むソースを解析できる");
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

#[test]
fn scannerは空白と符号を読み位置を進める() {
    reference_and_vm(
        r#"let input := "\t -42 +17\n0 ";
           var at := 0;
           let a := io_ascii_i64_read(input, at) ?? 999;
           let b := io_ascii_i64_read(input, at) ?? 999;
           let c := io_ascii_i64_read(input, at) ?? 999;
           if (a == -42 && b == 17 && c == 0 && ! io_ascii_i64_has_next(input, at)) 42 else 0 fi"#,
        "値 42",
    );
}

#[test]
fn scannerはi64の両端を読みoverflowを畳む() {
    reference_and_vm(
        r#"let input := "-9223372036854775808 9223372036854775807";
           var at := 0;
           let low := io_ascii_i64_read(input, at) ?? 0;
           let high := io_ascii_i64_read(input, at) ?? 0;
           if (low == (0 - 9223372036854775807 - 1) &&
               high == 9223372036854775807) 42 else 0 fi"#,
        "値 42",
    );
    for bad in [
        "9223372036854775808",
        "-9223372036854775809",
        "+",
        "-",
        "12x",
        "",
        "   ",
    ] {
        reference_and_vm(
            &format!("let input := {bad:?}; var at := 0; io_ascii_i64_read(input, at)"),
            "paradox",
        );
    }
}

#[test]
fn scannerの失敗はcursorを動かさない() {
    reference_and_vm(
        r#"let input := "  12x"; var at := 0;
           let value := io_ascii_i64_read(input, at) ?? 99;
           if (value == 99 && at == 0) 42 else 0 fi"#,
        "値 42",
    );
    reference_and_vm(
        r#"let input := "1"; var at := -1;
           let value := io_ascii_i64_read(input, at) ?? 99;
           if (value == 99 && at == -1) 42 else 0 fi"#,
        "値 42",
    );
}

#[test]
fn formatterは両端値と区切りを十進asciiへ書く() {
    reference_and_vm(
        r#"var output := "";
           io_ascii_i64_append(output, 0 - 9223372036854775807 - 1) ?? false;
           io_ascii_i64_append_space(output) ?? false;
           io_ascii_i64_append(output, 0) ?? false;
           io_ascii_i64_append_space(output) ?? false;
           io_ascii_i64_append(output, 9223372036854775807) ?? false;
           io_ascii_i64_append_newline(output) ?? false;
           if (output.len() == 43 && output[0] == 45 && output[1] == 57 &&
               output[20] == 32 && output[21] == 48 && output[22] == 32 &&
               output[41] == 55 && output[42] == 10) 42 else 0 fi"#,
        "値 42",
    );
}

#[test]
fn formatterからscannerへの往復をsteelにも渡せる() {
    let body = r#"
        var output := "";
        io_ascii_i64_append(output, -42) ?? false;
        io_ascii_i64_append_space(output) ?? false;
        io_ascii_i64_append(output, 17) ?? false;
        var at := 0;
        let a := io_ascii_i64_read(output, at) ?? 0;
        let b := io_ascii_i64_read(output, at) ?? 0;
        if (a == -42 && b == 17 && ! io_ascii_i64_has_next(output, at)) 42 else 0 fi
    "#;
    reference_and_vm(body, "値 42");

    let source = checked(body);
    let program = vaak::parser::parse(&source).expect("構文");
    let ir = vaak::steel::compile(&program).unwrap_or_else(|error| panic!("STEEL: {}", error.msg));
    if Command::new("clang").arg("--version").output().is_err() {
        return;
    }
    let directory = std::env::temp_dir().join(format!("vaak-io-ascii-i64-{}", std::process::id()));
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
    assert_eq!(status.code(), Some(42));
}
