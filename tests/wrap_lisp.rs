//! TokenPos を包んだ LISP 例の小さな回帰試験。

use vaak::interp::Eval;

const EXAMPLE: &str = "examples/vaak/06-LISP.vaak";

fn statics(source: &str) -> Vec<String> {
    let program = vaak::parser::parse(source).expect("構文を読める");
    vaak::check::check(&program)
        .into_iter()
        .chain(vaak::types::check_types(&program))
        .map(|error| error.msg)
        .collect()
}

#[test]
fn token位置を包んだlispが三経路へ通る() {
    let source = std::fs::read_to_string(EXAMPLE).expect("LISP の例を読める");
    let program = vaak::parser::parse(&source).expect("LISP の例を解析できる");
    assert!(statics(&source).is_empty());

    let reference = vaak::interp::Interp::new()
        .run(&program)
        .expect("参照実装で走る");
    assert!(matches!(reference, Eval::Value(ref value) if value.as_int() == Some(42)));

    let bytecode = vaak::vm::compile(&program).expect("VM へ翻訳できる");
    let vm = vaak::vm::run_program(&bytecode).expect("VM で走る");
    assert!(matches!(vm, Eval::Value(ref value) if value.as_int() == Some(42)));

    let ir = vaak::steel::compile(&program)
        .unwrap_or_else(|error| panic!("STEEL へ翻訳できる: {}", error.msg));
    assert!(ir.contains("define i32 @main()"));
}

#[test]
fn token位置には裸の整数を渡せない() {
    let errors = statics(
        "wrap TokenPos = i64;
         fn take (pos : TokenPos) { pos -> i64 } -> i64;
         let environment_index : i64 := 3;
         take(environment_index);",
    );
    assert!(!errors.is_empty(), "異なる用途の i64 は暗黙に混ざらない");
}
