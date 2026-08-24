//! pure Vaak UTF-8 JSON / JSONL codecの差分試験。

use vaak::interp::Eval;

fn source(body: &str) -> String {
    format!(
        "{}\n{}\n{body}",
        vaak::stdlib::STRING,
        vaak::stdlib::JSON_UTF8
    )
}

#[track_caller]
fn checked(body: &str) -> String {
    let source = source(body);
    let program = vaak::parser::parse(&source).expect("JSON codecを含むsourceを解析できる");
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

#[test]
fn objectとarrayをflat_documentへparseする() {
    reference_and_vm(
        r#"
        let limits := json_utf8_limits_default();
        let input := "{\"name\":\"Vaak\",\"values\":[null,true,-42]}";
        let parsed := json_utf8_parse(input, limits);
        let document := parsed.document;
        let root := parsed.root;
        let root_kind := json_utf8_kind(document, root) ?? 255;
        let values := json_utf8_child(document, root, 1) ?? -1;
        let number := json_utf8_child(document, values, 2) ?? -1;
        if (parsed.ok && root_kind == json_utf8_kind_object() &&
            (json_utf8_object_key(document, root, 0) ?? "") == "name" &&
            (json_utf8_number_i64(document, number) ?? 0) == -42) 42 else 0 fi
        "#,
        "値 42",
    );
}

#[test]
fn 空の集合体と一要素をparseする() {
    reference_and_vm(
        r#"
        let limits := json_utf8_limits_default();
        let sa := "{}";
        let sb := "[]";
        let sc := "[1]";
        let sd := "{\"a\":1}";
        let a := json_utf8_parse(sa, limits);
        let b := json_utf8_parse(sb, limits);
        let c := json_utf8_parse(sc, limits);
        let d := json_utf8_parse(sd, limits);
        if (! a.ok) a.error.code * 1000 + a.error.offset
        elif (! b.ok) 1000000 + b.error.code * 1000 + b.error.offset
        elif (! c.ok) 2000000 + c.error.code * 1000 + c.error.offset
        elif (! d.ok) 3000000 + d.error.code * 1000 + d.error.offset
        else 42 fi
        "#,
        "値 42",
    );
}

#[test]
fn top_levelの全scalar_kindをparseする() {
    reference_and_vm(
        r#"
        let limits := json_utf8_limits_default();
        let null_input := "null";
        let true_input := "true";
        let false_input := "false";
        let number_input := "-7";
        let string_input := "\"Vaak\"";
        let a := json_utf8_parse(null_input, limits);
        let b := json_utf8_parse(true_input, limits);
        let c := json_utf8_parse(false_input, limits);
        let d := json_utf8_parse(number_input, limits);
        let e := json_utf8_parse(string_input, limits);
        let ad := a.document;
        let bd := b.document;
        let cd := c.document;
        let dd := d.document;
        let ed := e.document;
        if (a.ok && (json_utf8_kind(ad, a.root) ?? 255) == json_utf8_kind_null() &&
            b.ok && (json_utf8_boolean(bd, b.root) ?? false) &&
            c.ok && ! (json_utf8_boolean(cd, c.root) ?? true) &&
            d.ok && (json_utf8_number_i64(dd, d.root) ?? 0) == -7 &&
            e.ok && (json_utf8_string(ed, e.root) ?? "") == "Vaak") 42 else 0 fi
        "#,
        "値 42",
    );
}

#[test]
fn i64両端と負のzeroを正規化して往復する() {
    reference_and_vm(
        r#"
        let limits := json_utf8_limits_default();
        let min_input := "-9223372036854775808";
        let max_input := "9223372036854775807";
        let zero_input := "0";
        let negative_zero_input := "-0";
        let minimum := json_utf8_parse(min_input, limits);
        let maximum := json_utf8_parse(max_input, limits);
        let zero := json_utf8_parse(zero_input, limits);
        let negative_zero := json_utf8_parse(negative_zero_input, limits);
        let minimum_document := minimum.document;
        let maximum_document := maximum.document;
        let zero_document := zero.document;
        let negative_zero_document := negative_zero.document;
        let serialized_minimum := json_utf8_serialize(
            minimum_document, minimum.root, limits
        );
        let serialized_maximum := json_utf8_serialize(
            maximum_document, maximum.root, limits
        );
        let serialized_negative_zero := json_utf8_serialize(
            negative_zero_document, negative_zero.root, limits
        );
        let minimum_bytes := serialized_minimum.bytes;
        let maximum_bytes := serialized_maximum.bytes;
        let negative_zero_bytes := serialized_negative_zero.bytes;
        if (minimum.ok && maximum.ok && zero.ok && negative_zero.ok &&
            (json_utf8_number_i64(minimum_document, minimum.root) ?? 0) ==
                (0 - 9223372036854775807 - 1) &&
            (json_utf8_number_i64(maximum_document, maximum.root) ?? 0) ==
                9223372036854775807 &&
            (json_utf8_number_i64(zero_document, zero.root) ?? 1) == 0 &&
            (json_utf8_number_i64(negative_zero_document, negative_zero.root) ?? 1) == 0 &&
            json_utf8__bytes_equal(minimum_bytes, min_input) &&
            json_utf8__bytes_equal(maximum_bytes, max_input) &&
            json_utf8__bytes_equal(negative_zero_bytes, zero_input)) 42 else 0 fi
        "#,
        "値 42",
    );
}

#[test]
fn 全escapeとunicodeをutf8へdecodeして決定的に往復する() {
    reference_and_vm(
        r#"
        let limits := json_utf8_limits_default();
        let input := "\"\\\"\\\\\\/\\b\\f\\n\\r\\t\\u0000\\u007f\\u0080\\u20ac\"";
        let parsed := json_utf8_parse(input, limits);
        let document := parsed.document;
        let text := json_utf8_string(document, parsed.root) ?? "";
        let expected_bytes : u8 array := [
            0x22, 0x5c, 0x2f, 0x08, 0x0c, 0x0a, 0x0d, 0x09,
            0x00, 0x7f, 0xc2, 0x80, 0xe2, 0x82, 0xac
        ];
        let expected := new str(expected_bytes);
        let first := json_utf8_serialize(document, parsed.root, limits);
        let first_bytes := first.bytes;
        let second := json_utf8_serialize(document, parsed.root, limits);
        let second_bytes := second.bytes;
        let reparsed := json_utf8_parse(first_bytes, limits);
        let reparsed_document := reparsed.document;
        let reparsed_text := json_utf8_string(reparsed_document, reparsed.root) ?? "";
        if (parsed.ok && first.ok && second.ok && reparsed.ok &&
            json_utf8__bytes_equal(text, expected) &&
            json_utf8__bytes_equal(first_bytes, second_bytes) &&
            json_utf8__bytes_equal(reparsed_text, expected)) 42 else 0 fi
        "#,
        "値 42",
    );
}

#[test]
fn builderはduplicate_keyを追加前に拒否する() {
    reference_and_vm(
        r#"
        let limits := json_utf8_limits_default();
        var document := json_utf8_document_new();
        let child := json_utf8_add_null(document, limits) ?? -1;
        let keys := ["same", "same"];
        let children := [child, child];
        let before_nodes := document.kinds.len();
        let before_edges := document.children.len();
        let rejected := json_utf8_add_object(document, keys, children, limits) ?? -1;
        if (rejected == -1 && document.kinds.len() == before_nodes &&
            document.children.len() == before_edges) 42 else 0 fi
        "#,
        "値 42",
    );
}

#[test]
fn builderはobject順を保って決定的にserializeする() {
    reference_and_vm(
        r#"
        let limits := json_utf8_limits_default();
        var document := json_utf8_document_new();
        let minimum := 0 - 9223372036854775807 - 1;
        let number := json_utf8_add_number_i64(document, minimum, limits) ?? -1;
        let text := "A\x00\nあ";
        let string := json_utf8_add_string(document, text, limits) ?? -1;
        let boolean := json_utf8_add_boolean(document, true, limits) ?? -1;
        let array_children := [number, string];
        let array_node := json_utf8_add_array(document, array_children, limits) ?? -1;
        let keys := ["values", "enabled"];
        let object_children := [array_node, boolean];
        let root := json_utf8_add_object(document, keys, object_children, limits) ?? -1;
        let serialized := json_utf8_serialize(document, root, limits);
        let bytes := serialized.bytes;
        let expected := "{\"values\":[-9223372036854775808,\"A\\u0000\\nあ\"],\"enabled\":true}";
        if (serialized.ok && json_utf8__bytes_equal(bytes, expected)) 42 else 0 fi
        "#,
        "値 42",
    );
}

#[test]
fn parse後のserializeは空白とescapeをcanonicalにする() {
    reference_and_vm(
        r#"
        let limits := json_utf8_limits_default();
        let input := " { \"b\" : [ 1 , \"\\uD83D\\uDE00\" ], \"a\":-0 } ";
        let parsed := json_utf8_parse(input, limits);
        let document := parsed.document;
        let root := parsed.root;
        let serialized := json_utf8_serialize(document, root, limits);
        let bytes := serialized.bytes;
        let expected := "{\"b\":[1,\"😀\"],\"a\":0}";
        if (parsed.ok && serialized.ok && json_utf8__bytes_equal(bytes, expected)) 42 else 0 fi
        "#,
        "値 42",
    );
}

#[test]
fn parse_errorはstable_codeとbyte位置を返して部分documentを捨てる() {
    reference_and_vm(
        r#"
        let limits := json_utf8_limits_default();
        let invalid_utf8_bytes : u8 array := [0x22, 0xc0, 0xaf, 0x22];
        let invalid_utf8 := new str(invalid_utf8_bytes);
        let bad_escape_bytes : u8 array := [0x22, 0x5c, 0x71, 0x22];
        let bad_escape := new str(bad_escape_bytes);
        let surrogate := "\"\\uD800\"";
        let range := "9223372036854775808";
        let decimal := "1.0";
        let trailing := "null x";
        let duplicate := "{\"a\":1,\"a\":2}";
        let leading_zero := "01";
        let line_error := "[\n  truX]";
        let a := json_utf8_parse(invalid_utf8, limits);
        let b := json_utf8_parse(bad_escape, limits);
        let c := json_utf8_parse(surrogate, limits);
        let d := json_utf8_parse(range, limits);
        let e := json_utf8_parse(decimal, limits);
        let f := json_utf8_parse(trailing, limits);
        let g := json_utf8_parse(duplicate, limits);
        let h := json_utf8_parse(leading_zero, limits);
        let i := json_utf8_parse(line_error, limits);
        if (! a.ok && a.root == -1 && a.document.kinds.len() == 0 &&
            a.error.category == 1 && a.error.code == 100 && a.error.offset == 1 &&
            b.error.code == 104 && b.error.offset == 1 &&
            c.error.code == 105 && c.error.offset == 1 &&
            d.error.code == 108 && d.error.offset == 0 &&
            e.error.code == 109 && e.error.offset == 0 &&
            f.error.code == 103 && f.error.offset == 5 &&
            g.error.code == 110 && g.error.offset == 7 &&
            h.error.code == 107 && h.error.offset == 1 &&
            i.error.code == 102 && i.error.offset == 4 &&
            i.error.line == 2 && i.error.column == 3) 42 else 0 fi
        "#,
        "値 42",
    );
}

#[test]
fn parse_limitは入力深さnode文字列edgeを別codeで止める() {
    reference_and_vm(
        r#"
        var byte_limits := json_utf8_limits_default();
        byte_limits.max_bytes := 3;
        let null_input := "null";
        let a := json_utf8_parse(null_input, byte_limits);

        var depth_limits := json_utf8_limits_default();
        depth_limits.max_depth := 1;
        let nested_input := "[[]]";
        let b := json_utf8_parse(nested_input, depth_limits);

        var node_limits := json_utf8_limits_default();
        node_limits.max_nodes := 1;
        let one_child := "[null]";
        let c := json_utf8_parse(one_child, node_limits);

        var string_limits := json_utf8_limits_default();
        string_limits.max_string_bytes := 1;
        let long_string := "\"ab\"";
        let d := json_utf8_parse(long_string, string_limits);

        var edge_limits := json_utf8_limits_default();
        edge_limits.max_edges := 0;
        let e := json_utf8_parse(one_child, edge_limits);

        if (a.error.code == 201 && a.error.category == 2 &&
            b.error.code == 202 && b.error.offset == 1 &&
            c.error.code == 203 && c.error.offset == 0 &&
            d.error.code == 204 && d.error.offset == 2 &&
            e.error.code == 205 && e.error.offset == 1) 42 else 0 fi
        "#,
        "値 42",
    );
}

#[test]
fn serialize失敗は部分byte列を公開しない() {
    reference_and_vm(
        r#"
        let limits := json_utf8_limits_default();
        var document := json_utf8_document_new();
        let text := "abcdef";
        let root := json_utf8_add_string(document, text, limits) ?? -1;
        var small := json_utf8_limits_default();
        small.max_bytes := 4;
        let too_large := json_utf8_serialize(document, root, small);

        var malformed := document;
        malformed.child_counts[root] := 1;
        let invalid := json_utf8_serialize(malformed, root, limits);
        if (! too_large.ok && too_large.bytes.len() == 0 &&
            too_large.error.category == 3 && too_large.error.code == 301 &&
            ! invalid.ok && invalid.bytes.len() == 0 && invalid.error.code == 300) 42 else 0 fi
        "#,
        "値 42",
    );
}
