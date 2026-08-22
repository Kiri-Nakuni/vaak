//! Vaak 自身で書いた S 式インタープリターの回帰試験。

use std::process::Command;

const EXAMPLE: &str = "examples/vaak/06-LISP.vaak";

fn program() -> vaak::ast::Program {
    let source = std::fs::read_to_string(EXAMPLE).expect("LISP の例を読める");
    let program = vaak::parser::parse(&source).expect("LISP の例を解析できる");
    let errors: Vec<_> = vaak::check::check(&program)
        .into_iter()
        .chain(vaak::types::check_types(&program))
        .map(|error| error.msg)
        .collect();
    assert!(errors.is_empty(), "LISP の例: {errors:#?}");
    program
}

#[test]
fn lispが参照実装とvmで同じ値になる() {
    let program = program();
    let reference = match vaak::interp::Interp::new().run(&program) {
        Ok(vaak::interp::Eval::Value(value)) => value.as_int(),
        other => panic!("参照実装: {other:?}"),
    };
    let bytecode = vaak::vm::compile(&program).expect("VM へ翻訳できる");
    let vm = match vaak::vm::run_program(&bytecode) {
        Ok(vaak::interp::Eval::Value(value)) => value.as_int(),
        other => panic!("VM: {other:?}"),
    };
    assert_eq!(reference, Some(42));
    assert_eq!(vm, reference);
}

#[test]
fn lispをsteelへ翻訳できる() {
    let ir = vaak::steel::compile(&program())
        .unwrap_or_else(|error| panic!("STEEL の LLVM IR へ翻訳できない: {}", error.msg));
    assert!(ir.contains("define i32 @main()"));

    // clang がある環境では、生成物まで動かして終了コードを照合する。
    if Command::new("clang").arg("--version").output().is_err() {
        return;
    }
    let directory = std::env::temp_dir().join(format!("vaak-lisp-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("一時ディレクトリを作れる");
    let llvm = directory.join("lisp.ll");
    let executable = directory.join(if cfg!(windows) { "lisp.exe" } else { "lisp" });
    std::fs::write(&llvm, ir).expect("LLVM IR を書ける");
    let compiled = Command::new("clang")
        .arg("-O2")
        .arg("-o")
        .arg(&executable)
        .arg(&llvm)
        .output()
        .expect("clang を呼べる");
    assert!(
        compiled.status.success(),
        "clang: {}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let status = Command::new(executable)
        .status()
        .expect("STEEL の生成物を実行できる");
    assert_eq!(status.code(), Some(42));
    std::fs::remove_dir_all(directory).expect("LISP 試験の一時ディレクトリを片付けられる");
}
