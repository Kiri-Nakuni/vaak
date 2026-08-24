//! JSONL readerで一括chunkから全recordを読む費用を測る。
//!
//! `RECORDS=512 ROUNDS=5 ENGINE=both cargo run --release --locked --example bench_jsonl`。
//! parse/check/VM compileは計測外で、各実行はreaderを新しく作る。

use std::fmt::Write as _;
use std::hint::black_box;
use std::time::{Duration, Instant};

use vaak::interp::Eval;

fn program(records: usize) -> (String, i128, usize) {
    let mut escaped_input = String::new();
    let mut bytes = 0;
    let mut expected = 0_i128;
    for value in 0..records {
        let digits = value.to_string();
        bytes += digits.len() + 1;
        escaped_input.push_str(&digits);
        escaped_input.push_str("\\n");
        expected += value as i128;
    }
    let body = format!(
        r#"
        let limits := jsonl_utf8_limits_default();
        var reader := jsonl_utf8_reader_new();
        let input := "{escaped_input}";
        let fed := jsonl_utf8_feed(reader, input, limits);
        let finished := jsonl_utf8_finish(reader, limits);
        var total := 0;
        var done := false;
        var failed := ! fed.ok || ! finished.ok;
        while (! done && ! failed) {{
            let item := jsonl_utf8_next(reader, limits);
            if (item.status == jsonl_utf8_status_value()) {{
                let document := item.document;
                total += json_utf8_number_i64(document, item.root) ?? 0;
            }} elif (item.status == jsonl_utf8_status_end()) {{
                done := true;
            }} else {{
                failed := true;
            }} fi;
        }};
        if (failed) -1 else total fi
        "#
    );
    (
        format!(
            "{}\n{}\n{}\n{body}",
            vaak::stdlib::STRING,
            vaak::stdlib::JSON_UTF8,
            vaak::stdlib::JSONL_UTF8
        ),
        expected,
        bytes,
    )
}

fn vaak_string(bytes: &[u8]) -> String {
    let mut escaped = String::new();
    for &byte in bytes {
        match byte {
            b'"' => escaped.push_str("\\\""),
            b'\\' => escaped.push_str("\\\\"),
            b'\n' => escaped.push_str("\\n"),
            0x20..=0x7e => escaped.push(byte as char),
            _ => write!(escaped, "\\x{byte:02x}").expect("文字列への書き込み"),
        }
    }
    escaped
}

fn split_program(payload_bytes: usize, chunk_bytes: usize) -> (String, i128, usize) {
    let mut input = Vec::with_capacity(payload_bytes + 3);
    input.push(b'"');
    input.extend(std::iter::repeat_n(b'a', payload_bytes));
    input.extend([b'"', b'\n']);
    let chunks = input.chunks(chunk_bytes).collect::<Vec<_>>();

    let mut body = String::from(
        r#"
        let limits := jsonl_utf8_limits_default();
        var reader := jsonl_utf8_reader_new();
        var failed := false;
        var decoded := -1;
        "#,
    );
    for (index, chunk) in chunks.iter().enumerate() {
        let escaped = vaak_string(chunk);
        writeln!(
            body,
            "let chunk_{index} := \"{escaped}\"; let feed_{index} := jsonl_utf8_feed(reader, chunk_{index}, limits); let item_{index} := jsonl_utf8_next(reader, limits);"
        )
        .expect("program文字列への書き込み");
        if index + 1 == chunks.len() {
            writeln!(
                body,
                "let document_{index} := item_{index}.document; let text_{index} := json_utf8_string(document_{index}, item_{index}.root) ?? \"\"; decoded := text_{index}.len(); if (! feed_{index}.ok || item_{index}.status != jsonl_utf8_status_value()) failed := true; fi;"
            )
            .expect("program文字列への書き込み");
        } else {
            writeln!(
                body,
                "if (! feed_{index}.ok || item_{index}.status != jsonl_utf8_status_need_more()) failed := true; fi;"
            )
            .expect("program文字列への書き込み");
        }
    }
    body.push_str(
        r#"
        let finished := jsonl_utf8_finish(reader, limits);
        let end := jsonl_utf8_next(reader, limits);
        if (! finished.ok || end.status != jsonl_utf8_status_end()) failed := true; fi;
        if (failed) -1 else decoded fi
        "#,
    );

    (
        format!(
            "{}\n{}\n{}\n{body}",
            vaak::stdlib::STRING,
            vaak::stdlib::JSON_UTF8,
            vaak::stdlib::JSONL_UTF8
        ),
        payload_bytes as i128,
        input.len(),
    )
}

fn result(result: Result<Eval, impl std::fmt::Debug>) -> i128 {
    match result {
        Ok(Eval::Value(value)) => value.as_int().expect("整数結果"),
        Ok(other) => panic!("値でない結果: {other:?}"),
        Err(error) => panic!("実行時: {error:?}"),
    }
}

fn median(mut run: impl FnMut() -> i128, expected: i128, rounds: usize) -> Duration {
    assert_eq!(black_box(run()), expected, "予熱");
    let mut samples = Vec::with_capacity(rounds);
    for _ in 0..rounds {
        let started = Instant::now();
        assert_eq!(black_box(run()), expected, "測定結果");
        samples.push(started.elapsed());
    }
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn main() {
    let mode = std::env::var("MODE").unwrap_or_else(|_| "records".into());
    let records = std::env::var("RECORDS")
        .ok()
        .and_then(|text| text.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(512);
    let record_bytes = std::env::var("RECORD_BYTES")
        .ok()
        .and_then(|text| text.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(4096);
    let chunk_bytes = std::env::var("CHUNK_BYTES")
        .ok()
        .and_then(|text| text.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(16);
    let rounds = std::env::var("ROUNDS")
        .ok()
        .and_then(|text| text.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(5);
    let engine = std::env::var("ENGINE").unwrap_or_else(|_| "both".into());

    let (source, expected, bytes) = if mode == "split" {
        split_program(record_bytes, chunk_bytes)
    } else {
        program(records)
    };
    let parsed = vaak::parser::parse(&source).expect("構文");
    let errors: Vec<_> = vaak::check::check(&parsed)
        .into_iter()
        .chain(vaak::types::check_types(&parsed))
        .map(|error| error.msg)
        .collect();
    assert!(errors.is_empty(), "静的検査: {errors:?}");
    let compiled = vaak::vm::compile(&parsed).expect("VM compile");

    if mode == "split" {
        println!(
            "mode=split record_bytes={record_bytes} chunk_bytes={chunk_bytes} bytes={bytes} rounds={rounds} median"
        );
    } else {
        println!("mode=records records={records} bytes={bytes} rounds={rounds} median");
    }
    if engine == "both" || engine == "interp" {
        let elapsed = median(
            || {
                result(
                    vaak::interp::Interp::new()
                        .run(&parsed)
                        .map_err(|error| error.msg),
                )
            },
            expected,
            rounds,
        );
        println!("reference {elapsed:?}");
    }
    if engine == "both" || engine == "vm" {
        let mut runner = vaak::vm::Runner::new();
        let elapsed = median(
            || {
                let (eval, _) = runner.run(&compiled, Vec::new()).expect("VM run");
                result(Ok::<Eval, String>(eval))
            },
            expected,
            rounds,
        );
        println!("vm        {elapsed:?}");
    }
}
