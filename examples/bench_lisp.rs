//! LISP 例の字句環境を、深い複製と alias の push/pop で比較する。

use std::time::{Duration, Instant};

const EXAMPLE: &str = "examples/vaak/06-LISP.vaak";
const MAIN_MARKER: &str = "let main_program :=";

#[derive(Clone, Copy)]
enum Engine {
    Reference,
    Vm,
}

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
    depth: usize,
    iterations: usize,
    copy: bool,
    workload: Workload,
) -> String {
    let expression = nested_let(depth);
    let copy = if copy { "true" } else { "false" };
    let (setup, operation) = match workload {
        Workload::TokenizeAndStreamEval => (
            "",
            format!(
                "    let benchmark_result := run_lisp(benchmark_source, {copy}) ?? 0 - 1;\n\
                 benchmark_answer := benchmark_result;"
            ),
        ),
        Workload::PretokenizedStreamEval => (
            "let benchmark_tokens := tokenize(benchmark_source);",
            format!(
                "    let benchmark_result := run_tokens(benchmark_tokens, {copy}) ?? 0 - 1;\n\
                 benchmark_answer := benchmark_result;"
            ),
        ),
    };
    format!(
        r#"{core}
let benchmark_source := {expression:?};
{setup}
var benchmark_answer := 0;
nfor (benchmark_i, 0, {iterations}) {{
{operation}
}};
benchmark_answer
"#
    )
}

fn checked_program(source: &str) -> vaak::ast::Program {
    let program = vaak::parser::parse(source).expect("ベンチを解析できる");
    let errors: Vec<_> = vaak::check::check(&program)
        .into_iter()
        .chain(vaak::types::check_types(&program))
        .map(|error| error.msg)
        .collect();
    assert!(errors.is_empty(), "ベンチの静的誤り: {errors:#?}");
    program
}

fn answer(eval: vaak::interp::Eval) -> i128 {
    match eval {
        vaak::interp::Eval::Value(value) => value.as_int().expect("答えは整数"),
        other => panic!("値で終わらなかった: {other:?}"),
    }
}

fn samples(engine: Engine, program: &vaak::ast::Program, count: usize) -> Vec<Duration> {
    match engine {
        Engine::Reference => (0..count)
            .map(|_| {
                let start = Instant::now();
                let result = vaak::interp::Interp::new()
                    .run(program)
                    .expect("参照実装で走る");
                let elapsed = start.elapsed();
                std::hint::black_box(answer(result));
                elapsed
            })
            .collect(),
        Engine::Vm => {
            let bytecode = vaak::vm::compile(program).expect("VM へ翻訳できる");
            let mut runner = vaak::vm::Runner::new();
            (0..count)
                .map(|_| {
                    let start = Instant::now();
                    let (result, _) = runner.run(&bytecode, Vec::new()).expect("VM で走る");
                    let elapsed = start.elapsed();
                    std::hint::black_box(answer(result));
                    elapsed
                })
                .collect()
        }
    }
}

fn median(mut values: Vec<Duration>) -> Duration {
    values.sort_unstable();
    values[values.len() / 2]
}

fn option(name: &str, default: usize) -> usize {
    let mut args = std::env::args();
    while let Some(arg) = args.next() {
        if arg == name {
            return args
                .next()
                .unwrap_or_else(|| panic!("{name} の値が要る"))
                .parse()
                .unwrap_or_else(|_| panic!("{name} は正の整数"));
        }
    }
    default
}

fn flag(name: &str) -> bool {
    std::env::args().any(|argument| argument == name)
}

fn run() {
    let depth = option("--depth", 48);
    let iterations = option("--iterations", 40);
    let sample_count = option("--samples", 5);
    let tsv = flag("--tsv");
    assert!(depth > 0 && iterations > 0 && sample_count > 0);

    let example = std::fs::read_to_string(EXAMPLE).expect("LISP 例を読める");
    let core = example
        .split_once(MAIN_MARKER)
        .map(|(core, _)| core)
        .expect("LISP 例の本体と確認式を分けられる");
    let expected = i128::try_from(depth - 1).expect("深さが i128 に収まる");

    let source_bytes = nested_let(depth).len();
    if tsv {
        println!(
            "engine\tworkload\tenvironment\tdepth\tsource_bytes\titerations\tsamples\tmedian_ns_per_eval\tanswer"
        );
    } else {
        println!(
            "depth={depth}, source_bytes={source_bytes}, evaluations/sample={iterations}, samples={sample_count}"
        );
        println!("engine\tworkload\tenvironment\tmedian\tns/eval");
    }
    for (engine_name, engine) in [("reference", Engine::Reference), ("vm", Engine::Vm)] {
        for (environment, copy) in [("deep-copy", true), ("alias-push-pop", false)] {
            for workload in [
                Workload::TokenizeAndStreamEval,
                Workload::PretokenizedStreamEval,
            ] {
                let source = benchmark_source(core, depth, iterations, copy, workload);
                let program = checked_program(&source);
                let verification = match engine {
                    Engine::Reference => vaak::interp::Interp::new()
                        .run(&program)
                        .expect("参照実装で検算できる"),
                    Engine::Vm => vaak::vm::run_program(
                        &vaak::vm::compile(&program).expect("VM へ翻訳できる"),
                    )
                    .expect("VM で検算できる"),
                };
                assert_eq!(answer(verification), expected);
                let elapsed = median(samples(engine, &program, sample_count));
                let ns = elapsed.as_nanos() / iterations as u128;
                if tsv {
                    println!(
                        "{engine_name}\t{}\t{environment}\t{depth}\t{source_bytes}\t{iterations}\t{sample_count}\t{ns}\t{expected}",
                        workload.name()
                    );
                } else {
                    println!(
                        "{engine_name}\t{}\t{environment}\t{elapsed:?}\t{ns}",
                        workload.name()
                    );
                }
            }
        }
    }
}

fn main() {
    // Vaak 内の再帰 LISP と参照実装の双方がホストの呼び出しスタックを使う。
    // 環境方式の比較が OS の既定スタック量で打ち切られないよう、測定全体を
    // 大きさを明記した一つのスレッドで行う。時間計測はスレッド作成後に始まる。
    std::thread::Builder::new()
        .name("vaak-lisp-benchmark".into())
        .stack_size(64 * 1024 * 1024)
        .spawn(run)
        .expect("測定スレッドを作れる")
        .join()
        .expect("測定スレッドが完走する");
}
