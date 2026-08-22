use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::time::{Duration, Instant};

const SOURCE: &str = include_str!("../../tape_language.vaak");
const PROGRAM_LINE: &str = "let brainfuck := \",>,<[->+<]>.\";";
const INPUT_LINE: &str = "let input := \" !\";";
const ROUNDS_LINE: &str = "let benchmark_rounds := 1;";
const PREPARED_LINE: &str = "let benchmark_prepared := false;";

fn quote_vaak(text: &str) -> String {
    text.chars()
        .flat_map(|c| match c {
            '\\' => "\\\\".chars().collect::<Vec<_>>(),
            '"' => "\\\"".chars().collect(),
            '\n' => "\\n".chars().collect(),
            '\r' => "\\r".chars().collect(),
            '\t' => "\\t".chars().collect(),
            other => vec![other],
        })
        .collect()
}

fn source_with(program: &str, input: &str, rounds: usize, prepared: bool) -> String {
    assert!(SOURCE.contains(PROGRAM_LINE), "命令列の目印がある");
    assert!(SOURCE.contains(INPUT_LINE), "入力列の目印がある");
    assert!(SOURCE.contains(ROUNDS_LINE), "反復回数の目印がある");
    assert!(SOURCE.contains(PREPARED_LINE), "前処理モードの目印がある");
    SOURCE
        .replace(
            PROGRAM_LINE,
            &format!("let brainfuck := \"{}\";", quote_vaak(program)),
        )
        .replace(
            INPUT_LINE,
            &format!("let input := \"{}\";", quote_vaak(input)),
        )
        .replace(ROUNDS_LINE, &format!("let benchmark_rounds := {rounds};"))
        .replace(
            PREPARED_LINE,
            &format!("let benchmark_prepared := {prepared};"),
        )
}

fn parse(source: &str) -> vaak::ast::Program {
    let program = vaak::parser::parse(source).expect("テープ言語を構文解析できる");
    let errors: Vec<_> = vaak::check::check(&program)
        .into_iter()
        .chain(vaak::types::check_types(&program))
        .map(|error| error.msg)
        .collect();
    assert!(errors.is_empty(), "テープ言語の静的検査: {errors:#?}");
    program
}

fn answer(eval: vaak::interp::Eval) -> i64 {
    match eval {
        vaak::interp::Eval::Value(value) => {
            i64::try_from(value.as_int().expect("答えは整数")).expect("答えは i64 に収まる")
        }
        other => panic!("値で終わる: {other:?}"),
    }
}

fn steel(program: &vaak::ast::Program) -> String {
    vaak::steel::compile(program)
        .unwrap_or_else(|error| panic!("STEEL の LLVM IR へ翻訳できない: {}", error.msg))
}

fn reference(program: &vaak::ast::Program) -> i64 {
    answer(
        vaak::interp::Interp::new()
            .run(program)
            .expect("参照実装で走る"),
    )
}

fn bytecode(program: &vaak::ast::Program) -> vaak::vm::Program2 {
    vaak::vm::compile(program).expect("VM へ翻訳できる")
}

fn vm(program: &vaak::vm::Program2) -> i64 {
    answer(vaak::vm::run_program(program).expect("VM で走る"))
}

fn clang_path() -> Option<PathBuf> {
    if Command::new("clang").arg("--version").output().is_ok() {
        return Some(PathBuf::from("clang"));
    }
    if cfg!(windows) {
        let candidate = std::env::var_os("ProgramFiles")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"))
            .join("LLVM")
            .join("bin")
            .join("clang.exe");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn executable_path(directory: &Path, name: &str) -> PathBuf {
    directory.join(if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    })
}

fn compile_native(ir: &str, directory: &Path, name: &str, clang: &Path) -> PathBuf {
    std::fs::create_dir_all(directory).expect("一時ディレクトリを作れる");
    let llvm = directory.join(format!("{name}.ll"));
    let executable = executable_path(directory, name);
    std::fs::write(&llvm, ir).expect("LLVM IR を書ける");
    let output = Command::new(clang)
        .arg("-O3")
        .arg("-o")
        .arg(&executable)
        .arg(&llvm)
        .output()
        .expect("clang を呼べる");
    assert!(
        output.status.success(),
        "clang: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    executable
}

fn native_status(executable: &Path) -> ExitStatus {
    Command::new(executable)
        .status()
        .expect("STEEL native を実行できる")
}

fn median(mut values: Vec<Duration>) -> Duration {
    values.sort_unstable();
    values[values.len() / 2]
}

fn measure<F>(samples: usize, mut operation: F) -> Duration
where
    F: FnMut(),
{
    operation();
    let mut elapsed = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        operation();
        elapsed.push(start.elapsed());
    }
    median(elapsed)
}

fn per_round(duration: Duration, rounds: usize) -> f64 {
    duration.as_secs_f64() * 1_000_000_000.0 / rounds as f64
}

fn main() {
    let rounds = std::env::args()
        .nth(1)
        .map(|arg| arg.parse::<usize>().expect("反復回数は正の整数"))
        .unwrap_or(100);
    let samples = std::env::args()
        .nth(2)
        .map(|arg| arg.parse::<usize>().expect("標本数は正の整数"))
        .unwrap_or(7);
    let native_rounds = std::env::args()
        .nth(3)
        .map(|arg| arg.parse::<usize>().expect("native 反復回数は正の整数"))
        .unwrap_or(50_000);
    let prepared = std::env::args().nth(4).as_deref() == Some("prepared");
    assert!(rounds > 0, "反復回数は正である");
    assert!(samples > 0, "標本数は正である");
    assert!(native_rounds > 0, "native 反復回数は正である");

    let source = source_with(",>,<[->+<]>.", " !", rounds, prepared);
    let program = parse(&source);
    let compiled = bytecode(&program);
    assert_eq!(reference(&program), 42, "参照実装の自己検査");
    assert_eq!(vm(&compiled), 42, "VM の自己検査");

    let reference_time = measure(samples, || assert_eq!(reference(&program), 42));
    let vm_time = measure(samples, || assert_eq!(vm(&compiled), 42));

    println!("mode: {}", if prepared { "prepared" } else { "source" });
    println!("managed: {rounds} runs/sample, {samples} samples, median");
    println!(
        "reference: {:>12.3} ns/run ({:?}/sample)",
        per_round(reference_time, rounds),
        reference_time
    );
    println!(
        "VM:        {:>12.3} ns/run ({:?}/sample)",
        per_round(vm_time, rounds),
        vm_time
    );

    let Some(clang) = clang_path() else {
        println!("STEEL: clang が無いので LLVM IR 生成だけを確認");
        steel(&program);
        return;
    };

    let native_program = parse(&source_with(",>,<[->+<]>.", " !", native_rounds, prepared));
    let directory = std::env::temp_dir().join(format!(
        "vaak-tape-language-{}-{}",
        std::process::id(),
        native_rounds
    ));
    let ir = steel(&native_program);
    let executable = compile_native(&ir, &directory, "tape", &clang);
    let baseline_program = parse("42");
    let baseline_ir = steel(&baseline_program);
    let baseline = compile_native(&baseline_ir, &directory, "baseline", &clang);

    assert_eq!(native_status(&executable).code(), Some(42));
    assert_eq!(native_status(&baseline).code(), Some(42));
    let native_time = measure(samples, || {
        assert_eq!(native_status(&executable).code(), Some(42))
    });
    let launch_time = measure(samples, || {
        assert_eq!(native_status(&baseline).code(), Some(42))
    });
    let net = native_time.saturating_sub(launch_time);

    println!(
        "STEEL raw: {:>12.3} ns/run ({:?}/sample, process launch included)",
        per_round(native_time, native_rounds),
        native_time
    );
    println!("native:  {native_rounds} runs/sample");
    println!("launch:                           {:?}/sample", launch_time);
    println!(
        "STEEL net: {:>12.3} ns/run ({:?}/sample, raw - launch)",
        per_round(net, native_rounds),
        net
    );

    std::fs::remove_dir_all(directory).expect("測定の一時ディレクトリを片付けられる");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn テープ言語が参照実装とvmで同じ答えになる() {
        for prepared in [false, true] {
            let program = parse(&source_with(",>,<[->+<]>.", " !", 1, prepared));
            let compiled = bytecode(&program);
            assert_eq!(reference(&program), 42);
            assert_eq!(vm(&compiled), 42);
        }
    }

    #[test]
    fn 不正な角括弧と左端越えを拒む() {
        for source in [
            source_with("]", "", 1, false),
            source_with("<", "", 1, false),
        ] {
            let program = parse(&source);
            let compiled = bytecode(&program);
            assert_eq!(reference(&program), 1);
            assert_eq!(vm(&compiled), 1);
        }
    }

    #[test]
    fn テープ言語をsteel_nativeまで通せる() {
        let program = parse(&source_with(",>,<[->+<]>.", " !", 1, false));
        let ir = steel(&program);
        assert!(ir.contains("define i32 @main()"));
        let Some(clang) = clang_path() else {
            return;
        };
        let directory =
            std::env::temp_dir().join(format!("vaak-tape-language-test-{}", std::process::id()));
        let executable = compile_native(&ir, &directory, "tape-test", &clang);
        assert_eq!(native_status(&executable).code(), Some(42));
        std::fs::remove_dir_all(directory).expect("試験の一時ディレクトリを片付けられる");
    }
}
