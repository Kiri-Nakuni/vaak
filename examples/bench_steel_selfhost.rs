//! Rust oracleと、Rust STEELでnative化したpure Vaak interpreterの同一bytecode比較。
//!
//! source parse、Vaak check/type-check、Rust STEEL compile、clangは準備時間で測定外。
//! 両者へ同じnumeric bytecodeとloop上限を渡し、bytecode dispatchだけを測る。

use std::process::Command;
use std::time::{Duration, Instant};

const INTERPRETER: &str =
    include_str!("../examples/experiments/steel_vaak_bytecode_interpreter.vaak");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Eval {
    ok: bool,
    value: i64,
    error: i64,
    steps: i64,
}

fn result(ok: bool, value: i64, error: i64, steps: i64) -> Eval {
    Eval {
        ok,
        value,
        error,
        steps,
    }
}

fn rust_interpret(code: &[i64], slot_count: usize, max_steps: i64) -> Eval {
    let mut values = Vec::<i64>::new();
    let mut oks = Vec::<bool>::new();
    let mut slots = vec![0_i64; slot_count];
    let mut slot_oks = vec![false; slot_count];
    let mut pc = 0_usize;
    let mut steps = 0_i64;
    loop {
        if steps >= max_steps {
            return result(false, 0, 8, steps);
        }
        let Some(&opcode) = code.get(pc) else {
            return result(false, 0, 2, steps);
        };
        pc += 1;
        steps += 1;
        match opcode {
            0 => {
                if values.len() != 1 || oks.len() != 1 {
                    return result(false, 0, 9, steps);
                }
                return result(oks[0], values[0], 0, steps);
            }
            1 => {
                let Some(&value) = code.get(pc) else {
                    return result(false, 0, 4, steps);
                };
                pc += 1;
                values.push(value);
                oks.push(true);
            }
            3 | 4 => {
                let Some(&slot) = code.get(pc) else {
                    return result(false, 0, 4, steps);
                };
                pc += 1;
                let Ok(slot) = usize::try_from(slot) else {
                    return result(false, 0, 6, steps);
                };
                if slot >= slots.len() {
                    return result(false, 0, 6, steps);
                }
                if opcode == 3 {
                    values.push(slots[slot]);
                    oks.push(slot_oks[slot]);
                } else {
                    let (Some(value), Some(ok)) = (values.pop(), oks.pop()) else {
                        return result(false, 0, 5, steps);
                    };
                    slots[slot] = value;
                    slot_oks[slot] = ok;
                }
            }
            5 | 11 => {
                let (Some(right), Some(right_ok), Some(left), Some(left_ok)) =
                    (values.pop(), oks.pop(), values.pop(), oks.pop())
                else {
                    return result(false, 0, 5, steps);
                };
                values.push(if opcode == 5 {
                    left.wrapping_add(right)
                } else {
                    i64::from(left < right)
                });
                oks.push(left_ok && right_ok);
            }
            12 | 13 => {
                let Some(&target) = code.get(pc) else {
                    return result(false, 0, 4, steps);
                };
                pc += 1;
                let Ok(target) = usize::try_from(target) else {
                    return result(false, 0, 7, steps);
                };
                if target >= code.len() {
                    return result(false, 0, 7, steps);
                }
                if opcode == 12 {
                    pc = target;
                } else {
                    let (Some(condition), Some(ok)) = (values.pop(), oks.pop()) else {
                        return result(false, 0, 5, steps);
                    };
                    if !ok {
                        return result(false, 0, 0, steps);
                    }
                    if condition == 0 {
                        pc = target;
                    }
                }
            }
            _ => return result(false, 0, 3, steps),
        }
    }
}

fn sum_loop_code(limit: i64) -> Vec<i64> {
    vec![
        1, 0, 4, 0, 1, 0, 4, 1, 3, 0, 1, limit, 11, 13, 31, 3, 1, 3, 0, 5, 4, 1, 3, 0, 1, 1, 5, 4,
        0, 12, 8, 3, 1, 0,
    ]
}

fn array_literal(values: &[i64]) -> String {
    values
        .iter()
        .map(i64::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn checked(source: &str) -> vaak::ast::Program {
    let program = vaak::parser::parse(source).expect("benchmark sourceをparseできる");
    let errors: Vec<_> = vaak::check::check(&program)
        .into_iter()
        .chain(vaak::types::check_types(&program))
        .map(|error| error.msg)
        .collect();
    assert!(errors.is_empty(), "静的検査: {errors:#?}");
    program
}

fn build_native(source: &str, label: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let ir = vaak::steel::compile(&checked(source))
        .unwrap_or_else(|error| panic!("Rust STEEL: {}", error.msg));
    let directory = std::env::temp_dir().join(format!(
        "vaak-steel-interpreter-bench-{}-{label}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).expect("一時directoryを作れる");
    let llvm = directory.join("benchmark.ll");
    let executable = directory.join("benchmark");
    std::fs::write(&llvm, ir).expect("LLVM IRを書ける");
    let output = Command::new("clang")
        .args(["-O2", "-o"])
        .arg(&executable)
        .arg(&llvm)
        .output()
        .expect("clangを呼べる");
    assert!(
        output.status.success(),
        "clang: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    (directory, executable)
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn native_sample(executable: &std::path::Path, expected: i32) -> Duration {
    let start = Instant::now();
    let status = Command::new(executable)
        .status()
        .expect("native interpreterを実行できる");
    let elapsed = start.elapsed();
    assert_eq!(status.code(), Some(expected));
    elapsed
}

fn main() {
    if Command::new("clang").arg("--version").output().is_err() {
        eprintln!("clangが無いため比較できない");
        std::process::exit(2);
    }
    let limit = std::env::args()
        .nth(1)
        .map(|arg| arg.parse::<i64>().expect("loop上限は正のi64"))
        .unwrap_or(200_000);
    assert!(limit > 0);
    let max_steps = limit
        .checked_mul(14)
        .and_then(|value| value.checked_add(100))
        .unwrap();
    let code = sum_loop_code(limit);
    let oracle = rust_interpret(&code, 2, max_steps);
    assert!(oracle.ok && oracle.error == 0);
    let expected = oracle.value.rem_euclid(251) as i32;

    let source = format!(
        r#"{INTERPRETER}
let code := [ {} ];
let result := steel_vaak_interpret(code, 2, {max_steps});
if (result.ok && result.error == 0) result.value mod 251 else 250 fi"#,
        array_literal(&code)
    );
    let (native_directory, native) = build_native(&source, "work");
    let (empty_directory, empty) = build_native("0", "empty");
    let _ = native_sample(&native, expected);
    let _ = native_sample(&empty, 0);

    let native_median = median((0..9).map(|_| native_sample(&native, expected)).collect());
    let startup_median = median((0..9).map(|_| native_sample(&empty, 0)).collect());
    let adjusted = native_median.saturating_sub(startup_median);
    let rust_median = median(
        (0..9)
            .map(|_| {
                let start = Instant::now();
                let result = rust_interpret(&code, 2, max_steps);
                assert_eq!(result, oracle);
                start.elapsed()
            })
            .collect(),
    );

    println!("loop_limit={limit}");
    println!("executed_opcodes={}", oracle.steps);
    println!("rust_oracle_ns={}", rust_median.as_nanos());
    println!("vaak_steel_native_raw_ns={}", native_median.as_nanos());
    println!("native_process_startup_ns={}", startup_median.as_nanos());
    println!("vaak_steel_native_adjusted_ns={}", adjusted.as_nanos());
    println!(
        "adjusted_ratio_vs_rust={:.2}",
        adjusted.as_nanos() as f64 / rust_median.as_nanos() as f64
    );
    println!("scope=same numeric bytecode dispatch; source parse/check/STEEL/clang excluded");

    std::fs::remove_dir_all(native_directory).expect("native一時directoryを片付けられる");
    std::fs::remove_dir_all(empty_directory).expect("empty一時directoryを片付けられる");
}
