//! ホストが**呼べる名前**も見せられる（S-11）。
//!
//! コールバックではない。**閉包が Vaak では表現できない**からである——
//! 参照で捕まえれば C-48（値の中に別名は入らない）に、
//! 写しで捕まえれば C-33（値は深く複製される）に掛かる。

use std::cell::RefCell;
use std::rc::Rc;
use vaak::ast::{HostSig, ValueType};
use vaak::host::{Host, HostFn, Outcome};
use vaak::value::Value;

/// 呼ばれた記録を残すだけの名前。
struct Recorder {
    seen: Rc<RefCell<Vec<i128>>>,
}

impl HostFn for Recorder {
    fn sig(&self) -> HostSig {
        HostSig { params: vec![ValueType::I64], ret: None }
    }
    fn call(&mut self, args: &[Value]) -> Option<Value> {
        self.seen.borrow_mut().push(args[0].as_int().unwrap_or(0));
        None
    }
}

/// 二つ足して返す名前。
struct Add2;

impl HostFn for Add2 {
    fn sig(&self) -> HostSig {
        HostSig {
            params: vec![ValueType::I64, ValueType::I64],
            ret: Some(ValueType::I64),
        }
    }
    fn call(&mut self, args: &[Value]) -> Option<Value> {
        let a = args[0].as_int()?;
        let b = args[1].as_int()?;
        Some(Value::I64((a + b) as i64))
    }
}

fn run(src: &str, vm: bool) -> (Outcome, Vec<i128>) {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let mut h = Host::new();
    h.use_vm = vm;
    h.expose_fn("note", Box::new(Recorder { seen: seen.clone() }));
    h.expose_fn("add2", Box::new(Add2));
    let out = h.run(src);
    let v = seen.borrow().clone();
    (out, v)
}

#[test]
fn 呼べる名前が呼べる() {
    for vm in [false, true] {
        let (out, _) = run("add2(20, 22)", vm);
        match out {
            Outcome::Value(v) => assert_eq!(v.as_int(), Some(42), "vm={vm}"),
            other => panic!("vm={vm}: {other:?}"),
        }
    }
}

#[test]
fn 返り値が無ければparadoxになる() {
    for vm in [false, true] {
        // **`;` が潰す。** 領域に値は残らない
        let (out, seen) = run("note(7);", vm);
        assert!(matches!(out, Outcome::Empty | Outcome::Paradox { .. }), "vm={vm}: {out:?}");
        assert_eq!(seen, vec![7], "vm={vm}");
    }
}

#[test]
fn 受けなければ落ちる() {
    for vm in [false, true] {
        // `;` を書かないと paradox が領域に残る
        let (out, _) = run("note(1)", vm);
        assert!(matches!(out, Outcome::Paradox { .. }), "vm={vm}: {out:?}");
    }
}

struct 呼ばれない関数;

impl HostFn for 呼ばれない関数 {
    fn sig(&self) -> HostSig {
        HostSig {
            params: Vec::new(),
            ret: None,
        }
    }

    fn call(&mut self, _args: &[Value]) -> Option<Value> {
        panic!("表現範囲を越えた関数は呼ばない")
    }
}

#[test]
fn 参照実装もhost関数indexが折り返す前に拒む() {
    let mut host = Host::new();
    for index in 0..u16::MAX as usize + 2 {
        host.expose_fn(&format!("f{index}"), Box::new(呼ばれない関数));
    }
    assert!(matches!(host.run("0"), Outcome::Static(errors) if errors.iter().any(|error| error.contains("65536"))));
}

#[test]
fn ループの中から何度でも呼べる() {
    for vm in [false, true] {
        let (_, seen) = run("nfor (i, 0, 4) { note(i); };", vm);
        assert_eq!(seen, vec![0, 1, 2, 3], "vm={vm}");
    }
}

#[test]
fn 関数の中からも呼べる() {
    // **値と違って、呼べる名前はスコープ全体で見える**（C-36 / C-96 と対）
    for vm in [false, true] {
        let (out, seen) = run(
            "fn twice (n : i64) { note(n); add2(n, n) } -> i64; twice(21)",
            vm,
        );
        match out {
            Outcome::Value(v) => assert_eq!(v.as_int(), Some(42), "vm={vm}"),
            other => panic!("vm={vm}: {other:?}"),
        }
        assert_eq!(seen, vec![21], "vm={vm}");
    }
}

#[test]
fn 引数の型が合わなければ静的に落ちる() {
    let (out, _) = run("add2(1)", false);
    match out {
        Outcome::Static(e) => assert!(e[0].contains("引数"), "{e:?}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn 知らない名前は静的に落ちる() {
    let (out, _) = run("nosuch(1)", false);
    assert!(matches!(out, Outcome::Static(_)), "{out:?}");
}
