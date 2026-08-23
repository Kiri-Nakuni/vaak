//! STEEL が生成した小 LISP を native executable として測る。

#![forbid(unsafe_code)]

#[allow(dead_code)]
#[path = "support/rust_lisp.rs"]
mod rust_lisp;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const EXAMPLE: &str = "examples/vaak/06-LISP.vaak";
const MAIN_MARKER: &str = "let main_program :=";

#[derive(Clone, Copy)]
enum Workload {
    TokenizeAndStreamEval,
    PretokenizedStreamEval,
}

impl Workload {
    fn name(self) -> &'static str {
        match self {
            Workload::TokenizeAndStreamEval => "tokenize+stream-eval",
            Workload::PretokenizedStreamEval => "pretokenized-stream-eval",
        }
    }
}

fn nested_let(depth: usize) -> String {
    assert!(depth > 0, "深さは 1 以上");
    let mut source = String::new();
    for i in 0..depth {
        source.push_str(&format!("(let x{i} {i} "));
    }
    source.push_str(&format!("x{}", depth - 1));
    source.extend(std::iter::repeat(')').take(depth));
    source
}

fn benchmark_source(
    core: &str,
    expression: &str,
    expected: i32,
    iterations: usize,
    workload: Workload,
) -> String {
    let (setup, operation) = match workload {
        Workload::TokenizeAndStreamEval => (
            "",
            "    let result := run_lisp(benchmark_source, false) ?? 0 - 1;\n\
             benchmark_answer := result;\n\
             benchmark_checksum += result;",
        ),
        Workload::PretokenizedStreamEval => (
            "let benchmark_tokens := tokenize(benchmark_source);",
            "    let result := run_tokens(benchmark_tokens, false) ?? 0 - 1;\n\
             benchmark_answer := result;\n\
             benchmark_checksum += result;",
        ),
    };
    format!(
        r#"{core}
let benchmark_source := {expression:?};
{setup}
var benchmark_answer := 0;
var benchmark_checksum := 0;
nfor (benchmark_i, 0, {iterations}) {{
{operation}
}};
if (benchmark_answer == {expected} &&
    benchmark_checksum == {expected} * {iterations}) {expected} else 0 fi
"#
    )
}

fn checked_program(source: &str) -> vaak::ast::Program {
    let program = vaak::parser::parse(source).expect("STEEL ベンチを解析できる");
    let errors: Vec<_> = vaak::check::check(&program)
        .into_iter()
        .chain(vaak::types::check_types(&program))
        .map(|error| error.msg)
        .collect();
    assert!(errors.is_empty(), "STEEL ベンチの静的誤り: {errors:#?}");
    program
}

fn option(name: &str, default: usize) -> usize {
    let mut arguments = std::env::args();
    while let Some(argument) = arguments.next() {
        if argument == name {
            return arguments
                .next()
                .unwrap_or_else(|| panic!("{name} の値が要る"))
                .parse()
                .unwrap_or_else(|_| panic!("{name} は正の整数"));
        }
    }
    default
}

fn string_option(name: &str) -> Option<String> {
    let mut arguments = std::env::args();
    while let Some(argument) = arguments.next() {
        if argument == name {
            return Some(
                arguments
                    .next()
                    .unwrap_or_else(|| panic!("{name} の値が要る")),
            );
        }
    }
    None
}

fn clang() -> PathBuf {
    if let Some(path) = string_option("--clang") {
        return path.into();
    }
    let installed = PathBuf::from(r"C:\Program Files\LLVM\bin\clang.exe");
    if installed.is_file() {
        installed
    } else {
        "clang".into()
    }
}

fn compile(clang: &Path, ir: &str, directory: &Path, name: &str) -> PathBuf {
    let llvm = directory.join(format!("{name}.ll"));
    let executable = directory.join(format!("{name}.exe"));
    std::fs::write(&llvm, ir).expect("LLVM IR を書ける");
    let output = Command::new(clang)
        .arg("-O3")
        .arg("-o")
        .arg(&executable)
        .arg(&llvm)
        .output()
        .unwrap_or_else(|error| panic!("{} を起動できない: {error}", clang.display()));
    assert!(
        output.status.success(),
        "clang: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    executable
}

fn steel_sample(executable: &Path, expected: i32, iterations: usize) -> f64 {
    let started = Instant::now();
    let status = Command::new(executable)
        .status()
        .expect("STEEL native executable を走らせられる");
    let elapsed = started.elapsed();
    assert_eq!(status.code(), Some(expected));
    elapsed.as_secs_f64() * 1_000_000_000.0 / iterations as f64
}

fn rust_sample(
    workload: Workload,
    expression: &str,
    program: &rust_lisp::tuned::Program<'_>,
    evaluator: &mut rust_lisp::tuned::Evaluator,
    expected: i64,
    iterations: usize,
) -> f64 {
    let started = Instant::now();
    let mut checksum = 0_i64;
    for _ in 0..iterations {
        let answer = match workload {
            Workload::TokenizeAndStreamEval => {
                rust_lisp::tuned::parse(std::hint::black_box(expression))
                    .expect("検査済みのS式を解析できる")
                    .eval()
                    .expect("検査済みのS式を評価できる")
            }
            Workload::PretokenizedStreamEval => {
                evaluator.eval(program).expect("解析済みのS式を評価できる")
            }
        };
        checksum = checksum.wrapping_add(std::hint::black_box(answer));
    }
    let elapsed = started.elapsed();
    assert_eq!(
        checksum,
        expected.wrapping_mul(i64::try_from(iterations).expect("反復数が i64 に収まる"))
    );
    std::hint::black_box(checksum);
    elapsed.as_secs_f64() * 1_000_000_000.0 / iterations as f64
}

fn main() {
    let depth = option("--depth", 48);
    let iterations = option("--iterations", 10_000);
    let samples = option("--samples", 9);
    assert!((1..=256).contains(&depth));
    assert!(iterations > 0 && samples > 0);

    let expression = nested_let(depth);
    let expected = i32::try_from(depth - 1).expect("答えが i32 に収まる");
    let example = std::fs::read_to_string(EXAMPLE).expect("LISP 例を読める");
    let core = example
        .split_once(MAIN_MARKER)
        .map(|(core, _)| core)
        .expect("LISP 例の本体と確認式を分けられる");
    let directory = std::env::temp_dir().join(format!("vaak-steel-lisp-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("STEEL ベンチ用ディレクトリを作れる");
    let clang = clang();
    let rust_program = rust_lisp::tuned::parse(&expression).expect("Rust tuned で解析できる");
    assert_eq!(rust_program.eval(), Ok(i64::from(expected)));

    println!(
        "engine\tworkload\tenvironment\tdepth\tsource_bytes\titerations\tsamples\tmedian_ns_per_eval\tmin_ns_per_eval\tmax_ns_per_eval\tratio_of_medians\tpaired_median_ratio\tanswer"
    );
    for (index, workload) in [
        Workload::TokenizeAndStreamEval,
        Workload::PretokenizedStreamEval,
    ]
    .into_iter()
    .enumerate()
    {
        let source = benchmark_source(core, &expression, expected, iterations, workload);
        let program = checked_program(&source);
        let ir = vaak::steel::compile(&program)
            .unwrap_or_else(|error| panic!("STEEL IR: {}", error.msg));
        let executable = compile(&clang, &ir, &directory, &format!("lisp-{index}"));
        let mut evaluator = rust_program.evaluator();
        let warmup_iterations = iterations.min(1_000);
        let warmup_started = Instant::now();
        while warmup_started.elapsed() < Duration::from_millis(200) {
            std::hint::black_box(rust_sample(
                workload,
                &expression,
                &rust_program,
                &mut evaluator,
                i64::from(expected),
                warmup_iterations,
            ));
        }
        std::hint::black_box(steel_sample(&executable, expected, iterations));

        let mut steel_timings = Vec::with_capacity(samples);
        let mut rust_timings = Vec::with_capacity(samples);
        for sample in 0..samples {
            let measure_steel = || steel_sample(&executable, expected, iterations);
            let mut measure_rust = || {
                rust_sample(
                    workload,
                    &expression,
                    &rust_program,
                    &mut evaluator,
                    i64::from(expected),
                    iterations,
                )
            };
            if sample % 2 == 0 {
                steel_timings.push(measure_steel());
                rust_timings.push(measure_rust());
            } else {
                rust_timings.push(measure_rust());
                steel_timings.push(measure_steel());
            }
        }
        let mut paired_ratios: Vec<_> = steel_timings
            .iter()
            .zip(&rust_timings)
            .map(|(steel, rust)| steel / rust)
            .collect();
        paired_ratios.sort_by(f64::total_cmp);
        let paired_ratio = paired_ratios[paired_ratios.len() / 2];
        steel_timings.sort_by(f64::total_cmp);
        rust_timings.sort_by(f64::total_cmp);
        let steel_median = steel_timings[steel_timings.len() / 2];
        let rust_median = rust_timings[rust_timings.len() / 2];
        let ratio = steel_median / rust_median;
        println!(
            "steel-native\t{}\talias-push-pop\t{depth}\t{}\t{iterations}\t{samples}\t{steel_median:.3}\t{:.3}\t{:.3}\t{ratio:.3}\t{paired_ratio:.3}\t{expected}",
            workload.name(),
            expression.len(),
            steel_timings[0],
            steel_timings[steel_timings.len() - 1]
        );
        let rust_workload = match workload {
            Workload::TokenizeAndStreamEval => "parse+eval",
            Workload::PretokenizedStreamEval => "eval-only",
        };
        println!(
            "rust-native\t{rust_workload}\tintern-arena-push-pop\t{depth}\t{}\t{iterations}\t{samples}\t{rust_median:.3}\t{:.3}\t{:.3}\t1.000\t1.000\t{expected}",
            expression.len(),
            rust_timings[0],
            rust_timings[rust_timings.len() - 1]
        );
    }
    std::fs::remove_dir_all(directory).expect("STEEL ベンチ用ディレクトリを片付けられる");
}
