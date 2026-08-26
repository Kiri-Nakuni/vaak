//! pure Vaak 文字列ライブラリの参照実装・VM・STEEL差分試験。

use std::process::Command;
use vaak::interp::Eval;

fn source(body: &str) -> String {
    format!("{}\n{body}", vaak::stdlib::STRING)
}

#[track_caller]
fn checked(body: &str) -> String {
    let src = source(body);
    let prog = vaak::parser::parse(&src).expect("文字列ライブラリを含むソースを解析できる");
    let errors: Vec<_> = vaak::check::check(&prog)
        .into_iter()
        .chain(vaak::types::check_types(&prog))
        .map(|e| e.msg)
        .collect();
    assert!(errors.is_empty(), "静的検査: {errors:?}\n{body}");
    src
}

fn shape(result: Result<Eval, String>) -> String {
    match result {
        Ok(Eval::Value(v)) => format!("値 {}", v.show()),
        Ok(Eval::Paradox(_)) => "paradox".into(),
        Ok(Eval::Akasha) => "虚無".into(),
        Ok(Eval::Escape(_)) => "脱出".into(),
        Err(e) => format!("エラー {e}"),
    }
}

#[track_caller]
fn both(body: &str, expected: &str) {
    let src = checked(body);
    for (name, result) in [
        ("参照", vaak::interp::run(&src)),
        ("VM", vaak::vm::run(&src)),
    ] {
        assert_eq!(shape(result), expected, "{name}: {body}");
    }
}

#[test]
fn rustからライブラリソースを露出する() {
    assert!(vaak::stdlib::STRING.contains("fn str_find_from"));
    checked("let source := \"abc\"; str_contains(source, \"b\")");
}

#[test]
fn 前方後方一致と包含は空needleも定義する() {
    both(
        r#"let source := "abcdef";
           if (str_eq(source, "abcdef") &&
               str_starts_with(source, "abc") &&
               str_ends_with(source, "def") &&
               str_contains(source, "cd") &&
               str_starts_with(source, "") &&
               str_ends_with(source, "") &&
               str_contains(source, "")) 42 else 0 fi"#,
        "値 42",
    );
    // needleは値引数なので、同じセルをsourceとneedleの両方へ渡せる。
    both(
        r#"let source := "same";
           if ((str_find(source, source) ?? -1) == 0 && str_eq(source, source)) 42 else 0 fi"#,
        "値 42",
    );
}

#[test]
fn 検索位置はバイト添字で返す() {
    both(
        r#"let source := "ababa";
           let a := str_find_from(source, "ba", 2) ?? -10;
           let b := str_rfind(source, "aba") ?? -10;
           let c := str_rfind_from(source, "aba", 1) ?? -10;
           let d := str_find_from(source, "", 3) ?? -10;
           a * 1000 + b * 100 + c * 10 + d"#,
        "値 3203",
    );
    both(r#"let source := "abc"; str_find(source, "z")"#, "paradox");
    both(
        r#"let source := "abc"; str_find_from(source, "a", -1)"#,
        "paradox",
    );
    both(
        r#"let source := "abc"; str_find_from(source, "", 4)"#,
        "paradox",
    );
}

#[test]
fn 一バイト検索はneedle文字列を作らない() {
    both(
        r#"let source := "a,b,c";
           let first := str_find_byte(source, 44, 0) ?? -10;
           let second := str_find_byte(source, 44, 2) ?? -10;
           first * 10 + second"#,
        "値 13",
    );
    both(
        r#"let source := "abc"; str_find_byte(source, 44, 0)"#,
        "paradox",
    );
    both(
        r#"let source := "abc"; str_find_byte(source, 97, 4)"#,
        "paradox",
    );
}

#[test]
fn sliceは半開区間で範囲違反を畳む() {
    both(
        r#"let source := "abcdef"; let part := str_slice(source, 1, 4) ?? "";
           if (str_eq(part, "bcd")) 42 else 0 fi"#,
        "値 42",
    );
    both(
        r#"let source := "abc"; str_slice(source, -1, 2)"#,
        "paradox",
    );
    both(r#"let source := "abc"; str_slice(source, 2, 1)"#, "paradox");
    both(r#"let source := "abc"; str_slice(source, 0, 4)"#, "paradox");
}

#[test]
fn ascii空白だけを両端から落とす() {
    both(
        r#"let source := "  あ  ";
           let trimmed := str_trim_ascii(source) ?? "";
           if (str_eq(trimmed, "あ")) 42 else 0 fi"#,
        "値 42",
    );
}

#[test]
fn splitは空欄と末尾の空欄を保持しjoinで戻せる() {
    both(
        r#"let source := "a,,b,";
           let parts := str_split(source, ",") ?? new str array(0, "");
           let joined := str_join(parts, "|") ?? "";
           if (parts.len() == 4 && str_eq(joined, "a||b|")) 42 else 0 fi"#,
        "値 42",
    );
    both(r#"let source := "abc"; str_split(source, "")"#, "paradox");
}

#[test]
fn 置換は左から右へ重ならずに進む() {
    both(
        r#"let source := "aaaa";
           let replaced := str_replace_all(source, "aa", "b") ?? "";
           if (str_eq(replaced, "bb")) 42 else 0 fi"#,
        "値 42",
    );
    both(
        r#"let source := "abc"; str_replace_all(source, "", "x")"#,
        "paradox",
    );
}

#[test]
fn 繰返しは負回数と出力長のoverflowを畳む() {
    both(
        r#"let source := "ab"; let repeated := str_repeat(source, 3) ?? "";
           if (str_eq(repeated, "ababab")) 42 else 0 fi"#,
        "値 42",
    );
    both(r#"let source := "x"; str_repeat(source, -1)"#, "paradox");
    both(
        r#"let source := "";
           let repeated := str_repeat(source, 9223372036854775807) ?? "x";
           repeated.len()"#,
        "値 0",
    );
    both(
        r#"let source := "ab"; str_repeat(source, 9223372036854775807)"#,
        "paradox",
    );
}

#[test]
fn ascii大小変換は非asciiバイトを変えない() {
    both(
        r#"let source := "AbCあ";
           let lower := str_ascii_lowercase(source) ?? "";
           let upper := str_ascii_uppercase(source) ?? "";
           if (str_eq(lower, "abcあ") && str_eq(upper, "ABCあ") &&
               str_eq_ignore_ascii_case(source, "aBcあ")) 42 else 0 fi"#,
        "値 42",
    );
}

#[test]
fn utf8の過長符号化サロゲート範囲上限超過を拒む() {
    both(
        r#"let valid_source := "あ😀";
           var overlong_bytes : u8 array := [192, 175];
           var surrogate_bytes : u8 array := [237, 160, 128];
           var too_large_bytes : u8 array := [244, 144, 128, 128];
           let overlong := new str(overlong_bytes);
           let surrogate := new str(surrogate_bytes);
           let too_large := new str(too_large_bytes);
           if (str_utf8_valid(valid_source) &&
               ! str_utf8_valid(overlong) &&
               ! str_utf8_valid(surrogate) &&
               ! str_utf8_valid(too_large)) 42 else 0 fi"#,
        "値 42",
    );
}

#[test]
fn utf8_sliceは符号位置の途中を拒む() {
    both(
        r#"let source := "AあB";
           let whole_codepoint := str_slice_utf8(source, 1, 4) ?? "";
           if (str_eq(whole_codepoint, "あ") &&
               ! str_is_utf8_boundary(source, 2)) 42 else 0 fi"#,
        "値 42",
    );
    both(
        r#"let source := "AあB"; str_slice_utf8(source, 2, 4)"#,
        "paradox",
    );
    both(
        r#"let source := "あ"; let raw := str_slice(source, 1, 3) ?? "";
           if (! str_utf8_valid(raw)) 42 else 0 fi"#,
        "値 42",
    );
}

#[test]
fn 利用例も参照実装とvmで動く() {
    let example = include_str!("../stdlib/examples/文字列.vaak");
    both(example, "値 42");
}

#[test]
fn steelでもpure_vaak実装をnative実行できる() {
    let body = r#"
        let source := "  Ababaあ  ";
        let trimmed := str_trim_ascii(source) ?? "";
        let replaced := str_replace_all(trimmed, "ba", "X") ?? "";
        let upper := str_ascii_uppercase(replaced) ?? "";
        let pieces := str_split(upper, "X") ?? new str array(0, "");
        let joined := str_join(pieces, "-") ?? "";
        let hyphen := str_find_byte(joined, 45, 0) ?? -1;
        if (str_utf8_valid(joined) && str_eq(joined, "A--あ") && hyphen == 1) 42 else 0 fi
    "#;
    let src = checked(body);
    let prog = vaak::parser::parse(&src).expect("構文");
    let ir = vaak::steel::compile(&prog).unwrap_or_else(|e| panic!("STEEL: {}", e.msg));

    if Command::new("clang").arg("--version").output().is_err() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("vaak-string-library-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("一時ディレクトリ");
    let ll = dir.join("string.ll");
    let exe = dir.join("string.exe");
    let pdb = dir.join("string.pdb");
    std::fs::write(&ll, ir).expect("LLVM IR");
    let built = Command::new("clang")
        .arg("-O2")
        .arg("-o")
        .arg(&exe)
        .arg(&ll)
        .output()
        .expect("clang");
    if !built.status.success() {
        let message = String::from_utf8_lossy(&built.stderr).into_owned();
        let _ = std::fs::remove_file(&ll);
        let _ = std::fs::remove_file(&exe);
        let _ = std::fs::remove_file(&pdb);
        let _ = std::fs::remove_dir(&dir);
        panic!("clang: {message}");
    }
    let status = Command::new(&exe).status().expect("native実行");
    let _ = std::fs::remove_file(&ll);
    let _ = std::fs::remove_file(&exe);
    let _ = std::fs::remove_file(&pdb);
    let _ = std::fs::remove_dir(&dir);
    assert_eq!(status.code(), Some(42));
}
