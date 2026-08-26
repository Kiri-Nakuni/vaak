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
        Value::array(
            ValueType::U8,
            self.0.iter().map(|b| Value::U8(*b)).collect(),
        )
    }
    fn write(&mut self, v: &Value) {
        if let Value::Array(ar) = v {
            self.0 = ar
                .items
                .iter()
                .filter_map(|x| x.as_int())
                .map(|x| x as u8)
                .collect();
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
    assert!(
        matches!(out, Outcome::Empty | Outcome::Paradox { .. }),
        "{out:?}"
    );
    match h.get("buf").map(|b| b.read()) {
        Some(Value::Array(ar)) => {
            assert_eq!(
                ar.items
                    .iter()
                    .filter_map(|x| x.as_int())
                    .collect::<Vec<_>>(),
                vec![0, 0, 0]
            );
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
    assert!(matches!(
        h.run("var x : i64 alias &= n; x"),
        Outcome::Static(_)
    ));
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
    assert!(matches!(
        h.run("var x := 1;"),
        Outcome::Paradox { .. } | Outcome::Empty
    ));
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

#[test]
fn c2_実行時エラーまでの書き換えは巻き戻さない() {
    for use_vm in [false, true] {
        let mut h = Host::new();
        h.use_vm = use_vm;
        h.expose_value("n", Value::I64(1));

        let out = h.run(
            "n := 42;
             fn f (a : i64) { a } -> i64;
             f(1 / 0);",
        );
        assert!(
            matches!(out, Outcome::Runtime { .. }),
            "use_vm={use_vm}: {out:?}"
        );
        match h.get("n").map(|b| b.read()) {
            Some(Value::I64(v)) => assert_eq!(v, 42, "use_vm={use_vm}"),
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
        assert!(
            matches!(run96("count[1]", vm), Outcome::Value(_)),
            "vm={vm}"
        );
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

// ── 契約：同じなら書き戻さない ─────────────────────────

/// 書き戻しを数える束縛。**ホストによっては書きが高い**——
/// rtex なら save stack が動く。
struct Counted {
    v: i64,
    writes: std::rc::Rc<std::cell::Cell<u32>>,
}

impl vaak::host::HostBinding for Counted {
    fn type_of(&self) -> vaak::ast::ValueType {
        vaak::ast::ValueType::I64
    }
    fn read(&self) -> vaak::value::Value {
        vaak::value::Value::I64(self.v)
    }
    fn write(&mut self, v: &vaak::value::Value) {
        self.writes.set(self.writes.get() + 1);
        if let Some(x) = v.as_int() {
            self.v = x as i64;
        }
    }
}

fn 書き戻しの回数(src: &str, use_vm: bool) -> u32 {
    let n = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut h = vaak::host::Host::new();
    h.use_vm = use_vm;
    h.expose(
        "a",
        Box::new(Counted {
            v: 7,
            writes: n.clone(),
        }),
    );
    h.expose(
        "b",
        Box::new(Counted {
            v: 9,
            writes: n.clone(),
        }),
    );
    let _ = h.run(src);
    n.get()
}

#[test]
fn 触らなければ書き戻さない() {
    for vm in [false, true] {
        assert_eq!(書き戻しの回数("a + b", vm), 0, "vm={vm}");
    }
}

#[test]
fn 同じ値を入れ直しても書き戻さない() {
    for vm in [false, true] {
        assert_eq!(書き戻しの回数("a := 7; b := 9; 0", vm), 0, "vm={vm}");
    }
}

#[test]
fn 変わったものだけ書き戻す() {
    for vm in [false, true] {
        assert_eq!(書き戻しの回数("a := 8; 0", vm), 1, "vm={vm}");
        assert_eq!(書き戻しの回数("a := 8; b := 10; 0", vm), 2, "vm={vm}");
    }
}

#[test]
fn 誤りでも途中までの書き換えを拾える() {
    // **C-2：実行時の誤りより前の書き換えは巻き戻さない。**
    // だから誤りのときの後の状態にも意味がある
    let prog = vaak::parser::parse("a := 42; b := a / 0; 0").expect("構文");
    let exposed = vec![
        (
            "a".to_string(),
            vaak::ast::HostItem::Value(vaak::ast::ValueType::I64),
        ),
        (
            "b".to_string(),
            vaak::ast::HostItem::Value(vaak::ast::ValueType::I64),
        ),
    ];
    let p = vaak::vm::compile_with_host(&prog, &exposed).expect("組み立て");
    let mut r = vaak::vm::Runner::new();
    let (result, after) = r.run_writeback(
        &p,
        vec![vaak::value::Value::I64(7), vaak::value::Value::I64(9)],
    );
    assert!(result.is_err(), "0 除算の paradox が消費されずに誤りになる");
    // **`a := 42` は誤りより前なので残っている**
    assert_eq!(after[0].as_int(), Some(42));
}

/// 読みを数える束縛。
struct CountedRead {
    v: i64,
    reads: std::rc::Rc<std::cell::Cell<u32>>,
}

impl vaak::host::HostBinding for CountedRead {
    fn type_of(&self) -> vaak::ast::ValueType {
        vaak::ast::ValueType::I64
    }
    fn read(&self) -> vaak::value::Value {
        self.reads.set(self.reads.get() + 1);
        vaak::value::Value::I64(self.v)
    }
    fn write(&mut self, v: &vaak::value::Value) {
        if let Some(x) = v.as_int() {
            self.v = x as i64;
        }
    }
}

fn 読みの回数(src: &str) -> u32 {
    let n = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut h = vaak::host::Host::new();
    h.use_vm = true;
    for name in ["a", "b", "c"] {
        h.expose(
            name,
            Box::new(CountedRead {
                v: 1,
                reads: n.clone(),
            }),
        );
    }
    let _ = h.run(src);
    n.get()
}

#[test]
fn 使わない名前は読まない() {
    // **束縛は三つあるが、触っているのは一つ**
    assert_eq!(読みの回数("a + 1"), 1);
    assert_eq!(読みの回数("a + b"), 2);
    assert_eq!(読みの回数("a + b + c"), 3);
    // 一つも触らなければ一度も読まない
    assert_eq!(読みの回数("1 + 1"), 0);
}

#[test]
fn 書くだけの名前も読む() {
    // **`:=` は丸ごと置き換えるので読まなくてよさそうだが、
    // 迷ったら読む側に倒してある**——読み過ぎは遅いだけ、読み落としは間違い
    assert!(読みの回数("a := 5; 0") <= 1);
}

// ── S-15：要素だけを問う ─────────────────────────

/// 256 個のレジスタを見せるが、**一つずつ答えられる**束縛。
struct Regs {
    v: Vec<i64>,
    whole_reads: std::rc::Rc<std::cell::Cell<u32>>,
    element_reads: std::rc::Rc<std::cell::Cell<u32>>,
    element_writes: std::rc::Rc<std::cell::Cell<u32>>,
}

impl vaak::host::HostBinding for Regs {
    fn type_of(&self) -> vaak::ast::ValueType {
        vaak::ast::ValueType::Array(Box::new(vaak::ast::ValueType::I64))
    }
    fn read(&self) -> vaak::value::Value {
        self.whole_reads.set(self.whole_reads.get() + 1);
        vaak::value::Value::array(
            vaak::ast::ValueType::I64,
            self.v.iter().map(|x| vaak::value::Value::I64(*x)).collect(),
        )
    }
    fn write(&mut self, _v: &vaak::value::Value) {}
    fn read_at(&self, i: usize) -> Option<vaak::value::Value> {
        self.element_reads.set(self.element_reads.get() + 1);
        self.v.get(i).map(|x| vaak::value::Value::I64(*x))
    }
    fn write_at(&mut self, i: usize, v: &vaak::value::Value) -> bool {
        self.element_writes.set(self.element_writes.get() + 1);
        match (self.v.get_mut(i), v.as_int()) {
            (Some(slot), Some(x)) => {
                *slot = x as i64;
                true
            }
            _ => false,
        }
    }
    fn supports_partial_writeback(&self) -> bool {
        true
    }
    fn len(&self) -> Option<usize> {
        Some(self.v.len())
    }
}

fn 要素で数える(src: &str) -> (u32, u32, u32) {
    let w = std::rc::Rc::new(std::cell::Cell::new(0));
    let r = std::rc::Rc::new(std::cell::Cell::new(0));
    let x = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut h = vaak::host::Host::new();
    h.use_vm = true;
    h.expose(
        "count",
        Box::new(Regs {
            v: (0..256).collect(),
            whole_reads: w.clone(),
            element_reads: r.clone(),
            element_writes: x.clone(),
        }),
    );
    let _ = h.run(src);
    (w.get(), r.get(), x.get())
}

#[test]
fn 定数の添字なら丸ごと写さない() {
    // **触ったのは 5 番だけ。** 256 個を読む必要は無い
    let (whole, elems, _) = 要素で数える("count[5] + 1");
    assert_eq!(whole, 0, "丸ごと読んではいけない");
    assert_eq!(elems, 1, "触った一つだけ");
}

#[test]
fn 複数の定数の添字() {
    let (whole, elems, _) = 要素で数える("count[5] + count[200] + count[5]");
    assert_eq!(whole, 0);
    assert_eq!(elems, 2, "同じ添字は一度だけ");
}

#[test]
fn 定数の添字へ書くのも要素だけ() {
    let (whole, _, writes) = 要素で数える("count[7] := 99; 0");
    assert_eq!(whole, 0);
    assert_eq!(writes, 1, "触った一つだけ書き戻す");
}

#[test]
fn 定数の添字への複合代入も要素だけ() {
    let (whole, reads, writes) = 要素で数える("count[7] += 1; 0");
    assert_eq!(whole, 0, "丸ごと読んではいけない");
    assert_eq!(reads, 1, "現在値も触った一つから読む");
    assert_eq!(writes, 1, "触った一つだけ書き戻す");
}

#[test]
fn 書いた値が本当に届く() {
    let w = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut h = vaak::host::Host::new();
    h.use_vm = true;
    h.expose(
        "count",
        Box::new(Regs {
            v: vec![0; 16],
            whole_reads: w.clone(),
            element_reads: w.clone(),
            element_writes: w.clone(),
        }),
    );
    let _ = h.run("count[3] := 42; 0");
    let after = h.get("count").unwrap().read();
    let vaak::value::Value::Array(a) = after else {
        panic!("配列のはず")
    };
    assert_eq!(a.items[3].as_int(), Some(42));
    // **触っていない要素は零のままで、潰されていない**
    assert_eq!(a.items[4].as_int(), Some(0));
}

#[test]
fn 動く添字なら丸ごと読む() {
    // 組み立て時に添字が分からないなら、要素だけでは足りない
    let (whole, elems, _) = 要素で数える("var i := 3; count[i] + 1");
    assert_eq!(whole, 1, "丸ごと読む");
    assert_eq!(elems, 0);
}

#[test]
fn 長さを見るなら本物の長さが要る() {
    let mut h = vaak::host::Host::new();
    h.use_vm = true;
    let z = std::rc::Rc::new(std::cell::Cell::new(0));
    h.expose(
        "count",
        Box::new(Regs {
            v: (0..256).collect(),
            whole_reads: z.clone(),
            element_reads: z.clone(),
            element_writes: z.clone(),
        }),
    );
    assert!(
        matches!(h.run("count.len()"), vaak::host::Outcome::Value(v) if v.as_int() == Some(256))
    );
}

// ── S-15：片側だけの部分accessは書き込みへ使わない ─────────────

struct ReadAtOnly {
    values: std::rc::Rc<std::cell::RefCell<Vec<i64>>>,
    whole_reads: std::rc::Rc<std::cell::Cell<u32>>,
    element_reads: std::rc::Rc<std::cell::Cell<u32>>,
    whole_writes: std::rc::Rc<std::cell::Cell<u32>>,
}

impl vaak::host::HostBinding for ReadAtOnly {
    fn type_of(&self) -> vaak::ast::ValueType {
        vaak::ast::ValueType::Array(Box::new(vaak::ast::ValueType::I64))
    }

    fn read(&self) -> vaak::value::Value {
        self.whole_reads.set(self.whole_reads.get() + 1);
        vaak::value::Value::array(
            vaak::ast::ValueType::I64,
            self.values
                .borrow()
                .iter()
                .copied()
                .map(vaak::value::Value::I64)
                .collect(),
        )
    }

    fn write(&mut self, value: &vaak::value::Value) {
        self.whole_writes.set(self.whole_writes.get() + 1);
        let vaak::value::Value::Array(array) = value else {
            return;
        };
        *self.values.borrow_mut() = array
            .items
            .iter()
            .map(|value| value.as_int().unwrap_or(0) as i64)
            .collect();
    }

    fn read_at(&self, index: usize) -> Option<vaak::value::Value> {
        self.element_reads.set(self.element_reads.get() + 1);
        self.values
            .borrow()
            .get(index)
            .copied()
            .map(vaak::value::Value::I64)
    }

    fn len(&self) -> Option<usize> {
        Some(self.values.borrow().len())
    }
}

fn read_at_only_host() -> (
    vaak::host::Host,
    std::rc::Rc<std::cell::RefCell<Vec<i64>>>,
    std::rc::Rc<std::cell::Cell<u32>>,
    std::rc::Rc<std::cell::Cell<u32>>,
    std::rc::Rc<std::cell::Cell<u32>>,
) {
    let values = std::rc::Rc::new(std::cell::RefCell::new(vec![0, 1, 2, 3]));
    let whole_reads = std::rc::Rc::new(std::cell::Cell::new(0));
    let element_reads = std::rc::Rc::new(std::cell::Cell::new(0));
    let whole_writes = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut host = vaak::host::Host::new();
    host.use_vm = true;
    host.expose(
        "count",
        Box::new(ReadAtOnly {
            values: values.clone(),
            whole_reads: whole_reads.clone(),
            element_reads: element_reads.clone(),
            whole_writes: whole_writes.clone(),
        }),
    );
    (host, values, whole_reads, element_reads, whole_writes)
}

#[test]
fn read_atだけでも読むだけなら一要素に絞れる() {
    let (mut host, _, whole_reads, element_reads, whole_writes) = read_at_only_host();
    assert!(matches!(host.run("count[2]"), Outcome::Value(v) if v.as_int() == Some(2)));
    assert_eq!(whole_reads.get(), 0);
    assert_eq!(element_reads.get(), 1);
    assert_eq!(whole_writes.get(), 0);
}

#[test]
fn read_atだけの束縛へ書くなら丸ごと経路へ戻す() {
    let (mut host, values, whole_reads, element_reads, whole_writes) = read_at_only_host();
    let _ = host.run("count[2] += 10; 0");
    assert_eq!(&*values.borrow(), &[0, 1, 12, 3]);
    assert_eq!(whole_reads.get(), 1);
    assert_eq!(element_reads.get(), 0);
    assert_eq!(whole_writes.get(), 1);
}

fn host_touched_for(source: &str) -> Option<Vec<i128>> {
    let syntax = vaak::parser::parse(source).expect("構文");
    let layout = vec![(
        "count".to_string(),
        vaak::ast::HostItem::Value(vaak::ast::ValueType::Array(Box::new(
            vaak::ast::ValueType::I64,
        ))),
    )];
    vaak::vm::compile_with_host(&syntax, &layout)
        .expect("VM組み立て")
        .host_touched(0)
}

#[test]
fn alias引数はhost値を丸ごと要求する() {
    assert_eq!(
        host_touched_for("fn f (var c : i64 array alias) { c[2] += 1; }; f(count); 0"),
        None
    );
}

#[test]
fn alias経由の変更も実行時errorで丸ごと書き戻す() {
    let (mut host, values, whole_reads, element_reads, whole_writes) = read_at_only_host();
    let outcome =
        host.run("fn f (var c : i64 array alias) { c[2] += 10; }; f(count); var x := 1 / 0; x");
    assert!(matches!(outcome, Outcome::Runtime { .. }), "{outcome:?}");
    assert_eq!(&*values.borrow(), &[0, 1, 12, 3]);
    assert_eq!(whole_reads.get(), 1);
    assert_eq!(element_reads.get(), 0);
    assert_eq!(whole_writes.get(), 1);
}

#[test]
fn 破壊的methodはhost値を丸ごと要求する() {
    assert_eq!(host_touched_for("count.push(9); 0"), None);
}

#[test]
fn 破壊的method後の実行時errorでも丸ごと書き戻す() {
    let (mut host, values, whole_reads, element_reads, whole_writes) = read_at_only_host();
    let outcome = host.run("count.push(9); var x := 1 / 0; x");
    assert!(matches!(outcome, Outcome::Runtime { .. }), "{outcome:?}");
    assert_eq!(&*values.borrow(), &[0, 1, 2, 3, 9]);
    assert_eq!(whole_reads.get(), 1);
    assert_eq!(element_reads.get(), 0);
    assert_eq!(whole_writes.get(), 1);
}
