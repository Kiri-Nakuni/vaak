//! Vaakで書いたSTEEL vertical sliceとfallback interpreterの差分試験。

use std::process::Command;
use vaak::interp::Eval;
use vaak::value::Value;

const COMPILER: &str = include_str!("../examples/experiments/steel_subset_compiler.vaak");
const STRING_PROBE: &str = include_str!("../examples/experiments/steel_selfhost_string_probe.vaak");
const INTERPRETER: &str =
    include_str!("../examples/experiments/steel_vaak_bytecode_interpreter.vaak");
const NESTED_COMPILER_PROBE: &str =
    include_str!("../examples/experiments/steel_subset_compile_nested_probe.vaak");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OracleEval {
    ok: bool,
    value: i64,
    error: i64,
    steps: i64,
}

fn oracle(ok: bool, value: i64, error: i64, steps: i64) -> OracleEval {
    OracleEval {
        ok,
        value,
        error,
        steps,
    }
}

fn rust_oracle(code: &[i64], slot_count: i64, max_steps: i64) -> OracleEval {
    if slot_count < 0 || max_steps <= 0 {
        return oracle(false, 0, 1, 0);
    }
    let mut values = Vec::<i64>::new();
    let mut oks = Vec::<bool>::new();
    let mut slot_values = vec![0_i64; slot_count as usize];
    let mut slot_oks = vec![false; slot_count as usize];
    let mut pc = 0_i64;
    let mut steps = 0_i64;
    loop {
        if steps >= max_steps {
            return oracle(false, 0, 8, steps);
        }
        let Some(&opcode) = usize::try_from(pc).ok().and_then(|at| code.get(at)) else {
            return oracle(false, 0, 2, steps);
        };
        pc += 1;
        steps += 1;
        match opcode {
            0 => {
                if values.len() != 1 || oks.len() != 1 {
                    return oracle(false, 0, 9, steps);
                }
                return oracle(oks[0], values[0], 0, steps);
            }
            1 => {
                let Some(&value) = usize::try_from(pc).ok().and_then(|at| code.get(at)) else {
                    return oracle(false, 0, 4, steps);
                };
                pc += 1;
                values.push(value);
                oks.push(true);
            }
            2 => {
                values.push(0);
                oks.push(false);
            }
            3 | 4 => {
                let Some(&slot) = usize::try_from(pc).ok().and_then(|at| code.get(at)) else {
                    return oracle(false, 0, 4, steps);
                };
                pc += 1;
                let Ok(slot) = usize::try_from(slot) else {
                    return oracle(false, 0, 6, steps);
                };
                if slot >= slot_values.len() {
                    return oracle(false, 0, 6, steps);
                }
                if opcode == 3 {
                    values.push(slot_values[slot]);
                    oks.push(slot_oks[slot]);
                } else {
                    let (Some(value), Some(ok)) = (values.pop(), oks.pop()) else {
                        return oracle(false, 0, 5, steps);
                    };
                    slot_values[slot] = value;
                    slot_oks[slot] = ok;
                }
            }
            5..=11 => {
                let (Some(right), Some(right_ok), Some(left), Some(left_ok)) =
                    (values.pop(), oks.pop(), values.pop(), oks.pop())
                else {
                    return oracle(false, 0, 5, steps);
                };
                if !left_ok || !right_ok || (matches!(opcode, 8 | 9) && right == 0) {
                    values.push(0);
                    oks.push(false);
                } else {
                    let value = match opcode {
                        5 => left.wrapping_add(right),
                        6 => left.wrapping_sub(right),
                        7 => left.wrapping_mul(right),
                        8 if left == i64::MIN && right == -1 => i64::MIN,
                        8 => left.div_euclid(right),
                        9 if left == i64::MIN && right == -1 => 0,
                        9 => left.rem_euclid(right),
                        10 => i64::from(left == right),
                        11 => i64::from(left < right),
                        _ => unreachable!(),
                    };
                    values.push(value);
                    oks.push(true);
                }
            }
            12 | 13 => {
                let Some(&target) = usize::try_from(pc).ok().and_then(|at| code.get(at)) else {
                    return oracle(false, 0, 4, steps);
                };
                pc += 1;
                if target < 0 || target as usize >= code.len() {
                    return oracle(false, 0, 7, steps);
                }
                if opcode == 12 {
                    pc = target;
                } else {
                    let (Some(condition), Some(condition_ok)) = (values.pop(), oks.pop()) else {
                        return oracle(false, 0, 5, steps);
                    };
                    if !condition_ok {
                        return oracle(false, 0, 0, steps);
                    }
                    if condition == 0 {
                        pc = target;
                    }
                }
            }
            14 => {
                let (Some(right), Some(right_ok), Some(left), Some(left_ok)) =
                    (values.pop(), oks.pop(), values.pop(), oks.pop())
                else {
                    return oracle(false, 0, 5, steps);
                };
                if left_ok {
                    values.push(left);
                    oks.push(true);
                } else {
                    values.push(right);
                    oks.push(right_ok);
                }
            }
            15 => {
                if values.pop().is_none() || oks.pop().is_none() {
                    return oracle(false, 0, 5, steps);
                }
            }
            _ => return oracle(false, 0, 3, steps),
        }
        if values.len() != oks.len() {
            return oracle(false, 0, 5, steps);
        }
    }
}

fn array_literal(values: &[i64]) -> String {
    values
        .iter()
        .map(i64::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn interpreter_driver(code: &[i64], slots: i64, max_steps: i64, expected: OracleEval) -> String {
    let code = array_literal(code);
    let ok = if expected.ok { "true" } else { "false" };
    format!(
        r#"{INTERPRETER}
let code : i64 array := [ {code} ];
let result := steel_vaak_interpret(code, {slots}, {max_steps});
if (result.ok == {ok} && result.value == {} && result.error == {} && result.steps == {})
    42
else
    0
fi"#,
        expected.value, expected.error, expected.steps
    )
}

fn checked(source: &str) -> vaak::ast::Program {
    let program = vaak::parser::parse(source).expect("self-hosting fixtureを解析できる");
    let errors: Vec<_> = vaak::check::check(&program)
        .into_iter()
        .chain(vaak::types::check_types(&program))
        .map(|error| format!("{} @{:?}", error.msg, error.span))
        .collect();
    assert!(errors.is_empty(), "静的検査: {errors:#?}");
    program
}

fn source(body: &str) -> String {
    format!("{COMPILER}\n{body}")
}

fn text(result: Result<Eval, impl std::fmt::Debug>) -> Option<Vec<u8>> {
    match result.ok()? {
        Eval::Value(Value::Str(bytes)) => Some(*bytes),
        _ => None,
    }
}

fn compile_in_both(expression: &str) -> Option<Vec<u8>> {
    let body = format!("let expression := {expression:?}; steel_subset_compile(expression)");
    let source = source(&body);
    let program = checked(&source);
    let reference = text(vaak::interp::Interp::new().run(&program));
    let bytecode = vaak::vm::compile(&program).expect("VMへ翻訳できる");
    let vm = text(vaak::vm::run_program(&bytecode));
    assert_eq!(vm, reference, "{expression}");
    reference
}

fn clang_run(name: &str, ir: &[u8]) -> Option<i32> {
    if Command::new("clang").arg("--version").output().is_err() {
        return None;
    }
    let directory =
        std::env::temp_dir().join(format!("vaak-steel-selfhost-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("一時directoryを作れる");
    let llvm = directory.join("program.ll");
    let executable = directory.join("program");
    std::fs::write(&llvm, ir).expect("LLVM IRを書ける");
    let output = Command::new("clang")
        .args(["-O2", "-o"])
        .arg(&executable)
        .arg(&llvm)
        .output()
        .expect("clangを呼べる");
    assert!(
        output.status.success(),
        "clang: {}\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(ir)
    );
    let status = Command::new(&executable)
        .status()
        .expect("生成物を実行できる");
    #[cfg(unix)]
    if status.code().is_none() {
        use std::os::unix::process::ExitStatusExt;
        eprintln!("{name}: native signal={:?}", status.signal());
    }
    std::fs::remove_dir_all(directory).expect("一時directoryを片付けられる");
    status.code()
}

fn steel_native(name: &str, source: &str) -> Option<i32> {
    let program = checked(source);
    let ir =
        vaak::steel::compile(&program).unwrap_or_else(|error| panic!("Rust STEEL: {}", error.msg));
    clang_run(name, ir.as_bytes())
}

fn direct_backends(name: &str, source: &str) -> i32 {
    let program = checked(source);
    let reference = match vaak::interp::Interp::new().run(&program) {
        Ok(Eval::Value(value)) => value.as_int().expect("subsetはi64"),
        other => panic!("{name} 参照: {other:?}"),
    };
    let vm = match vaak::vm::run_program(&vaak::vm::compile(&program).expect("VMへ翻訳できる"))
    {
        Ok(Eval::Value(value)) => value.as_int().expect("subsetはi64"),
        other => panic!("{name} VM: {other:?}"),
    };
    assert_eq!(vm, reference, "{name} 参照/VM");
    let expected = reference.rem_euclid(256) as i32;
    if let Some(status) = steel_native(&format!("direct-{name}"), source) {
        assert_eq!(status.rem_euclid(256), expected, "{name} Rust STEEL");
    }
    expected
}

#[test]
fn vaak製compilerは決定的なllvm_irを返す() {
    let ir = compile_in_both("2 + 3 * (4 - 1)").expect("正しい式をcompileできる");
    let expected = concat!(
        "; generated by Vaak steel_subset_compile\n",
        "target triple = \"x86_64-pc-linux-gnu\"\n",
        "define i32 @main() {\n",
        "entry:\n",
        "  %v0 = sub i64 4, 1\n",
        "  %v1 = mul i64 3, %v0\n",
        "  %v2 = add i64 2, %v1\n",
        "  %exit = trunc i64 %v2 to i32\n",
        "  ret i32 %exit\n",
        "}\n",
    );
    assert_eq!(ir, expected.as_bytes());
}

#[test]
fn vaak製compilerの出力をclangで実行する() {
    let cases = [
        ("precedence", "2 + 3 * (4 - 1)", 11),
        ("unary", "-(8 - 50) * 3", 126),
        ("wrapping", "9223372036854775807 + 2", 1),
    ];
    for (name, expression, expected) in cases {
        let direct = direct_backends(name, expression);
        assert_eq!(direct, expected, "{name} original Vaak source");
        let ir = compile_in_both(expression).expect("正しい式をcompileできる");
        let Some(status) = clang_run(name, &ir) else {
            return;
        };
        assert_eq!(status.rem_euclid(256), expected);
    }
}

#[test]
fn 不完全な入力は部分llvm_irを公開しない() {
    for expression in ["", "1 +", "(1 + 2", "1 / 2", "9223372036854775808"] {
        assert_eq!(compile_in_both(expression), None, "{expression:?}");
    }
}

#[test]
fn compiler自身をrust_steelでnative化して動かす() {
    let source = source(
        r#"let expression := "2 + 3 * (4 - 1)";
           let ir := steel_subset_compile(expression) ?? "";
           steel_subset_checksum(ir)"#,
    );
    let program = checked(&source);
    let expected = match vaak::interp::Interp::new().run(&program) {
        Ok(Eval::Value(value)) => value.as_int().expect("checksumはi64") as i32,
        other => panic!("参照実装: {other:?}"),
    };
    let Some(status) = steel_native("compiler-native", &source) else {
        return;
    };
    assert_eq!(status.rem_euclid(256), expected.rem_euclid(256));
}

#[test]
fn strを返す最小probeは三実装で一致する() {
    let program = checked(STRING_PROBE);
    let reference = vaak::interp::Interp::new().run(&program);
    let got = match reference {
        Ok(Eval::Value(value)) => value.as_int(),
        other => panic!("参照: {other:?}"),
    };
    assert_eq!(got, Some(42), "参照");
    let vm = vaak::vm::run_program(&vaak::vm::compile(&program).expect("VMへ翻訳できる"));
    let got = match vm {
        Ok(Eval::Value(value)) => value.as_int(),
        other => panic!("VM: {other:?}"),
    };
    assert_eq!(got, Some(42), "VM");
    if let Some(status) = steel_native("string-probe", STRING_PROBE) {
        assert_eq!(status, 42);
    }
}

#[test]
fn numeric_bytecode_interpreterは四経路で一致する() {
    let fixtures: &[(&str, &[i64], i64, i64)] = &[
        ("arithmetic", &[1, 2, 1, 3, 1, 4, 7, 5, 0], 0, 100),
        (
            "slots",
            &[1, 5, 4, 0, 3, 0, 1, 7, 5, 4, 0, 3, 0, 1, 3, 7, 0],
            1,
            100,
        ),
        (
            "branch",
            &[1, 3, 1, 5, 11, 13, 11, 1, 11, 12, 13, 1, 22, 0],
            0,
            100,
        ),
        ("coalesce", &[1, 1, 1, 0, 8, 1, 42, 14, 0], 0, 100),
        (
            "loop",
            &[
                1, 0, 4, 0, 1, 0, 4, 1, 3, 0, 1, 1000, 11, 13, 31, 3, 1, 3, 0, 5, 4, 1, 3, 0, 1, 1,
                5, 4, 0, 12, 8, 3, 1, 0,
            ],
            2,
            20_000,
        ),
    ];
    for (name, code, slots, max_steps) in fixtures {
        let expected = rust_oracle(code, *slots, *max_steps);
        let source = interpreter_driver(code, *slots, *max_steps, expected);
        let program = checked(&source);

        let reference = match vaak::interp::Interp::new().run(&program) {
            Ok(Eval::Value(value)) => value.as_int(),
            other => panic!("{name} 参照: {other:?}"),
        };
        assert_eq!(reference, Some(42), "{name} 参照");

        let bytecode = vaak::vm::compile(&program).expect("fallbackをVMへ翻訳できる");
        let vm = match vaak::vm::run_program(&bytecode) {
            Ok(Eval::Value(value)) => value.as_int(),
            other => panic!("{name} VM: {other:?}"),
        };
        assert_eq!(vm, Some(42), "{name} VM");

        if let Some(status) = steel_native(name, &source) {
            assert_eq!(status, 42, "{name} STEEL native");
        }
    }
}

#[test]
fn numeric_bytecode_interpreterは診断codeを安定させる() {
    let fixtures: &[(&str, &[i64], i64, i64, i64)] = &[
        ("bad-budget", &[0], 0, 0, 1),
        ("bad-pc", &[], 0, 10, 2),
        ("bad-opcode", &[99], 0, 10, 3),
        ("missing-operand", &[1], 0, 10, 4),
        ("stack-underflow", &[5, 0], 0, 10, 5),
        ("bad-slot", &[3, 1, 0], 1, 10, 6),
        ("bad-jump", &[12, 9, 0], 0, 10, 7),
        ("step-limit", &[12, 0], 0, 5, 8),
        ("halt-shape", &[0], 0, 10, 9),
    ];
    for (name, code, slots, max_steps, error) in fixtures {
        let expected = rust_oracle(code, *slots, *max_steps);
        assert_eq!(expected.error, *error, "{name} oracle");
        let source = interpreter_driver(code, *slots, *max_steps, expected);
        let program = checked(&source);
        for (engine, value) in [
            (
                "参照",
                vaak::interp::Interp::new()
                    .run(&program)
                    .ok()
                    .and_then(|result| match result {
                        Eval::Value(value) => value.as_int(),
                        _ => None,
                    }),
            ),
            (
                "VM",
                vaak::vm::run_program(&vaak::vm::compile(&program).expect("VMへ翻訳できる"))
                    .ok()
                    .and_then(|result| match result {
                        Eval::Value(value) => value.as_int(),
                        _ => None,
                    }),
            ),
        ] {
            assert_eq!(value, Some(42), "{name} {engine}");
        }
        if let Some(status) = steel_native(name, &source) {
            assert_eq!(status, 42, "{name} STEEL native");
        }
    }
}

#[test]
#[ignore = "現行Rust STEELのnested str returnがSIGSEGVする棄却実験"]
fn compilerを別のvaak関数から呼ぶnative_probe() {
    let source = format!("{COMPILER}\n{NESTED_COMPILER_PROBE}");
    let program = checked(&source);
    let expected = match vaak::interp::Interp::new().run(&program) {
        Ok(Eval::Value(value)) => value.as_int().expect("checksumはi64") as i32,
        other => panic!("参照: {other:?}"),
    };
    let status = steel_native("nested-compiler-probe", &source);
    assert_eq!(status, Some(expected), "現行backendではsignal 11を再現する");
}
