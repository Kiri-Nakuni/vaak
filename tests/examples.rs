//! 例が動くことを確かめる。
//!
//! **動かない例を置かない。** 言語が変われば、ここで落ちる。

fn run(path: &str) -> i128 {
    let src = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let prog = vaak::parser::parse(&src).unwrap_or_else(|e| panic!("{path}: 構文: {}", e.msg));
    let errs: Vec<String> = vaak::check::check(&prog)
        .into_iter()
        .chain(vaak::types::check_types(&prog))
        .map(|e| e.msg)
        .collect();
    assert!(errs.is_empty(), "{path}: {errs:#?}");
    // **木を辿る実装が参照実装である**（S-5）。VM とも突き合わせる
    let mut it = vaak::interp::Interp::new();
    let a = match it.run(&prog) {
        Ok(vaak::interp::Eval::Value(v)) => v.as_int().expect("整数のはず"),
        Ok(other) => panic!("{path}: {other:?}"),
        Err(e) => panic!("{path}: 実行時: {} @{:?}", e.msg, e.span),
    };
    // **VM とも突き合わせる。**
    {
        let p2 = vaak::vm::compile(&prog).unwrap_or_else(|e| panic!("{path}: {}", e.msg));
        let b = match vaak::vm::run_program(&p2) {
            Ok(vaak::interp::Eval::Value(v)) => v.as_int().expect("整数のはず"),
            other => panic!("{path}: VM: {other:?}"),
        };
        assert_eq!(a, b, "{path}: 木を辿る実装と VM が食い違った");
    }
    a
}

#[test]
fn 探索() {
    assert_eq!(run("examples/vaak/01-探索.vaak"), 4499);
}

#[test]
fn 篩() {
    assert_eq!(run("examples/vaak/02-篩.vaak"), 25);
}

#[test]
fn 語数え() {
    assert_eq!(run("examples/vaak/03-語数え.vaak"), 308);
}

#[test]
fn 逆ポーランド() {
    assert_eq!(run("examples/vaak/04-逆ポーランド.vaak"), 13889);
}

#[test]
fn ホストを操る() {
    // **ホストが `count` を見せていなければ動かない**（C-96）
    use vaak::ast::ValueType;
    use vaak::host::{Host, Outcome};
    use vaak::value::Value;
    let src = std::fs::read_to_string("examples/vaak/05-ホストを操る.vaak").unwrap();
    let regs = Value::array(
        ValueType::I32,
        (0..256usize)
            .map(|i| Value::I32(if (1..=3).contains(&i) { [19, 28, 37][i - 1] } else { 0 }))
            .collect(),
    );
    for vm in [false, true] {
        let mut h = Host::new();
        h.use_vm = vm;
        h.expose_value("count", regs.clone());
        match h.run(&src) {
            Outcome::Value(v) => assert_eq!(v.as_int(), Some(30), "vm={vm}"),
            other => panic!("vm={vm}: {other:?}"),
        }
    }
}


/// S-16 の食い違い。**直ったらこの `ignore` を外す。**
#[test]
#[ignore = "S-16: VM が `??` と動的な段数の脱出の組み合わせで落ちる"]
fn s16_vmの食い違い() {
    let src = "flow $return = $repeat(break, getdepth());
fn search (a : i64 array alias, x : i64) {
    var lo := 0;
    var hi := a.len() - 1;
    while (lo <= hi) {
        let mid := (lo + hi) / 2;
        if (a[mid] == x) $return mid;
        elif (a[mid] < x) lo := mid + 1;
        else hi := mid - 1;
        fi;
    };
} -> i64;
let xs := [ 2, 3, 5, 7, 11 ];
var f := 0; nfor (i, 0, xs.len()) { f += search(xs, xs[i]) ?? 0 - 1; }; f";
    let prog = vaak::parser::parse(src).unwrap();
    let a = match vaak::interp::Interp::new().run(&prog) {
        Ok(vaak::interp::Eval::Value(v)) => v.as_int().unwrap(),
        other => panic!("参照実装: {other:?}"),
    };
    let p2 = vaak::vm::compile(&prog).unwrap();
    let b = match vaak::vm::run_program(&p2) {
        Ok(vaak::interp::Eval::Value(v)) => v.as_int().unwrap(),
        other => panic!("VM: {other:?}"),
    };
    assert_eq!(a, b);
}
