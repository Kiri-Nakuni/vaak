//! ホスト界面（S-4）の契約。

use vaak::ast::ValueType;
use vaak::host::{Host, HostBinding, Outcome};
use vaak::value::Value;

/// テスト用：`u8 array` を見せる。
struct Bytes(Vec<u8>);
impl HostBinding for Bytes {
    fn type_of(&self) -> ValueType {
        ValueType::Array(Box::new(ValueType::U8))
    }
    fn read(&self) -> Value {
        Value::array(ValueType::U8, self.0.iter().map(|b| Value::U8(*b)).collect())
    }
    fn write(&mut self, v: &Value) {
        if let Value::Array(ar) = v {
            self.0 = ar.items.iter().filter_map(|x| x.as_int()).map(|x| x as u8).collect();
        }
    }
}

#[test]
fn s4_ホストのセルに張れる() {
    let mut h = Host::new();
    h.expose("buf", Box::new(Bytes(vec![1, 2, 3])));
    let out = h.run("var b : u8 array alias &= buf; b.len()");
    match out {
        Outcome::Value(v) => assert_eq!(v.show(), "3"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn s4_書き戻される() {
    let mut h = Host::new();
    h.expose_value("n", Value::I64(1));
    h.run("var x : i64 alias &= n; x := 42;");
    match h.get("n").map(|b| b.read()) {
        Some(Value::I64(v)) => assert_eq!(v, 42),
        other => panic!("{other:?}"),
    }
}

#[test]
fn s4_配列を書き換えられる() {
    let mut h = Host::new();
    h.expose("buf", Box::new(Bytes(vec![9, 9, 9])));
    let out = h.run(
        "var b : u8 array alias &= buf;
         nfor (i, 0, b.len()) { b[i] := 0; };",
    );
    assert!(matches!(out, Outcome::Empty | Outcome::Paradox { .. }), "{out:?}");
    match h.get("buf").map(|b| b.read()) {
        Some(Value::Array(ar)) => {
            assert_eq!(ar.items.iter().filter_map(|x| x.as_int()).collect::<Vec<_>>(), vec![0, 0, 0]);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn s4_剥がされたセルは見えない() {
    let mut h = Host::new();
    h.expose_value("n", Value::I64(1));
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

/// **VM でも同じ結果になる。** ホストの名前を受け取れるようになった。
#[test]
fn s4_vm_でもホストの名前を受け取る() {
    for use_vm in [false, true] {
        let mut h = Host::new();
        h.use_vm = use_vm;
        h.expose_value("n", Value::I64(1));
        let out = h.run("n := n + 41; n");
        match out {
            Outcome::Value(v) => assert_eq!(v.show(), "42", "use_vm={use_vm}"),
            other => panic!("use_vm={use_vm}: {other:?}"),
        }
        match h.get("n").map(|b| b.read()) {
            Some(Value::I64(v)) => assert_eq!(v, 42, "use_vm={use_vm}"),
            other => panic!("use_vm={use_vm}: {other:?}"),
        }
    }
}

#[test]
fn s4_vm_でも配列を書き換えられる() {
    for use_vm in [false, true] {
        let mut h = Host::new();
        h.use_vm = use_vm;
        h.expose_value(
            "count",
            Value::array(ValueType::I32, vec![Value::I32(0); 4]),
        );
        h.run("count[2] := 7;");
        match h.get("count").map(|b| b.read()) {
            Some(Value::Array(ar)) => {
                assert_eq!(ar.items[2].as_int(), Some(7), "use_vm={use_vm}")
            }
            other => panic!("use_vm={use_vm}: {other:?}"),
        }
    }
}

// ===== C-96：ホストの名前は関数の中からは見えない =====

fn regs96(v: &[i32]) -> Value {
    Value::array(ValueType::I32, v.iter().map(|x| Value::I32(*x)).collect())
}

fn run96(src: &str, vm: bool) -> Outcome {
    let mut h = Host::new();
    h.use_vm = vm;
    h.expose_value("count", regs96(&[0, 10, 20, 30]));
    h.run(src)
}

#[test]
fn c96_最上位からは見える() {
    for vm in [false, true] {
        assert!(matches!(run96("count[1]", vm), Outcome::Value(_)), "vm={vm}");
    }
}

#[test]
fn c96_関数の中からは見えない() {
    for vm in [false, true] {
        match run96("fn f () { count[1] } -> i32; f()", vm) {
            // **なぜ見えないかを言う。** 「知らない」で済ませると綴りを疑わせる
            Outcome::Static(e) => assert!(e[0].contains("C-96"), "{e:?}"),
            other => panic!("vm={vm}: {other:?}"),
        }
    }
}

#[test]
fn c96_引数で受ければ触れる() {
    for vm in [false, true] {
        match run96("fn f (c : i32 array alias) { c[1] } -> i32; f(count)", vm) {
            Outcome::Value(v) => assert_eq!(v.as_int(), Some(10)),
            other => panic!("vm={vm}: {other:?}"),
        }
    }
}

#[test]
fn c96_同じセルに二つ届かない() {
    // **これが理由である**（C-87）。周囲から見えたら、この検査を素通りできてしまう
    let out = run96(
        "fn f (a : i32 array alias, b : i32 array alias) { a[0] } -> i32; f(count, count)",
        false,
    );
    assert!(matches!(out, Outcome::Static(_)), "{out:?}");
}
