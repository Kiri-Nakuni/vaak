//! ホスト界面（S-4）の契約。

use vaak::host::{Host, HostCell, Outcome};

#[test]
fn s4_ホストのセルに張れる() {
    let mut h = Host::new();
    h.expose("buf", HostCell::U8Array(vec![1, 2, 3]));
    let out = h.run("var b : u8 array alias &= buf; b.len()");
    match out {
        Outcome::Value(v) => assert_eq!(v.show(), "3"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn s4_書き戻される() {
    let mut h = Host::new();
    h.expose("n", HostCell::I64(1));
    h.run("var x : i64 alias &= n; x := 42;");
    match h.get("n") {
        Some(HostCell::I64(v)) => assert_eq!(*v, 42),
        other => panic!("{other:?}"),
    }
}

#[test]
fn s4_配列を書き換えられる() {
    let mut h = Host::new();
    h.expose("buf", HostCell::U8Array(vec![9, 9, 9]));
    let out = h.run(
        "var b : u8 array alias &= buf;
         nfor (i, 0, b.len()) { b[i] := 0; };",
    );
    assert!(matches!(out, Outcome::Empty | Outcome::Paradox { .. }), "{out:?}");
    match h.get("buf") {
        Some(HostCell::U8Array(v)) => assert_eq!(v, &vec![0, 0, 0]),
        other => panic!("{other:?}"),
    }
}

#[test]
fn s4_剥がされたセルは見えない() {
    let mut h = Host::new();
    h.expose("n", HostCell::I64(1));
    h.invalidate("n");
    // 名前ごと消えるので、名前解決で落ちる
    assert!(matches!(h.run("var x : i64 alias &= n; x"), Outcome::Static(_)));
}

#[test]
fn s4_静的エラーは走らせる前に返る() {
    let mut h = Host::new();
    match h.run("loop { 1 }") {
        Outcome::Static(e) => assert!(e.iter().any(|m| m.contains("値が残っている"))),
        other => panic!("{other:?}"),
    }
}

#[test]
fn s4_paradox_は発生点を持つ() {
    let mut h = Host::new();
    // 消費されなかった paradox は実行時エラー。発生点を持つ
    match h.run("fn f (a : i64) { a } -> i64;\nf(1 / 0);") {
        Outcome::Runtime { line, .. } => assert_eq!(line, 2),
        other => panic!("{other:?}"),
    }
}

#[test]
fn s4_最上位の外界面を受け取る() {
    let mut h = Host::new();
    match h.run("1 + 2") {
        Outcome::Value(v) => assert_eq!(v.show(), "3"),
        other => panic!("{other:?}"),
    }
    // 中身が空で終わればホストに委ねる
    assert!(matches!(h.run("var x := 1;"), Outcome::Paradox { .. } | Outcome::Empty));
}
