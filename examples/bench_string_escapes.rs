//! 文字列リテラルのescape処理を、同じsource規模でlex / parse計測する。
//!
//! `ROUNDS=100 cargo run --release --locked --example bench_string_escapes`。

use std::hint::black_box;
use std::time::{Duration, Instant};
use vaak::lexer::Tok;

const BODY_BYTES: usize = 64 * 1024;

struct Case {
    name: &'static str,
    source: String,
}

fn body(unit: &str) -> String {
    let mut body = String::with_capacity(BODY_BYTES);
    while body.len() + unit.len() <= BODY_BYTES {
        body.push_str(unit);
    }
    body
}

fn source(unit: &str) -> String {
    format!("let text := \"{}\"; text.len()", body(unit))
}

fn measure(mut run: impl FnMut(), rounds: u32) -> Duration {
    for _ in 0..3 {
        run();
    }
    let started = Instant::now();
    for _ in 0..rounds {
        run();
    }
    started.elapsed() / rounds
}

fn decoded_len(source: &str) -> usize {
    vaak::lexer::lex(source)
        .expect("benchmark sourceを字句解析できる")
        .into_iter()
        .find_map(|token| match token.tok {
            Tok::Str(value) => Some(value.len()),
            _ => None,
        })
        .expect("文字列tokenがある")
}

fn main() {
    let rounds = std::env::var("ROUNDS")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|rounds| *rounds > 0)
        .unwrap_or(100);
    let cases = [
        Case {
            name: "plain ASCII",
            source: source("abcdefghijklmnop"),
        },
        Case {
            name: "existing escapes",
            source: source(r#"\n\t\x41\u{3042}\\\"\0"#),
        },
        Case {
            name: "carriage return",
            source: source(r#"\r"#),
        },
    ];

    println!("rounds={rounds}, body target={BODY_BYTES} bytes");
    println!(
        "{:<20} {:>10} {:>10} {:>12} {:>12}",
        "case", "source", "decoded", "lex", "parse"
    );
    for case in &cases {
        let lex = measure(
            || {
                black_box(vaak::lexer::lex(black_box(&case.source)).expect("字句解析"));
            },
            rounds,
        );
        let parse = measure(
            || {
                black_box(vaak::parser::parse(black_box(&case.source)).expect("構文解析"));
            },
            rounds,
        );
        println!(
            "{:<20} {:>10} {:>10} {:>12?} {:>12?}",
            case.name,
            case.source.len(),
            decoded_len(&case.source),
            lex,
            parse
        );
    }
}
