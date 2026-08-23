//! Vaak 自身で書いた Forth 系インタープリターの回帰試験。

use std::process::Command;

const EXAMPLE: &str = "examples/vaak/07-Forth.vaak";

fn parse(source: &str) -> vaak::ast::Program {
    let program = vaak::parser::parse(source).expect("Forth の例を解析できる");
    let errors: Vec<_> = vaak::check::check(&program)
        .into_iter()
        .chain(vaak::types::check_types(&program))
        .map(|error| error.msg)
        .collect();
    assert!(errors.is_empty(), "Forth の例: {errors:#?}");
    program
}

fn program() -> vaak::ast::Program {
    let source = std::fs::read_to_string(EXAMPLE).expect("Forth の例を読める");
    parse(&source)
}

fn program_for(forth: &str) -> vaak::ast::Program {
    let source = std::fs::read_to_string(EXAMPLE).expect("Forth の例を読める");
    let core = source
        .split("let main_program :=")
        .next()
        .expect("実験本体の前を取り出せる");
    parse(&format!(
        "{core}\nlet input_program := {forth:?}; run_forth(input_program)"
    ))
}

fn result(program: &vaak::ast::Program, vm: bool) -> Result<Option<i128>, String> {
    let evaluated = if vm {
        let bytecode = vaak::vm::compile(program).map_err(|error| error.msg)?;
        vaak::vm::run_program(&bytecode).map_err(|error| error.msg)?
    } else {
        vaak::interp::Interp::new()
            .run(program)
            .map_err(|error| error.msg)?
    };
    Ok(match evaluated {
        vaak::interp::Eval::Value(value) => value.as_int(),
        vaak::interp::Eval::Paradox(_) => None,
        other => return Err(format!("予期しない外界面: {other:?}")),
    })
}

fn steel_native(program: &vaak::ast::Program, name: &str) -> Option<i32> {
    let ir = vaak::steel::compile(program)
        .unwrap_or_else(|error| panic!("STEEL の LLVM IR へ翻訳できない: {}", error.msg));
    if Command::new("clang").arg("--version").output().is_err() {
        return None;
    }
    let directory = std::env::temp_dir().join(format!("vaak-forth-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("一時ディレクトリを作れる");
    let llvm = directory.join("program.ll");
    let executable = directory.join(if cfg!(windows) {
        "program.exe"
    } else {
        "program"
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
    std::fs::remove_dir_all(directory).expect("STEEL 試験の一時ディレクトリを片付けられる");
    status.code()
}

#[test]
fn forthが参照実装とvmで同じ値になる() {
    let program = program();
    let reference = result(&program, false).expect("参照実装で走る");
    let vm = result(&program, true).expect("VM で走る");
    assert_eq!(reference, Some(42));
    assert_eq!(vm, reference);
}

#[test]
fn forthのスタック誤りはparadoxになる() {
    for bad in ["1 +", "1 0 /", "unknown", ": unfinished 1"] {
        let program = program_for(bad);
        assert_eq!(result(&program, false).unwrap(), None, "参照実装: {bad}");
        assert_eq!(result(&program, true).unwrap(), None, "VM: {bad}");
    }
}

#[test]
fn forthの定義は前方参照と再定義ができる() {
    for source in [
        ": first second ; : second 21 2 * ; first",
        ": answer 1 ; : answer 42 ; answer",
    ] {
        let program = program_for(source);
        assert_eq!(result(&program, false).unwrap(), Some(42));
        assert_eq!(result(&program, true).unwrap(), Some(42));
    }
}

#[test]
fn forthをsteelへ翻訳できる() {
    let ir = vaak::steel::compile(&program())
        .unwrap_or_else(|error| panic!("STEEL の LLVM IR へ翻訳できない: {}", error.msg));
    assert!(ir.contains("define i32 @main()"));
}

#[test]
#[ignore = "STEEL は grow した alias 配列の記述子を呼び出し元へ共有していない"]
fn steelのalias引数から伸ばした配列が呼び出し元に残る() {
    let program = parse(
        "fn push_one (var stack : i64 array alias) { stack.push(42); };
         var stack : i64 array := new i64 array(0, 0);
         push_one(stack);
         stack.pop() ?? 0",
    );
    assert_eq!(result(&program, false).unwrap(), Some(42));
    assert_eq!(result(&program, true).unwrap(), Some(42));
    let Some(native) = steel_native(&program, "alias-grow") else {
        return;
    };
    assert_eq!(native, 42);
}
