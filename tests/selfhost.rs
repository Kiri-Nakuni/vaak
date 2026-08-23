//! Vaak-on-Vaak の最小骨格を三つの実行系で照合する。

use std::process::Command;

const EXAMPLE: &str = "examples/vaak/07-セルフホスト骨格.vaak";
const EXAMPLE_MARKER: &str = "% --- 実行例 ---";

fn checked(source: &str) -> vaak::ast::Program {
    let program = vaak::parser::parse(source).expect("セルフホスト例を解析できる");
    let errors: Vec<_> = vaak::check::check(&program)
        .into_iter()
        .chain(vaak::types::check_types(&program))
        .map(|error| error.msg)
        .collect();
    assert!(errors.is_empty(), "セルフホスト例: {errors:#?}");
    program
}

fn reference(program: &vaak::ast::Program) -> i128 {
    match vaak::interp::Interp::new().run(program) {
        Ok(vaak::interp::Eval::Value(value)) => value.as_int().expect("整数を返す"),
        other => panic!("参照実装: {other:?}"),
    }
}

fn vm(program: &vaak::ast::Program) -> i128 {
    let bytecode = vaak::vm::compile(program).expect("VM へ翻訳できる");
    match vaak::vm::run_program(&bytecode) {
        Ok(vaak::interp::Eval::Value(value)) => value.as_int().expect("整数を返す"),
        other => panic!("VM: {other:?}"),
    }
}

fn core() -> String {
    let source = std::fs::read_to_string(EXAMPLE).expect("セルフホスト例を読める");
    source
        .split_once(EXAMPLE_MARKER)
        .expect("実行例の境界がある")
        .0
        .to_owned()
}

#[test]
fn 字句解析から反復評価まで参照実装とvmが一致する() {
    let source = std::fs::read_to_string(EXAMPLE).expect("セルフホスト例を読める");
    let program = checked(&source);
    assert_eq!(reference(&program), 42);
    assert_eq!(vm(&program), 42);
}

#[test]
fn 深い構文木も組み込みの再帰なしで評価できる() {
    // 左へ 256 段伸びる木。ゲストの評価器は host call stack ではなく
    // `work_nodes` / `expanded` の二配列へ frame を積む。
    let depth = 256usize;
    let mut expression = "1".to_owned();
    for _ in 0..depth {
        expression = format!("({expression} + 1)");
    }
    let source = format!(
        "{}\nlet input := {:?};\nlet result := compile_and_run(input);\n\
         if (result.ok && result.value == {}) result.max_work else 0 fi",
        core(),
        expression,
        depth + 1
    );
    let program = checked(&source);
    // 左深い二項木では「復帰 frame、右、左」が一段ごとに二枠ずつ増える。
    let expected = (depth * 2 + 1) as i128;
    assert_eq!(reference(&program), expected);
    assert_eq!(vm(&program), expected);
}

#[test]
fn セルフホスト骨格をsteelへ翻訳できる() {
    let source = std::fs::read_to_string(EXAMPLE).expect("セルフホスト例を読める");
    let ir = vaak::steel::compile(&checked(&source))
        .unwrap_or_else(|error| panic!("STEEL の LLVM IR へ翻訳できない: {}", error.msg));
    assert!(ir.contains("define i32 @main()"));

    // clang がある環境では生成した機械語まで照合する。
    if Command::new("clang").arg("--version").output().is_err() {
        return;
    }
    let directory = std::env::temp_dir().join(format!("vaak-selfhost-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("一時ディレクトリを作れる");
    let llvm = directory.join("selfhost.ll");
    let executable = directory.join(if cfg!(windows) {
        "selfhost.exe"
    } else {
        "selfhost"
    });
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
    std::fs::remove_dir_all(directory).expect("一時ディレクトリを片付けられる");
}
