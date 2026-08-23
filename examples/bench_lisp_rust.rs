#![forbid(unsafe_code)]

#[path = "support/rust_lisp.rs"]
mod rust_lisp;

use std::hint::black_box;
use std::process::ExitCode;
use std::time::Instant;

#[derive(Debug)]
struct Config {
    depth: usize,
    iterations: usize,
    samples: usize,
    input: Input,
    tsv: bool,
}

#[derive(Debug)]
enum Input {
    Generated,
    Source(String),
    File(String),
}

#[derive(Clone, Debug)]
struct Measurement {
    implementation: &'static str,
    workload: &'static str,
    median_ns: f64,
    min_ns: f64,
    max_ns: f64,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), String> {
    let Some(config) = parse_arguments()? else {
        print_help();
        return Ok(());
    };

    let (source, expected, input_name) = match &config.input {
        Input::Generated => {
            if config.depth > 512 {
                return Err(
                    "--depth は再帰呼び出しの安全余裕のため 512 以下にしてください".to_owned(),
                );
            }
            let (source, expected) = rust_lisp::nested_let_source(config.depth);
            (
                source,
                Some(expected),
                format!("nested-let({})", config.depth),
            )
        }
        Input::Source(source) => (source.clone(), None, "--source".to_owned()),
        Input::File(path) => (
            std::fs::read_to_string(path).map_err(|error| format!("{path} を読めない: {error}"))?,
            None,
            path.clone(),
        ),
    };

    let naive_program =
        rust_lisp::naive::parse(&source).map_err(|error| format!("naive parser: {error}"))?;
    let tuned_program =
        rust_lisp::tuned::parse(&source).map_err(|error| format!("tuned parser: {error}"))?;
    let naive_answer = naive_program
        .eval()
        .map_err(|error| format!("naive evaluator: {error}"))?;
    let tuned_answer = tuned_program
        .eval()
        .map_err(|error| format!("tuned evaluator: {error}"))?;
    if naive_answer != tuned_answer {
        return Err(format!(
            "実装間で答えが違う: naive={naive_answer}, tuned={tuned_answer}"
        ));
    }
    if let Some(expected) = expected {
        if naive_answer != expected {
            return Err(format!(
                "生成入力の答えが違う: expected={expected}, actual={naive_answer}"
            ));
        }
    }

    let mut measurements = Vec::with_capacity(4);
    measurements.push(measure(
        "naive",
        "parse+eval",
        config.samples,
        config.iterations,
        || {
            let program = rust_lisp::naive::parse(black_box(source.as_str()))
                .expect("事前検査済みの入力を解析できる");
            black_box(program.eval().expect("事前検査済みの入力を評価できる"))
        },
    ));
    measurements.push(measure(
        "naive",
        "eval-only",
        config.samples,
        config.iterations,
        || {
            black_box(
                naive_program
                    .eval()
                    .expect("事前検査済みの構文木を評価できる"),
            )
        },
    ));
    measurements.push(measure(
        "tuned",
        "parse+eval",
        config.samples,
        config.iterations,
        || {
            let program = rust_lisp::tuned::parse(black_box(source.as_str()))
                .expect("事前検査済みの入力を解析できる");
            black_box(program.eval().expect("事前検査済みの入力を評価できる"))
        },
    ));
    let mut tuned_evaluator = tuned_program.evaluator();
    measurements.push(measure(
        "tuned",
        "eval-only",
        config.samples,
        config.iterations,
        || {
            black_box(
                tuned_evaluator
                    .eval(&tuned_program)
                    .expect("事前検査済みの構文木を評価できる"),
            )
        },
    ));

    if config.tsv {
        let input_name = input_name
            .replace('\t', " ")
            .replace('\r', " ")
            .replace('\n', " ");
        println!(
            "input\tsource_bytes\titerations\tsamples\timplementation\tworkload\tmedian_ns_per_op\tmin_ns_per_op\tmax_ns_per_op\tanswer"
        );
        for measurement in measurements {
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{:.3}\t{:.3}\t{:.3}\t{}",
                input_name,
                source.len(),
                config.iterations,
                config.samples,
                measurement.implementation,
                measurement.workload,
                measurement.median_ns,
                measurement.min_ns,
                measurement.max_ns,
                naive_answer,
            );
        }
    } else {
        println!("Safe Rust small-LISP benchmark");
        println!("input       {input_name}");
        println!("source      {} bytes", source.len());
        println!("iterations  {} / sample", config.iterations);
        println!("samples     {}", config.samples);
        println!("answer      {naive_answer}");
        println!();
        println!("implementation  workload       median ns/op     min ns/op     max ns/op");
        for measurement in measurements {
            println!(
                "{:<15} {:<12} {:>14.1} {:>13.1} {:>13.1}",
                measurement.implementation,
                measurement.workload,
                measurement.median_ns,
                measurement.min_ns,
                measurement.max_ns,
            );
        }
    }

    Ok(())
}

fn measure(
    implementation: &'static str,
    workload: &'static str,
    samples: usize,
    iterations: usize,
    mut operation: impl FnMut() -> i64,
) -> Measurement {
    let warmup_iterations = iterations.min(32).max(1);
    let mut warmup_checksum = 0_i64;
    for _ in 0..warmup_iterations {
        warmup_checksum = warmup_checksum.wrapping_add(black_box(operation()));
    }
    black_box(warmup_checksum);

    let mut timings = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        let mut checksum = 0_i64;
        for _ in 0..iterations {
            checksum = checksum.wrapping_add(black_box(operation()));
        }
        black_box(checksum);
        timings.push(start.elapsed().as_secs_f64() * 1_000_000_000.0 / iterations as f64);
    }
    timings.sort_by(f64::total_cmp);

    Measurement {
        implementation,
        workload,
        median_ns: timings[timings.len() / 2],
        min_ns: timings[0],
        max_ns: timings[timings.len() - 1],
    }
}

fn parse_arguments() -> Result<Option<Config>, String> {
    let mut depth = 48;
    let mut iterations = 2_000;
    let mut samples = 7;
    let mut input = Input::Generated;
    let mut has_custom_input = false;
    let mut tsv = false;
    let mut arguments = std::env::args().skip(1);

    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "-h" | "--help" => return Ok(None),
            "--depth" => depth = number_argument("--depth", arguments.next())?,
            "--iterations" => iterations = positive_argument("--iterations", arguments.next())?,
            "--samples" => samples = positive_argument("--samples", arguments.next())?,
            "--source" => {
                if has_custom_input {
                    return Err("--source と --file は同時に指定できません".to_owned());
                }
                input = Input::Source(value_argument("--source", arguments.next())?);
                has_custom_input = true;
            }
            "--file" => {
                if has_custom_input {
                    return Err("--source と --file は同時に指定できません".to_owned());
                }
                input = Input::File(value_argument("--file", arguments.next())?);
                has_custom_input = true;
            }
            "--tsv" => tsv = true,
            _ => {
                return Err(format!(
                    "未知の引数: {argument}\n--help で使い方を表示できます"
                ))
            }
        }
    }

    Ok(Some(Config {
        depth,
        iterations,
        samples,
        input,
        tsv,
    }))
}

fn value_argument(name: &str, value: Option<String>) -> Result<String, String> {
    value.ok_or_else(|| format!("{name} の値がありません"))
}

fn number_argument(name: &str, value: Option<String>) -> Result<usize, String> {
    let value = value_argument(name, value)?;
    value
        .parse()
        .map_err(|_| format!("{name} には非負整数を指定してください: {value}"))
}

fn positive_argument(name: &str, value: Option<String>) -> Result<usize, String> {
    let value = number_argument(name, value)?;
    if value == 0 {
        Err(format!("{name} には 1 以上を指定してください"))
    } else {
        Ok(value)
    }
}

fn print_help() {
    println!(
        "\
Safe Rust で書いた小 LISP の比較ベンチマーク

USAGE:
    cargo run --release --example bench_lisp_rust -- [OPTIONS]

OPTIONS:
    --depth N       生成する nested-let の深さ [default: 48, max: 512]
    --iterations N  一標本あたりの反復数 [default: 2000]
    --samples N     標本数。中央値を代表値にする [default: 7]
    --source EXPR   生成入力の代わりに同じ S 式を直接渡す
    --file PATH     生成入力の代わりに UTF-8 ファイルを読む
    --tsv           集計しやすい TSV で出力する
    -h, --help      この説明を表示する

`--source` / `--file` を使わない場合は、Vaak 側と同じ depth の入力を生成する。
各実装について parse+eval と、解析済み表現を使う eval-only を別々に測る。"
    );
}
#![forbid(unsafe_code)]

#[path = "support/rust_lisp.rs"]
mod rust_lisp;

use std::hint::black_box;
use std::process::ExitCode;
use std::time::Instant;

#[derive(Debug)]
struct Config {
    depth: usize,
    iterations: usize,
    samples: usize,
    input: Input,
    tsv: bool,
    only_tuned: bool,
}

#[derive(Debug)]
enum Input {
    Generated,
    Source(String),
    File(String),
}

#[derive(Clone, Debug)]
struct Measurement {
    implementation: &'static str,
    workload: &'static str,
    median_ns: f64,
    min_ns: f64,
    max_ns: f64,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), String> {
    let Some(config) = parse_arguments()? else {
        print_help();
        return Ok(());
    };

    let (source, expected, input_name) = match &config.input {
        Input::Generated => {
            if config.depth > 512 {
                return Err(
                    "--depth は再帰呼び出しの安全余裕のため 512 以下にしてください".to_owned(),
                );
            }
            let (source, expected) = rust_lisp::nested_let_source(config.depth);
            (
                source,
                Some(expected),
                format!("nested-let({})", config.depth),
            )
        }
        Input::Source(source) => (source.clone(), None, "--source".to_owned()),
        Input::File(path) => (
            std::fs::read_to_string(path).map_err(|error| format!("{path} を読めない: {error}"))?,
            None,
            path.clone(),
        ),
    };

    let naive_program =
        rust_lisp::naive::parse(&source).map_err(|error| format!("naive parser: {error}"))?;
    let tuned_program =
        rust_lisp::tuned::parse(&source).map_err(|error| format!("tuned parser: {error}"))?;
    let naive_answer = naive_program
        .eval()
        .map_err(|error| format!("naive evaluator: {error}"))?;
    let tuned_answer = tuned_program
        .eval()
        .map_err(|error| format!("tuned evaluator: {error}"))?;
    if naive_answer != tuned_answer {
        return Err(format!(
            "実装間で答えが違う: naive={naive_answer}, tuned={tuned_answer}"
        ));
    }
    if let Some(expected) = expected {
        if naive_answer != expected {
            return Err(format!(
                "生成入力の答えが違う: expected={expected}, actual={naive_answer}"
            ));
        }
    }

    let mut measurements = Vec::with_capacity(4);
    if !config.only_tuned {
        measurements.push(measure(
            "naive",
            "parse+eval",
            config.samples,
            config.iterations,
            || {
                let program = rust_lisp::naive::parse(black_box(source.as_str()))
                    .expect("事前検査済みの入力を解析できる");
                black_box(program.eval().expect("事前検査済みの入力を評価できる"))
            },
        ));
        measurements.push(measure(
            "naive",
            "eval-only",
            config.samples,
            config.iterations,
            || {
                black_box(
                    naive_program
                        .eval()
                        .expect("事前検査済みの構文木を評価できる"),
                )
            },
        ));
    }
    measurements.push(measure(
        "tuned",
        "parse+eval",
        config.samples,
        config.iterations,
        || {
            let program = rust_lisp::tuned::parse(black_box(source.as_str()))
                .expect("事前検査済みの入力を解析できる");
            black_box(program.eval().expect("事前検査済みの入力を評価できる"))
        },
    ));
    let mut tuned_evaluator = tuned_program.evaluator();
    measurements.push(measure(
        "tuned",
        "eval-only",
        config.samples,
        config.iterations,
        || {
            black_box(
                tuned_evaluator
                    .eval(&tuned_program)
                    .expect("事前検査済みの構文木を評価できる"),
            )
        },
    ));

    if config.tsv {
        let input_name = input_name
            .replace('\t', " ")
            .replace('\r', " ")
            .replace('\n', " ");
        println!(
            "input\tsource_bytes\titerations\tsamples\timplementation\tworkload\tmedian_ns_per_op\tmin_ns_per_op\tmax_ns_per_op\tanswer"
        );
        for measurement in measurements {
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{:.3}\t{:.3}\t{:.3}\t{}",
                input_name,
                source.len(),
                config.iterations,
                config.samples,
                measurement.implementation,
                measurement.workload,
                measurement.median_ns,
                measurement.min_ns,
                measurement.max_ns,
                naive_answer,
            );
        }
    } else {
        println!("Safe Rust small-LISP benchmark");
        println!("input       {input_name}");
        println!("source      {} bytes", source.len());
        println!("iterations  {} / sample", config.iterations);
        println!("samples     {}", config.samples);
        println!("answer      {naive_answer}");
        println!();
        println!("implementation  workload       median ns/op     min ns/op     max ns/op");
        for measurement in measurements {
            println!(
                "{:<15} {:<12} {:>14.1} {:>13.1} {:>13.1}",
                measurement.implementation,
                measurement.workload,
                measurement.median_ns,
                measurement.min_ns,
                measurement.max_ns,
            );
        }
    }

    Ok(())
}

fn measure(
    implementation: &'static str,
    workload: &'static str,
    samples: usize,
    iterations: usize,
    mut operation: impl FnMut() -> i64,
) -> Measurement {
    let warmup_iterations = iterations.min(32).max(1);
    let mut warmup_checksum = 0_i64;
    for _ in 0..warmup_iterations {
        warmup_checksum = warmup_checksum.wrapping_add(black_box(operation()));
    }
    black_box(warmup_checksum);

    let mut timings = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        let mut checksum = 0_i64;
        for _ in 0..iterations {
            checksum = checksum.wrapping_add(black_box(operation()));
        }
        black_box(checksum);
        timings.push(start.elapsed().as_secs_f64() * 1_000_000_000.0 / iterations as f64);
    }
    timings.sort_by(f64::total_cmp);

    Measurement {
        implementation,
        workload,
        median_ns: timings[timings.len() / 2],
        min_ns: timings[0],
        max_ns: timings[timings.len() - 1],
    }
}

fn parse_arguments() -> Result<Option<Config>, String> {
    let mut depth = 48;
    let mut iterations = 2_000;
    let mut samples = 7;
    let mut input = Input::Generated;
    let mut has_custom_input = false;
    let mut tsv = false;
    let mut only_tuned = false;
    let mut arguments = std::env::args().skip(1);

    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "-h" | "--help" => return Ok(None),
            "--depth" => depth = number_argument("--depth", arguments.next())?,
            "--iterations" => iterations = positive_argument("--iterations", arguments.next())?,
            "--samples" => samples = positive_argument("--samples", arguments.next())?,
            "--source" => {
                if has_custom_input {
                    return Err("--source と --file は同時に指定できません".to_owned());
                }
                input = Input::Source(value_argument("--source", arguments.next())?);
                has_custom_input = true;
            }
            "--file" => {
                if has_custom_input {
                    return Err("--source と --file は同時に指定できません".to_owned());
                }
                input = Input::File(value_argument("--file", arguments.next())?);
                has_custom_input = true;
            }
            "--tsv" => tsv = true,
            "--only-tuned" => only_tuned = true,
            _ => {
                return Err(format!(
                    "未知の引数: {argument}\n--help で使い方を表示できます"
                ))
            }
        }
    }

    Ok(Some(Config {
        depth,
        iterations,
        samples,
        input,
        tsv,
        only_tuned,
    }))
}

fn value_argument(name: &str, value: Option<String>) -> Result<String, String> {
    value.ok_or_else(|| format!("{name} の値がありません"))
}

fn number_argument(name: &str, value: Option<String>) -> Result<usize, String> {
    let value = value_argument(name, value)?;
    value
        .parse()
        .map_err(|_| format!("{name} には非負整数を指定してください: {value}"))
}

fn positive_argument(name: &str, value: Option<String>) -> Result<usize, String> {
    let value = number_argument(name, value)?;
    if value == 0 {
        Err(format!("{name} には 1 以上を指定してください"))
    } else {
        Ok(value)
    }
}

fn print_help() {
    println!(
        "\
Safe Rust で書いた小 LISP の比較ベンチマーク

USAGE:
    cargo run --release --example bench_lisp_rust -- [OPTIONS]

OPTIONS:
    --depth N       生成する nested-let の深さ [default: 48, max: 512]
    --iterations N  一標本あたりの反復数 [default: 2000]
    --samples N     標本数。中央値を代表値にする [default: 7]
    --source EXPR   生成入力の代わりに同じ S 式を直接渡す
    --file PATH     生成入力の代わりに UTF-8 ファイルを読む
    --tsv           集計しやすい TSV で出力する
    --only-tuned    naive を走らせず、調整版だけを測る
    -h, --help      この説明を表示する

`--source` / `--file` を使わない場合は、Vaak 側と同じ depth の入力を生成する。
各実装について parse+eval と、解析済み表現を使う eval-only を別々に測る。"
    );
}
