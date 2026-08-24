//! pure Vaak UTF-8 JSON Lines codecの差分試験。

use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use vaak::interp::Eval;

static TEMP_ID: AtomicU64 = AtomicU64::new(0);

fn source(body: &str) -> String {
    format!(
        "{}\n{}\n{}\n{body}",
        vaak::stdlib::STRING,
        vaak::stdlib::JSON_UTF8,
        vaak::stdlib::JSONL_UTF8
    )
}

#[track_caller]
fn checked(body: &str) -> String {
    let source = source(body);
    let program = vaak::parser::parse(&source).expect("JSONL codecを含むsourceを解析できる");
    let errors: Vec<_> = vaak::check::check(&program)
        .into_iter()
        .chain(vaak::types::check_types(&program))
        .map(|error| format!("{} @{:?}", error.msg, error.span))
        .collect();
    assert!(errors.is_empty(), "静的検査: {errors:?}\n{body}");
    source
}

fn shape(result: Result<Eval, String>) -> String {
    match result {
        Ok(Eval::Value(value)) => format!("値 {}", value.show()),
        Ok(Eval::Paradox(_)) => "paradox".into(),
        Ok(Eval::Akasha) => "虚無".into(),
        Ok(Eval::Escape(_)) => "脱出".into(),
        Err(error) => format!("エラー {error}"),
    }
}

#[track_caller]
fn reference_and_vm(body: &str, expected: &str) {
    let source = checked(body);
    let program = vaak::parser::parse(&source).expect("構文");
    let mut interpreter = vaak::interp::Interp::new();
    let reference = interpreter
        .run(&program)
        .map_err(|error| format!("{} @{}..{}", error.msg, error.span.start, error.span.end));
    assert_eq!(shape(reference), expected, "参照: {body}");
    assert_eq!(shape(vaak::vm::run(&source)), expected, "VM: {body}");
}

#[track_caller]
fn steel_native(body: &str, expected_exit: i32) {
    let source = checked(body);
    let program = vaak::parser::parse(&source).expect("構文");
    let ir = vaak::steel::compile(&program).unwrap_or_else(|error| panic!("STEEL: {}", error.msg));
    if Command::new("clang").arg("--version").output().is_err() {
        return;
    }

    let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!("vaak-jsonl-{}-{id}", std::process::id()));
    std::fs::create_dir(&directory).expect("専用一時directoryを作れる");
    let llvm = directory.join("program.ll");
    let executable = directory.join(if cfg!(windows) {
        "program.exe"
    } else {
        "program"
    });
    std::fs::write(&llvm, ir).expect("LLVM IRを書ける");
    let built = Command::new("clang")
        .arg("-O2")
        .arg("-o")
        .arg(&executable)
        .arg(&llvm)
        .output()
        .expect("clangを起動できる");
    if !built.status.success() {
        let message = String::from_utf8_lossy(&built.stderr).into_owned();
        let _ = std::fs::remove_dir_all(&directory);
        panic!("clang: {message}");
    }
    let status = Command::new(&executable)
        .status()
        .expect("STEEL生成物を実行できる");
    std::fs::remove_dir_all(&directory).expect("専用一時directoryを片付けられる");
    assert_eq!(status.code(), Some(expected_exit));
}

#[test]
fn jsonlの代表経路をsteel_nativeでも実行する() {
    let body = r#"
        let limits := jsonl_utf8_limits_default();
        var reader := jsonl_utf8_reader_new();
        let first := "20\n";
        let second := "22\n";
        jsonl_utf8_feed(reader, first, limits);
        jsonl_utf8_feed(reader, second, limits);
        let a := jsonl_utf8_next(reader, limits);
        let b := jsonl_utf8_next(reader, limits);
        let ad := a.document;
        let bd := b.document;
        (json_utf8_number_i64(ad, a.root) ?? 0) +
        (json_utf8_number_i64(bd, b.root) ?? 0)
    "#;
    reference_and_vm(body, "値 42");
    steel_native(body, 42);
}

#[test]
fn 一件読むと処理済みprefixをbufferから捨てる() {
    reference_and_vm(
        r#"
        let limits := jsonl_utf8_limits_default();
        var reader := jsonl_utf8_reader_new();
        let input := "1\n2\n";
        jsonl_utf8_feed(reader, input, limits);
        let value := jsonl_utf8_next(reader, limits);
        if (value.status == jsonl_utf8_status_value() && reader.buffer.len() == 2 &&
            reader.buffer[0] == 0x32 && reader.buffer_start == 2 &&
            reader.next_record == 1) 42 else 0 fi
        "#,
        "値 42",
    );
}

#[test]
fn 次chunk前に消費済みprefixをcompactしてbuffer上限を守る() {
    reference_and_vm(
        r#"
        var limits := jsonl_utf8_limits_default();
        limits.max_buffer_bytes := 6;
        var reader := jsonl_utf8_reader_new();
        let first_chunk := "1\n2222";
        let second_chunk := "\n";
        let first_feed := jsonl_utf8_feed(reader, first_chunk, limits);
        let first := jsonl_utf8_next(reader, limits);
        limits.max_buffer_bytes := 5;
        let second_feed := jsonl_utf8_feed(reader, second_chunk, limits);
        let second := jsonl_utf8_next(reader, limits);
        let first_document := first.document;
        let second_document := second.document;
        if (first_feed.ok && second_feed.ok &&
            (json_utf8_number_i64(first_document, first.root) ?? 0) == 1 &&
            (json_utf8_number_i64(second_document, second.root) ?? 0) == 2222 &&
            reader.buffer.len() == 0 && jsonl_utf8_buffered_bytes(reader) == 0 &&
            reader.buffer_start == 7 && reader.total_bytes == 7) 42 else 0 fi
        "#,
        "値 42",
    );
}

#[test]
fn lfとcrlfと終端改行なしを順に読む() {
    reference_and_vm(
        r#"
        let limits := jsonl_utf8_limits_default();
        var reader := jsonl_utf8_reader_new();
        let input := "1\n{\"a\":2}\r\ntrue";
        let fed := jsonl_utf8_feed(reader, input, limits);
        let finished := jsonl_utf8_finish(reader, limits);
        let a := jsonl_utf8_next(reader, limits);
        let b := jsonl_utf8_next(reader, limits);
        let c := jsonl_utf8_next(reader, limits);
        let end := jsonl_utf8_next(reader, limits);
        let a_document := a.document;
        let a_root := a.root;
        let b_document := b.document;
        let b_root := b.root;
        let c_document := c.document;
        let c_root := c.root;
        let child := json_utf8_child(b_document, b_root, 0) ?? -1;
        if (fed.ok && finished.ok &&
            a.status == jsonl_utf8_status_value() && a.record == 0 &&
            a.record_start == 0 && (json_utf8_number_i64(a_document, a_root) ?? 0) == 1 &&
            b.status == jsonl_utf8_status_value() && b.record == 1 &&
            b.record_start == 2 && (json_utf8_number_i64(b_document, child) ?? 0) == 2 &&
            c.status == jsonl_utf8_status_value() && c.record == 2 &&
            c.record_start == 11 && (json_utf8_boolean(c_document, c_root) ?? false) &&
            end.status == jsonl_utf8_status_end()) 42 else 0 fi
        "#,
        "値 42",
    );
}

#[test]
fn chunk境界をutf8とescapeとnumberの途中へ置ける() {
    reference_and_vm(
        r#"
        let limits := jsonl_utf8_limits_default();
        var reader := jsonl_utf8_reader_new();
        let a_bytes : u8 array := [
            0x7b, 0x22, 0x73, 0x22, 0x3a, 0x22, 0xe3
        ];
        let b_bytes : u8 array := [
            0x81, 0x82, 0x5c, 0x75, 0x30
        ];
        let c_bytes : u8 array := [
            0x30, 0x34, 0x31, 0x22, 0x2c, 0x22, 0x6e, 0x22, 0x3a, 0x31
        ];
        let d_bytes : u8 array := [0x32, 0x33, 0x7d, 0x0a];
        let a := new str(a_bytes);
        let b := new str(b_bytes);
        let c := new str(c_bytes);
        let d := new str(d_bytes);
        let fa := jsonl_utf8_feed(reader, a, limits);
        let na := jsonl_utf8_next(reader, limits);
        let fb := jsonl_utf8_feed(reader, b, limits);
        let nb := jsonl_utf8_next(reader, limits);
        let fc := jsonl_utf8_feed(reader, c, limits);
        let nc := jsonl_utf8_next(reader, limits);
        let fd := jsonl_utf8_feed(reader, d, limits);
        let value := jsonl_utf8_next(reader, limits);
        let document := value.document;
        let root := value.root;
        let text_node := json_utf8_child(document, root, 0) ?? -1;
        let number_node := json_utf8_child(document, root, 1) ?? -1;
        if (fa.ok && fb.ok && fc.ok && fd.ok &&
            na.status == jsonl_utf8_status_need_more() &&
            nb.status == jsonl_utf8_status_need_more() &&
            nc.status == jsonl_utf8_status_need_more() &&
            value.status == jsonl_utf8_status_value() &&
            (json_utf8_string(document, text_node) ?? "") == "あA" &&
            (json_utf8_number_i64(document, number_node) ?? 0) == 123) 42 else 0 fi
        "#,
        "値 42",
    );
}

#[test]
fn chunkを跨いだ不正utf8はrecord内先頭byteへ戻して報告する() {
    reference_and_vm(
        r#"
        let limits := jsonl_utf8_limits_default();
        var reader := jsonl_utf8_reader_new();
        let first_bytes : u8 array := [0xe3];
        let second_bytes : u8 array := [0x28, 0x0a];
        let first_chunk := new str(first_bytes);
        let second_chunk := new str(second_bytes);
        let a := jsonl_utf8_feed(reader, first_chunk, limits);
        let pending := jsonl_utf8_next(reader, limits);
        let b := jsonl_utf8_feed(reader, second_chunk, limits);
        let failed := jsonl_utf8_next(reader, limits);
        if (a.ok && b.ok && pending.status == jsonl_utf8_status_need_more() &&
            failed.status == jsonl_utf8_status_error() && failed.error.code == 100 &&
            failed.error.record == 0 && failed.error.record_start == 0 &&
            failed.error.offset == 0 && failed.error.absolute_offset == 0 &&
            failed.error.line == 1 && failed.error.column == 1) 42 else 0 fi
        "#,
        "値 42",
    );
}

#[test]
fn parse_errorへrecord番号と絶対位置を付けてterminalにする() {
    reference_and_vm(
        r#"
        let limits := jsonl_utf8_limits_default();
        var reader := jsonl_utf8_reader_new();
        let input := "null\n[truX]\n";
        let fed := jsonl_utf8_feed(reader, input, limits);
        let first := jsonl_utf8_next(reader, limits);
        let failed := jsonl_utf8_next(reader, limits);
        let repeated := jsonl_utf8_next(reader, limits);
        if (fed.ok && first.status == jsonl_utf8_status_value() &&
            failed.status == jsonl_utf8_status_error() && failed.root == -1 &&
            failed.document.kinds.len() == 0 && failed.error.category == 1 &&
            failed.error.code == 102 && failed.error.record == 1 &&
            failed.error.record_start == 5 && failed.error.offset == 1 &&
            failed.error.absolute_offset == 6 && failed.error.line == 1 &&
            failed.error.column == 2 && repeated.error.code == failed.error.code &&
            repeated.error.absolute_offset == failed.error.absolute_offset) 42 else 0 fi
        "#,
        "値 42",
    );
}

#[test]
fn 空または空白だけのrecordをjsonl固有errorにする() {
    reference_and_vm(
        r#"
        let limits := jsonl_utf8_limits_default();
        var empty_reader := jsonl_utf8_reader_new();
        let empty := "\n";
        jsonl_utf8_feed(empty_reader, empty, limits);
        let a := jsonl_utf8_next(empty_reader, limits);

        var blank_reader := jsonl_utf8_reader_new();
        let blank := " \t\r\n";
        jsonl_utf8_feed(blank_reader, blank, limits);
        let b := jsonl_utf8_next(blank_reader, limits);
        if (a.status == jsonl_utf8_status_error() && a.error.category == 4 &&
            a.error.code == 400 && a.error.record == 0 && a.error.absolute_offset == 0 &&
            b.error.code == 400 && b.error.offset == 0 && b.error.line == 1 &&
            b.error.column == 1) 42 else 0 fi
        "#,
        "値 42",
    );
}

#[test]
fn record総量bufferと不正limitを別codeで止める() {
    reference_and_vm(
        r#"
        var record_limits := jsonl_utf8_limits_default();
        record_limits.max_record_bytes := 3;
        var record_reader := jsonl_utf8_reader_new();
        let null_input := "null";
        let ra := jsonl_utf8_feed(record_reader, null_input, record_limits);
        let a := jsonl_utf8_next(record_reader, record_limits);

        var total_limits := jsonl_utf8_limits_default();
        total_limits.max_total_bytes := 3;
        var total_reader := jsonl_utf8_reader_new();
        let prefix := "nu";
        let suffix := "ll";
        let tb := jsonl_utf8_feed(total_reader, prefix, total_limits);
        let b := jsonl_utf8_feed(total_reader, suffix, total_limits);

        var buffer_limits := jsonl_utf8_limits_default();
        buffer_limits.max_buffer_bytes := 3;
        var buffer_reader := jsonl_utf8_reader_new();
        let cb := jsonl_utf8_feed(buffer_reader, prefix, buffer_limits);
        let c := jsonl_utf8_feed(buffer_reader, suffix, buffer_limits);

        var invalid_limits := jsonl_utf8_limits_default();
        invalid_limits.max_record_bytes := -1;
        var invalid_reader := jsonl_utf8_reader_new();
        let d := jsonl_utf8_feed(invalid_reader, null_input, invalid_limits);
        if (ra.ok && a.error.code == 401 && a.error.offset == 3 &&
            tb.ok && ! b.ok && b.error.code == 402 && total_reader.buffer.len() == 2 &&
            total_reader.total_bytes == 2 &&
            cb.ok && ! c.ok && c.error.code == 404 && buffer_reader.buffer.len() == 2 &&
            ! d.ok && d.error.code == 405) 42 else 0 fi
        "#,
        "値 42",
    );
}

#[test]
fn compact前でもrecord上限errorは現在recordの絶対開始を使う() {
    reference_and_vm(
        r#"
        var limits := jsonl_utf8_limits_default();
        limits.max_record_bytes := 3;
        var reader := jsonl_utf8_reader_new();
        let input := "0\nabcd";
        let fed := jsonl_utf8_feed(reader, input, limits);
        let first := jsonl_utf8_next(reader, limits);
        let failed := jsonl_utf8_next(reader, limits);
        let first_document := first.document;
        if (fed.ok && (json_utf8_number_i64(first_document, first.root) ?? -1) == 0 &&
            failed.status == jsonl_utf8_status_error() && failed.error.code == 401 &&
            failed.error.record == 1 && failed.error.record_start == 2 &&
            failed.error.offset == 3 && failed.error.absolute_offset == 5) 42 else 0 fi
        "#,
        "値 42",
    );
}

#[test]
fn finishは冪等でその後のfeedだけをstate_errorにする() {
    reference_and_vm(
        r#"
        let limits := jsonl_utf8_limits_default();
        var reader := jsonl_utf8_reader_new();
        let input := "1";
        let extra := "2";
        let fed := jsonl_utf8_feed(reader, input, limits);
        let first_finish := jsonl_utf8_finish(reader, limits);
        let second_finish := jsonl_utf8_finish(reader, limits);
        let rejected := jsonl_utf8_feed(reader, extra, limits);
        let repeated := jsonl_utf8_next(reader, limits);
        if (fed.ok && first_finish.ok && second_finish.ok && ! rejected.ok &&
            rejected.error.code == 403 && reader.buffer.len() == 1 &&
            reader.total_bytes == 1 && repeated.status == jsonl_utf8_status_error() &&
            repeated.error.code == 403) 42 else 0 fi
        "#,
        "値 42",
    );
}

#[test]
fn reader同士のbufferとrecord位置は共有されない() {
    reference_and_vm(
        r#"
        let limits := jsonl_utf8_limits_default();
        var left := jsonl_utf8_reader_new();
        var right := jsonl_utf8_reader_new();
        let left_a := "1";
        let left_b := "\n2\n";
        let right_input := "9\n";
        jsonl_utf8_feed(left, left_a, limits);
        let pending := jsonl_utf8_next(left, limits);
        jsonl_utf8_feed(right, right_input, limits);
        let nine := jsonl_utf8_next(right, limits);
        jsonl_utf8_feed(left, left_b, limits);
        let one := jsonl_utf8_next(left, limits);
        let two := jsonl_utf8_next(left, limits);
        let nine_document := nine.document;
        let nine_root := nine.root;
        let one_document := one.document;
        let one_root := one.root;
        let two_document := two.document;
        let two_root := two.root;
        if (pending.status == jsonl_utf8_status_need_more() &&
            (json_utf8_number_i64(nine_document, nine_root) ?? 0) == 9 &&
            nine.record == 0 && nine.record_start == 0 &&
            (json_utf8_number_i64(one_document, one_root) ?? 0) == 1 &&
            one.record == 0 && one.record_start == 0 &&
            (json_utf8_number_i64(two_document, two_root) ?? 0) == 2 &&
            two.record == 1 && two.record_start == 2) 42 else 0 fi
        "#,
        "値 42",
    );
}

#[test]
fn serialize_lineはlfを一つ足し失敗時は空bytesにする() {
    reference_and_vm(
        r#"
        let limits := jsonl_utf8_limits_default();
        var document := json_utf8_document_new();
        let text := "x\n";
        let json_limits := limits.json;
        let root := json_utf8_add_string(document, text, json_limits) ?? -1;
        let line := jsonl_utf8_serialize_line(document, root, limits);
        let line_bytes := line.bytes;
        let expected := "\"x\\n\"\n";

        var small := jsonl_utf8_limits_default();
        small.max_record_bytes := 2;
        let too_large := jsonl_utf8_serialize_line(document, root, small);

        var malformed := document;
        malformed.child_counts[root] := 1;
        let invalid := jsonl_utf8_serialize_line(malformed, root, limits);

        var total_small := jsonl_utf8_limits_default();
        total_small.max_total_bytes := 4;
        let total_failure := jsonl_utf8_serialize_line(document, root, total_small);

        var buffer_small := jsonl_utf8_limits_default();
        buffer_small.max_buffer_bytes := 4;
        let buffer_failure := jsonl_utf8_serialize_line(document, root, buffer_small);
        if (line.ok && json_utf8__bytes_equal(line_bytes, expected) &&
            ! too_large.ok && too_large.bytes.len() == 0 && too_large.error.code == 401 &&
            ! invalid.ok && invalid.bytes.len() == 0 && invalid.error.code == 300 &&
            ! total_failure.ok && total_failure.bytes.len() == 0 &&
            total_failure.error.code == 402 &&
            ! buffer_failure.ok && buffer_failure.bytes.len() == 0 &&
            buffer_failure.error.code == 404) 42 else 0 fi
        "#,
        "値 42",
    );
}
