//! pure Vaak 文字列ライブラリの探索費用を測る。
//!
//! 測定対象の構文解析と VM の組み立ては計時外。時間は最小値で比べ、通常試験
//! には閾値を置かない。末尾にだけ `z` を置き、必ず入力全体を走査させる。

use std::time::{Duration, Instant};

use vaak::interp::Eval;
use vaak::value::Value;

#[derive(Clone, Copy)]
enum Engine {
    Interp,
    Vm,
}

impl Engine {
    fn name(self) -> &'static str {
        match self {
            Self::Interp => "参照",
            Self::Vm => "VM",
        }
    }
}

fn source(body: &str) -> String {
    format!("{}\n{body}", vaak::stdlib::STRING)
}

fn result_i64(eval: Eval) -> i64 {
    match eval {
        Eval::Value(Value::I64(value)) => value,
        other => panic!("i64 でない結果: {other:?}"),
    }
}

fn minimum(mut run: impl FnMut() -> i64, expected: i64, rounds: usize) -> Duration {
    assert_eq!(run(), expected, "予熱の結果");
    let mut best = Duration::MAX;
    for _ in 0..rounds {
        let started = Instant::now();
        let result = run();
        let elapsed = started.elapsed();
        assert_eq!(result, expected, "測定の結果");
        best = best.min(elapsed);
    }
    best
}

fn measure(src: &str, expected: i64, engine: Engine, rounds: usize) -> Duration {
    let program = vaak::parser::parse(src).expect("構文解析");
    let errors: Vec<_> = vaak::check::check(&program)
        .into_iter()
        .chain(vaak::types::check_types(&program))
        .map(|error| error.msg)
        .collect();
    assert!(errors.is_empty(), "静的検査: {errors:?}");

    match engine {
        Engine::Interp => minimum(
            || {
                let mut interp = vaak::interp::Interp::new();
                result_i64(interp.run(&program).expect("参照実装"))
            },
            expected,
            rounds,
        ),
        Engine::Vm => {
            let compiled = vaak::vm::compile(&program).expect("VM 組み立て");
            let mut runner = vaak::vm::Runner::new();
            minimum(
                || {
                    let (eval, _) = runner.run(&compiled, Vec::new()).expect("VM 実行");
                    result_i64(eval)
                },
                expected,
                rounds,
            )
        }
    }
}

fn find_program(bytes: usize, calls: i64, kind: &str) -> String {
    let mut haystack = "a".repeat(bytes - 1);
    haystack.push('z');
    let (definition, call) = match kind {
        "byte" => ("", "str_find_byte(text, 122, 0)"),
        "inline" => (
            r#"fn bench_find_inline (source : str alias, needle : str, from : i64) {
                    if (from < 0 || from > source.len()) $return; fi;
                    if (needle.len() != 1) $return; fi;
                    let byte := needle[0] ?? $return;
                    nfor (i, from, source.len() - from) {
                        if (source[i] == byte) $return i; fi;
                    };
                } -> i64;"#,
            "bench_find_inline(text, \"z\", 0)",
        ),
        "generic" => ("", "str_find(text, \"z\")"),
        _ => unreachable!(),
    };
    source(&format!(
        r#"{definition}
            let text := "{haystack}";
            var total := 0;
            nfor (i, 0, {calls}) {{
                total += {call} ?? -1;
            }};
            total"#
    ))
}

fn argument_program(bytes: usize, calls: i64, alias: bool) -> String {
    let haystack = "a".repeat(bytes);
    let parameter = if alias { "str alias" } else { "str" };
    source(&format!(
        r#"fn bench_touch (text : {parameter}) {{ text.len() }} -> i64;
            let text := "{haystack}";
            var total := 0;
            nfor (i, 0, {calls}) {{ total += bench_touch(text); }};
            total"#
    ))
}

fn repeated_prefix_program(bytes: usize, needle_bytes: usize) -> String {
    let haystack = "a".repeat(bytes);
    let mut needle = "a".repeat(needle_bytes - 1);
    needle.push('z');
    source(&format!(
        r#"let text := "{haystack}";
            str_find(text, "{needle}") ?? -1"#
    ))
}

fn main() {
    // 一般探索は候補ごとに str__match_at の名前付きフレームを作るので、
    // 大入力で測定時間が伸びすぎない回数にする。
    let sizes = [(4 * 1024, 4_i64), (32 * 1024, 1_i64)];

    println!("探索（対象の構文解析・VM 組み立てを除く最小値）");
    println!("engine bytes kind       ns/search   ns/candidate");
    for engine in [Engine::Interp, Engine::Vm] {
        for (bytes, calls) in sizes {
            for label in ["byte", "inline", "generic"] {
                let src = find_program(bytes, calls, label);
                let elapsed = measure(&src, (bytes as i64 - 1) * calls, engine, 4);
                let ns_per_search = elapsed.as_nanos() as f64 / calls as f64;
                let ns_per_candidate = ns_per_search / bytes as f64;
                println!(
                    "{:<6} {:>5} {:<8} {:>12.0} {:>14.1}",
                    engine.name(),
                    bytes,
                    label,
                    ns_per_search,
                    ns_per_candidate
                );
            }
        }
    }

    println!("\n反復接頭辞の最悪形（4 KiB の a 列から a×31+z を探索）");
    println!("engine ns/search  ns/candidate");
    let bytes = 4 * 1024;
    let needle_bytes = 32;
    let candidates = bytes - needle_bytes + 1;
    let src = repeated_prefix_program(bytes, needle_bytes);
    for engine in [Engine::Interp, Engine::Vm] {
        let elapsed = measure(&src, -1, engine, 3);
        println!(
            "{:<6} {:>9} {:>13.1}",
            engine.name(),
            elapsed.as_nanos(),
            elapsed.as_nanos() as f64 / candidates as f64
        );
    }

    println!("\n32 KiB 引数（同じ関数を512回、最小値）");
    println!("engine mode       ns/call");
    let bytes = 32 * 1024;
    let calls = 512_i64;
    for engine in [Engine::Interp, Engine::Vm] {
        for (label, alias) in [("alias", true), ("value", false)] {
            let src = argument_program(bytes, calls, alias);
            let elapsed = measure(&src, bytes as i64 * calls, engine, 5);
            let ns_per_call = elapsed.as_nanos() as f64 / calls as f64;
            println!("{:<6} {:<8} {:>12.0}", engine.name(), label, ns_per_call);
        }
    }
}
