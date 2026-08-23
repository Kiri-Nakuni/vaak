use std::cell::Cell;
use std::collections::BTreeMap;
use std::rc::Rc;

use vaak::ast::{HostItem, HostSig, ValueType};
use vaak::embedding::{
    prepare, EmbeddingRunError, EmbeddingRunner, HostFunctionSlot, HostFunctionsError, HostLayout,
    HostLayoutError, HostSlot, HostValueKind, HostValuesError, PrepareStage, PreparedRunError,
};
use vaak::host::{HostBinding, HostFn};
use vaak::interp::Eval;
use vaak::value::{HashVal, MapKey, Value};

struct Number {
    value: i64,
    reads: Rc<Cell<u32>>,
    writes: Rc<Cell<u32>>,
}

impl HostBinding for Number {
    fn type_of(&self) -> ValueType {
        ValueType::I64
    }

    fn read(&self) -> Value {
        self.reads.set(self.reads.get() + 1);
        Value::I64(self.value)
    }

    fn write(&mut self, value: &Value) {
        self.writes.set(self.writes.get() + 1);
        self.value = value.as_int().expect("整数") as i64;
    }
}

fn 数値一つの配置() -> HostLayout {
    HostLayout::new(vec![("n".into(), HostItem::Value(ValueType::I64))]).expect("配置")
}

fn 整数の結果(eval: Eval) -> i128 {
    let Eval::Value(value) = eval else {
        panic!("値が要る: {eval:?}");
    };
    value.as_int().expect("整数")
}

#[test]
fn 一度prepareしたprogramとrunnerを繰り返し使える() {
    let layout = 数値一つの配置();
    let program = prepare("n += 1; n", &layout).expect("prepare");
    let reads = Rc::new(Cell::new(0));
    let writes = Rc::new(Cell::new(0));
    let mut number = Number {
        value: 40,
        reads: reads.clone(),
        writes: writes.clone(),
    };
    let mut runner = EmbeddingRunner::new();

    {
        let mut slots = [HostSlot::value("n", &mut number)];
        let eval = runner
            .run_without_functions(&program, &mut slots)
            .expect("一回目");
        assert_eq!(整数の結果(eval), 41);
    }
    {
        let mut slots = [HostSlot::value("n", &mut number)];
        let eval = runner
            .run_without_functions(&program, &mut slots)
            .expect("二回目");
        assert_eq!(整数の結果(eval), 42);
    }

    assert_eq!(number.value, 42);
    assert_eq!(reads.get(), 2);
    assert_eq!(writes.get(), 2);
}

#[test]
fn 実行時layoutが違えば値を読む前に拒む() {
    let layout = 数値一つの配置();
    let program = prepare("n + 1", &layout).expect("prepare");
    let reads = Rc::new(Cell::new(0));
    let writes = Rc::new(Cell::new(0));
    let mut number = Number {
        value: 7,
        reads: reads.clone(),
        writes: writes.clone(),
    };
    let mut slots = [HostSlot::value("other", &mut number)];

    let error = EmbeddingRunner::new()
        .run_without_functions(&program, &mut slots)
        .expect_err("名前が違う");
    assert!(matches!(error, EmbeddingRunError::LayoutMismatch { .. }));
    assert_eq!(reads.get(), 0);
    assert_eq!(writes.get(), 0);
}

struct 読まれない構造体 {
    reads: Rc<Cell<u32>>,
}

impl HostBinding for 読まれない構造体 {
    fn type_of(&self) -> ValueType {
        ValueType::Named("P".into())
    }

    fn read(&self) -> Value {
        self.reads.set(self.reads.get() + 1);
        Value::strukt("P".into(), vec![("x".into(), Value::I64(1))])
    }

    fn write(&mut self, _value: &Value) {}
}

#[test]
fn 使わない構造体host値は読まずに型を保つ() {
    let layout = HostLayout::new(vec![(
        "p".into(),
        HostItem::Value(ValueType::Named("P".into())),
    )])
    .expect("配置");
    let program = prepare("struct P { let x : i64; }; 42", &layout).expect("prepare");
    let reads = Rc::new(Cell::new(0));
    let mut binding = 読まれない構造体 {
        reads: reads.clone(),
    };
    let mut slots = [HostSlot::value("p", &mut binding)];

    let result = EmbeddingRunner::new()
        .run_without_functions(&program, &mut slots)
        .expect("実行");
    assert_eq!(整数の結果(result), 42);
    assert_eq!(reads.get(), 0);
}

struct 構造体配列 {
    reads: Rc<Cell<u32>>,
}

impl HostBinding for 構造体配列 {
    fn type_of(&self) -> ValueType {
        ValueType::Array(Box::new(ValueType::Named("P".into())))
    }

    fn read(&self) -> Value {
        panic!("定数添字なので丸ごと読まない")
    }

    fn write(&mut self, _value: &Value) {}

    fn read_at(&self, index: usize) -> Option<Value> {
        self.reads.set(self.reads.get() + 1);
        Some(Value::strukt(
            "P".into(),
            vec![("x".into(), Value::I64(index as i64 + 40))],
        ))
    }

    fn len(&self) -> Option<usize> {
        Some(4)
    }
}

#[test]
fn 名付き型配列の部分読みはdummy要素を検査しない() {
    let layout = HostLayout::new(vec![(
        "ps".into(),
        HostItem::Value(ValueType::Array(Box::new(ValueType::Named("P".into())))),
    )])
    .expect("配置");
    let program = prepare("struct P { let x : i64; }; ps[2].x", &layout).expect("prepare");
    let reads = Rc::new(Cell::new(0));
    let mut binding = 構造体配列 {
        reads: reads.clone(),
    };
    let mut slots = [HostSlot::value("ps", &mut binding)];

    let result = EmbeddingRunner::new()
        .run_without_functions(&program, &mut slots)
        .expect("実行");
    assert_eq!(整数の結果(result), 42);
    assert_eq!(reads.get(), 1);
}

struct AddOne {
    calls: Rc<Cell<u32>>,
}

impl HostFn for AddOne {
    fn sig(&self) -> HostSig {
        HostSig {
            params: vec![ValueType::I64],
            ret: Some(ValueType::I64),
        }
    }

    fn call(&mut self, args: &[Value]) -> Option<Value> {
        self.calls.set(self.calls.get() + 1);
        Some(Value::I64(args.first()?.as_int()? as i64 + 1))
    }
}

struct Double {
    calls: Rc<Cell<u32>>,
}

impl HostFn for Double {
    fn sig(&self) -> HostSig {
        HostSig {
            params: vec![ValueType::I64],
            ret: Some(ValueType::I64),
        }
    }

    fn call(&mut self, args: &[Value]) -> Option<Value> {
        self.calls.set(self.calls.get() + 1);
        Some(Value::I64(args.first()?.as_int()? as i64 * 2))
    }
}

#[test]
fn host関数の番号はprepare時のfunction順で固定される() {
    let unary = HostSig {
        params: vec![ValueType::I64],
        ret: Some(ValueType::I64),
    };
    let layout = HostLayout::new(vec![
        ("first".into(), HostItem::Fn(unary.clone())),
        ("n".into(), HostItem::Value(ValueType::I64)),
        ("second".into(), HostItem::Fn(unary)),
    ])
    .expect("配置");
    let program = prepare("second(first(n))", &layout).expect("prepare");
    let mut values = program.host_values(vec![Value::I64(20)]).expect("host値");
    let first_calls = Rc::new(Cell::new(0));
    let second_calls = Rc::new(Cell::new(0));
    let mut first = AddOne {
        calls: first_calls.clone(),
    };
    let mut second = Double {
        calls: second_calls.clone(),
    };
    let mut functions = program
        .host_functions(vec![
            HostFunctionSlot::new("first", &mut first),
            HostFunctionSlot::new("second", &mut second),
        ])
        .expect("host関数");
    let result = EmbeddingRunner::new()
        .run_values(&program, &mut values, &mut functions)
        .expect("同じ配置")
        .expect("実行");
    drop(functions);

    assert_eq!(整数の結果(result), 42);
    assert_eq!(first_calls.get(), 1);
    assert_eq!(second_calls.get(), 1);
    assert_eq!(values.values()[0].as_int(), Some(20));
}

#[test]
fn host関数の順序違いは呼ぶ前に拒む() {
    let unary = HostSig {
        params: vec![ValueType::I64],
        ret: Some(ValueType::I64),
    };
    let layout = HostLayout::new(vec![
        ("first".into(), HostItem::Fn(unary.clone())),
        ("second".into(), HostItem::Fn(unary)),
    ])
    .expect("配置");
    let program = prepare("second(first(20))", &layout).expect("prepare");
    let first_calls = Rc::new(Cell::new(0));
    let second_calls = Rc::new(Cell::new(0));
    let mut first = AddOne {
        calls: first_calls.clone(),
    };
    let mut second = Double {
        calls: second_calls.clone(),
    };

    let Err(error) = program.host_functions(vec![
        HostFunctionSlot::new("second", &mut second),
        HostFunctionSlot::new("first", &mut first),
    ]) else {
        panic!("順序が違う");
    };
    assert!(matches!(
        error,
        HostFunctionsError::WrongName { index: 0, .. }
    ));
    assert_eq!(first_calls.get(), 0);
    assert_eq!(second_calls.get(), 0);
}

struct WrongSignature {
    calls: Rc<Cell<u32>>,
}

impl HostFn for WrongSignature {
    fn sig(&self) -> HostSig {
        HostSig {
            params: Vec::new(),
            ret: Some(ValueType::I64),
        }
    }

    fn call(&mut self, _args: &[Value]) -> Option<Value> {
        self.calls.set(self.calls.get() + 1);
        Some(Value::I64(0))
    }
}

#[test]
fn host関数の署名違いも呼ぶ前に拒む() {
    let layout = HostLayout::new(vec![(
        "first".into(),
        HostItem::Fn(HostSig {
            params: vec![ValueType::I64],
            ret: Some(ValueType::I64),
        }),
    )])
    .expect("配置");
    let program = prepare("first(1)", &layout).expect("prepare");
    let calls = Rc::new(Cell::new(0));
    let mut wrong = WrongSignature {
        calls: calls.clone(),
    };

    let Err(error) = program.host_functions(vec![HostFunctionSlot::new("first", &mut wrong)])
    else {
        panic!("署名が違う");
    };
    assert!(matches!(
        error,
        HostFunctionsError::WrongSignature { index: 0, .. }
    ));
    assert_eq!(calls.get(), 0);
}

#[test]
fn mixed_layoutは照合した実物のhost関数を呼ぶ() {
    let unary = HostSig {
        params: vec![ValueType::I64],
        ret: Some(ValueType::I64),
    };
    let layout = HostLayout::new(vec![
        ("n".into(), HostItem::Value(ValueType::I64)),
        ("first".into(), HostItem::Fn(unary)),
    ])
    .expect("配置");
    let program = prepare("first(n)", &layout).expect("prepare");
    let reads = Rc::new(Cell::new(0));
    let mut number = Number {
        value: 20,
        reads: reads.clone(),
        writes: Rc::new(Cell::new(0)),
    };
    let calls = Rc::new(Cell::new(0));
    let mut first = AddOne {
        calls: calls.clone(),
    };
    let mut slots = [
        HostSlot::value("n", &mut number),
        HostSlot::function("first", &mut first),
    ];

    let result = EmbeddingRunner::new()
        .run(&program, &mut slots)
        .expect("実行");
    assert_eq!(整数の結果(result), 21);
    assert_eq!(reads.get(), 1);
    assert_eq!(calls.get(), 1);
}

#[test]
fn mixed_layoutの関数順序違いも値を読む前に拒む() {
    let unary = HostSig {
        params: vec![ValueType::I64],
        ret: Some(ValueType::I64),
    };
    let layout = HostLayout::new(vec![
        ("n".into(), HostItem::Value(ValueType::I64)),
        ("first".into(), HostItem::Fn(unary.clone())),
        ("second".into(), HostItem::Fn(unary)),
    ])
    .expect("配置");
    let program = prepare("second(first(n))", &layout).expect("prepare");
    let reads = Rc::new(Cell::new(0));
    let mut number = Number {
        value: 20,
        reads: reads.clone(),
        writes: Rc::new(Cell::new(0)),
    };
    let first_calls = Rc::new(Cell::new(0));
    let second_calls = Rc::new(Cell::new(0));
    let mut first = AddOne {
        calls: first_calls.clone(),
    };
    let mut second = Double {
        calls: second_calls.clone(),
    };
    let mut slots = [
        HostSlot::value("n", &mut number),
        HostSlot::function("second", &mut second),
        HostSlot::function("first", &mut first),
    ];

    let error = EmbeddingRunner::new()
        .run(&program, &mut slots)
        .expect_err("関数順が違う");
    assert!(matches!(error, EmbeddingRunError::LayoutMismatch { .. }));
    assert_eq!(reads.get(), 0);
    assert_eq!(first_calls.get(), 0);
    assert_eq!(second_calls.get(), 0);
}

#[test]
fn 関数を要るlayoutはwithout_functionsで実行しない() {
    let layout = HostLayout::new(vec![(
        "first".into(),
        HostItem::Fn(HostSig {
            params: vec![ValueType::I64],
            ret: Some(ValueType::I64),
        }),
    )])
    .expect("配置");
    let program = prepare("first(1)", &layout).expect("prepare");
    let mut values = program.host_values(Vec::new()).expect("host値");

    assert!(matches!(
        EmbeddingRunner::new().run_values_without_functions(&program, &mut values),
        Err(PreparedRunError::FunctionsRequired { expected: 1 })
    ));
}

#[test]
fn 高水準without_functionsも値を読む前に拒む() {
    let layout = HostLayout::new(vec![
        ("n".into(), HostItem::Value(ValueType::I64)),
        (
            "first".into(),
            HostItem::Fn(HostSig {
                params: vec![ValueType::I64],
                ret: Some(ValueType::I64),
            }),
        ),
    ])
    .expect("配置");
    let program = prepare("first(n)", &layout).expect("prepare");
    let reads = Rc::new(Cell::new(0));
    let mut number = Number {
        value: 1,
        reads: reads.clone(),
        writes: Rc::new(Cell::new(0)),
    };
    let calls = Rc::new(Cell::new(0));
    let mut first = AddOne {
        calls: calls.clone(),
    };
    let mut slots = [
        HostSlot::value("n", &mut number),
        HostSlot::function("first", &mut first),
    ];

    let error = EmbeddingRunner::new()
        .run_without_functions(&program, &mut slots)
        .expect_err("関数が必要");
    assert!(matches!(
        error,
        EmbeddingRunError::FunctionsRequired { expected: 1 }
    ));
    assert_eq!(reads.get(), 0);
    assert_eq!(calls.get(), 0);
}

#[test]
fn 低水準host値は個数と型を実行前に検査する() {
    let layout = 数値一つの配置();
    let program = prepare("n + 1", &layout).expect("prepare");

    assert!(matches!(
        program.host_values(Vec::new()),
        Err(HostValuesError::WrongCount {
            expected: 1,
            actual: 0
        })
    ));
    assert!(matches!(
        program.host_values(vec![Value::U32(1)]),
        Err(HostValuesError::WrongType {
            index: 0,
            expected: ValueType::I64,
            actual: HostValueKind::U32,
            ..
        })
    ));
}

#[test]
fn 低水準host値は構造体の欄列と内部型も検査する() {
    let layout = 構造体一つの配置();
    let program = prepare("struct P { let x : i64; }; p.x", &layout).expect("prepare");
    assert!(matches!(
        program.host_values(vec![Value::strukt("P".into(), Vec::new())]),
        Err(HostValuesError::MalformedValue { .. })
    ));
    assert!(matches!(
        program.host_values(vec![Value::strukt(
            "P".into(),
            vec![("x".into(), Value::U32(1))]
        )]),
        Err(HostValuesError::MalformedValue { .. })
    ));
}

#[test]
fn 低水準host値は配列要素とmap鍵も内側まで検査する() {
    let layout = HostLayout::new(vec![
        (
            "a".into(),
            HostItem::Value(ValueType::Array(Box::new(ValueType::I64))),
        ),
        (
            "m".into(),
            HostItem::Value(ValueType::Map(
                Box::new(ValueType::U8),
                Box::new(ValueType::I64),
            )),
        ),
    ])
    .expect("配置");
    let program = prepare("0", &layout).expect("prepare");
    let mut entries = BTreeMap::new();
    entries.insert(MapKey::Int(300), Value::I64(1));
    assert!(matches!(
        program.host_values(vec![
            Value::array(ValueType::I64, vec![Value::U32(1)]),
            Value::map(ValueType::U8, ValueType::I64, entries),
        ]),
        Err(HostValuesError::MalformedValue { index: 0, .. })
    ));

    let mut entries = BTreeMap::new();
    entries.insert(MapKey::Int(300), Value::I64(1));
    assert!(matches!(
        program.host_values(vec![
            Value::array(ValueType::I64, vec![Value::I64(1)]),
            Value::map(ValueType::U8, ValueType::I64, entries),
        ]),
        Err(HostValuesError::MalformedValue { index: 1, .. })
    ));
}

#[test]
fn 低水準host値はhashのentryとindexの不整合を拒む() {
    let layout = HostLayout::new(vec![(
        "h".into(),
        HostItem::Value(ValueType::Hash(
            Box::new(ValueType::I64),
            Box::new(ValueType::I64),
        )),
    )])
    .expect("配置");
    let program = prepare("0", &layout).expect("prepare");
    let mut hash = HashVal::new(ValueType::I64, ValueType::I64);
    hash.entries.push(Some((MapKey::Int(1), Value::I64(2))));
    assert!(matches!(
        program.host_values(vec![Value::Hash(Box::new(hash))]),
        Err(HostValuesError::MalformedValue { .. })
    ));
}

#[test]
fn wrap_host値は空名一欄と基底型を検査する() {
    let layout = 構造体一つの配置();
    let program = prepare("wrap P = i64; 0", &layout).expect("prepare");
    assert!(program
        .host_values(vec![Value::strukt(
            "P".into(),
            vec![(String::new(), Value::I64(1))]
        )])
        .is_ok());
    assert!(matches!(
        program.host_values(vec![Value::strukt(
            "P".into(),
            vec![(String::new(), Value::U32(1))]
        )]),
        Err(HostValuesError::MalformedValue { .. })
    ));
}

struct 型と違う値を返す束縛;

impl HostBinding for 型と違う値を返す束縛 {
    fn type_of(&self) -> ValueType {
        ValueType::I64
    }

    fn read(&self) -> Value {
        Value::U32(1)
    }

    fn write(&mut self, _value: &Value) {}
}

#[test]
fn 高水準経路もbindingが返した値の型を実行前に検査する() {
    let layout = 数値一つの配置();
    let program = prepare("n + 1", &layout).expect("prepare");
    let mut binding = 型と違う値を返す束縛;
    let mut slots = [HostSlot::value("n", &mut binding)];

    let error = EmbeddingRunner::new()
        .run_without_functions(&program, &mut slots)
        .expect_err("宣言した型と実値が違う");
    assert!(matches!(
        error,
        EmbeddingRunError::HostValues(HostValuesError::WrongType {
            expected: ValueType::I64,
            actual: HostValueKind::U32,
            ..
        })
    ));
}

struct 欄が欠けた構造体を返す束縛;

impl HostBinding for 欄が欠けた構造体を返す束縛 {
    fn type_of(&self) -> ValueType {
        ValueType::Named("P".into())
    }

    fn read(&self) -> Value {
        Value::strukt("P".into(), Vec::new())
    }

    fn write(&mut self, _value: &Value) {}
}

#[test]
fn 高水準経路も構造体bindingの欄列を実行前に検査する() {
    let layout = 構造体一つの配置();
    let program = prepare("struct P { let x : i64; }; p.x", &layout).expect("prepare");
    let mut binding = 欄が欠けた構造体を返す束縛;
    let mut slots = [HostSlot::value("p", &mut binding)];

    assert!(matches!(
        EmbeddingRunner::new().run_without_functions(&program, &mut slots),
        Err(EmbeddingRunError::HostValues(
            HostValuesError::MalformedValue { .. }
        ))
    ));
}

struct 欄が欠けた構造体を返す関数;

impl HostFn for 欄が欠けた構造体を返す関数 {
    fn sig(&self) -> HostSig {
        HostSig {
            params: Vec::new(),
            ret: Some(ValueType::Named("P".into())),
        }
    }

    fn call(&mut self, _args: &[Value]) -> Option<Value> {
        Some(Value::strukt("P".into(), Vec::new()))
    }
}

#[test]
fn host関数が返した不正構造体は呼び出し直後に止める() {
    let layout = HostLayout::new(vec![
        ("p".into(), HostItem::Value(ValueType::Named("P".into()))),
        (
            "make".into(),
            HostItem::Fn(HostSig {
                params: Vec::new(),
                ret: Some(ValueType::Named("P".into())),
            }),
        ),
    ])
    .expect("配置");
    let program = prepare("struct P { let x : i64; }; p := make(); 0", &layout).expect("prepare");
    let mut values = program
        .host_values(vec![Value::strukt(
            "P".into(),
            vec![("x".into(), Value::I64(1))],
        )])
        .expect("host値");
    let mut function = 欄が欠けた構造体を返す関数;
    let mut functions = program
        .host_functions(vec![HostFunctionSlot::new("make", &mut function)])
        .expect("host関数");

    let result = EmbeddingRunner::new()
        .run_values(&program, &mut values, &mut functions)
        .expect("検査済みtoken");
    assert!(result.is_err(), "host契約違反はVM error");
    let Value::Struct(value) = &values.values()[0] else {
        panic!("構造体値を保つ");
    };
    assert_eq!(value.fields.len(), 1, "不正返値をhost slotへ書かない");
}

struct 違うscalar型を返す関数;

impl HostFn for 違うscalar型を返す関数 {
    fn sig(&self) -> HostSig {
        HostSig {
            params: Vec::new(),
            ret: Some(ValueType::I64),
        }
    }

    fn call(&mut self, _args: &[Value]) -> Option<Value> {
        Some(Value::U32(1))
    }
}

struct 返値無しの宣言で値を返す関数;

impl HostFn for 返値無しの宣言で値を返す関数 {
    fn sig(&self) -> HostSig {
        HostSig {
            params: Vec::new(),
            ret: None,
        }
    }

    fn call(&mut self, _args: &[Value]) -> Option<Value> {
        Some(Value::I64(1))
    }
}

#[test]
fn host関数のscalar返値型違反は後続命令前に止める() {
    let layout = HostLayout::new(vec![(
        "bad".into(),
        HostItem::Fn(HostSig {
            params: Vec::new(),
            ret: Some(ValueType::I64),
        }),
    )])
    .expect("配置");
    let program = prepare("bad()", &layout).expect("prepare");
    let mut values = program.host_values(Vec::new()).expect("host値");
    let mut function = 違うscalar型を返す関数;
    let mut functions = program
        .host_functions(vec![HostFunctionSlot::new("bad", &mut function)])
        .expect("host関数");

    let result = EmbeddingRunner::new()
        .run_values(&program, &mut values, &mut functions)
        .expect("検査済みtoken");
    assert!(result.is_err());
}

#[test]
fn 返値無しを宣言したhost関数のsome値を拒む() {
    let layout = HostLayout::new(vec![(
        "bad".into(),
        HostItem::Fn(HostSig {
            params: Vec::new(),
            ret: None,
        }),
    )])
    .expect("配置");
    let program = prepare("bad(); 42", &layout).expect("prepare");
    let mut values = program.host_values(Vec::new()).expect("host値");
    let mut function = 返値無しの宣言で値を返す関数;
    let mut functions = program
        .host_functions(vec![HostFunctionSlot::new("bad", &mut function)])
        .expect("host関数");

    let result = EmbeddingRunner::new()
        .run_values(&program, &mut values, &mut functions)
        .expect("検査済みtoken");
    assert!(result.is_err());
}

#[test]
fn 高水準host関数契約違反でもそれまでのwritebackを保つ() {
    let layout = HostLayout::new(vec![
        ("n".into(), HostItem::Value(ValueType::I64)),
        (
            "make".into(),
            HostItem::Fn(HostSig {
                params: Vec::new(),
                ret: Some(ValueType::Named("P".into())),
            }),
        ),
    ])
    .expect("配置");
    let program = prepare(
        "struct P { let x : i64; }; n := 42; make(); n := 99; 0",
        &layout,
    )
    .expect("prepare");
    let reads = Rc::new(Cell::new(0));
    let writes = Rc::new(Cell::new(0));
    let mut number = Number {
        value: 1,
        reads,
        writes: writes.clone(),
    };
    let mut function = 欄が欠けた構造体を返す関数;
    let mut slots = [
        HostSlot::value("n", &mut number),
        HostSlot::function("make", &mut function),
    ];

    let error = EmbeddingRunner::new()
        .run(&program, &mut slots)
        .expect_err("host契約違反");
    assert!(matches!(error, EmbeddingRunError::Runtime(_)));
    assert_eq!(number.value, 42, "後続命令は走らず先行変更は書き戻す");
    assert_eq!(writes.get(), 1);
}

#[test]
fn 実行時errorでもそれ以前のwritebackを失わない() {
    let layout = 数値一つの配置();
    let program = prepare("n := 42; n := n / 0; 0", &layout).expect("prepare");
    let reads = Rc::new(Cell::new(0));
    let writes = Rc::new(Cell::new(0));
    let mut number = Number {
        value: 7,
        reads,
        writes: writes.clone(),
    };
    let mut slots = [HostSlot::value("n", &mut number)];

    let error = EmbeddingRunner::new()
        .run_without_functions(&program, &mut slots)
        .expect_err("零除算");
    assert!(matches!(error, EmbeddingRunError::Runtime(_)));
    assert_eq!(number.value, 42);
    assert_eq!(writes.get(), 1);
}

#[test]
fn 低水準経路も実行時errorの後の値をtokenへ戻す() {
    let layout = 数値一つの配置();
    let program = prepare("n := 42; n := n / 0; 0", &layout).expect("prepare");
    let mut values = program.host_values(vec![Value::I64(7)]).expect("host値");
    let result = EmbeddingRunner::new()
        .run_values_without_functions(&program, &mut values)
        .expect("同じ配置");

    assert!(result.is_err(), "零除算は実行時error");
    assert_eq!(values.values()[0].as_int(), Some(42));
}

#[test]
fn 低水準host値tokenは別のlayoutへ流用できない() {
    let first_layout = 数値一つの配置();
    let second_layout = 数値一つの配置();
    let first = prepare("n + 1", &first_layout).expect("一つ目");
    let second = prepare("n + 1", &second_layout).expect("二つ目");
    let mut values = first.host_values(vec![Value::I64(1)]).expect("host値");

    assert!(EmbeddingRunner::new()
        .run_values_without_functions(&second, &mut values)
        .is_err());
}

#[test]
fn 同じcanonical_layoutなら別のprepared_sourceでもtokenを共有できる() {
    let layout = 数値一つの配置();
    let first = prepare("n + 1", &layout).expect("一つ目");
    let second = prepare("n + 2", &layout).expect("二つ目");
    let mut values = first.host_values(vec![Value::I64(40)]).expect("host値");

    let result = EmbeddingRunner::new()
        .run_values_without_functions(&second, &mut values)
        .expect("同じcanonical layout")
        .expect("実行");
    assert_eq!(整数の結果(result), 42);
}

fn 構造体一つの配置() -> HostLayout {
    HostLayout::new(vec![(
        "p".into(),
        HostItem::Value(ValueType::Named("P".into())),
    )])
    .expect("配置")
}

#[test]
fn 同じ名付き型schemaなら別sourceでもtokenを共有できる() {
    let layout = 構造体一つの配置();
    let first = prepare("struct P { let x : i64; }; p.x", &layout).expect("一つ目");
    let second = prepare("struct P { let x : i64; }; p.x + 1", &layout).expect("二つ目");
    let mut values = first
        .host_values(vec![Value::strukt(
            "P".into(),
            vec![("x".into(), Value::I64(41))],
        )])
        .expect("host値");

    let result = EmbeddingRunner::new()
        .run_values_without_functions(&second, &mut values)
        .expect("同じ名付き型schema")
        .expect("実行");
    assert_eq!(整数の結果(result), 42);
}

#[test]
fn 同じlayoutでも名付き型schemaが違えば値tokenを拒む() {
    let layout = 構造体一つの配置();
    let first = prepare("struct P { let x : i64; }; p.x", &layout).expect("一つ目");
    let second = prepare("struct P { let y : u32; }; p.y", &layout).expect("二つ目");
    let mut values = first
        .host_values(vec![Value::strukt(
            "P".into(),
            vec![("x".into(), Value::I64(41))],
        )])
        .expect("host値");

    assert!(matches!(
        EmbeddingRunner::new().run_values_without_functions(&second, &mut values),
        Err(PreparedRunError::HostValuesTypeSchemaMismatch)
    ));
}

#[test]
fn 同名structがあってもwrap基底型の違いをschemaに含める() {
    let layout = 構造体一つの配置();
    let first = prepare("struct P { let x : i64; }; wrap P = i64; 0", &layout).expect("一つ目");
    let second = prepare("struct P { let x : i64; }; wrap P = u32; 0", &layout).expect("二つ目");
    let mut values = first
        .host_values(vec![Value::strukt(
            "P".into(),
            vec![("x".into(), Value::I64(1))],
        )])
        .expect("host値");

    assert!(matches!(
        EmbeddingRunner::new().run_values_without_functions(&second, &mut values),
        Err(PreparedRunError::HostValuesTypeSchemaMismatch)
    ));
}

struct 名付き型を受ける関数 {
    calls: Rc<Cell<u32>>,
}

impl HostFn for 名付き型を受ける関数 {
    fn sig(&self) -> HostSig {
        HostSig {
            params: vec![ValueType::Named("P".into())],
            ret: Some(ValueType::I64),
        }
    }

    fn call(&mut self, _args: &[Value]) -> Option<Value> {
        self.calls.set(self.calls.get() + 1);
        Some(Value::I64(1))
    }
}

#[test]
fn host関数の名付き型schemaが違えば呼ぶ前に拒む() {
    let layout = HostLayout::new(vec![(
        "inspect".into(),
        HostItem::Fn(HostSig {
            params: vec![ValueType::Named("P".into())],
            ret: Some(ValueType::I64),
        }),
    )])
    .expect("配置");
    let first = prepare(
        "struct P { let x : i64; }; inspect(new P ( x := 1 ))",
        &layout,
    )
    .expect("一つ目");
    let second = prepare(
        "struct P { let y : u32; }; inspect(new P ( y := 1 ))",
        &layout,
    )
    .expect("二つ目");
    let mut values = second.host_values(Vec::new()).expect("host値");
    let calls = Rc::new(Cell::new(0));
    let mut function = 名付き型を受ける関数 {
        calls: calls.clone(),
    };
    let mut functions = first
        .host_functions(vec![HostFunctionSlot::new("inspect", &mut function)])
        .expect("host関数");

    assert!(matches!(
        EmbeddingRunner::new().run_values(&second, &mut values, &mut functions),
        Err(PreparedRunError::HostFunctionsTypeSchemaMismatch)
    ));
    drop(functions);
    assert_eq!(calls.get(), 0);
}

#[test]
fn writebackはhost値vecの容量を再利用する() {
    let layout = 数値一つの配置();
    let program = prepare("n += 1; n", &layout).expect("prepare");
    let mut input = Vec::with_capacity(64);
    input.push(Value::I64(1));
    let mut values = program.host_values(input).expect("host値");
    EmbeddingRunner::new()
        .run_values_without_functions(&program, &mut values)
        .expect("同じ配置")
        .expect("実行");

    let after = values.into_values();
    assert!(after.capacity() >= 64, "元のbufferを失ってはならない");
    assert_eq!(after[0].as_int(), Some(2));
}

#[test]
fn host関数indexの表現範囲を越えるlayoutはcompile前に拒む() {
    let syntax = vaak::parser::parse("0").expect("構文");
    let signature = HostSig {
        params: Vec::new(),
        ret: None,
    };
    let host = (0..u16::MAX as usize + 2)
        .map(|index| (format!("f{index}"), HostItem::Fn(signature.clone())))
        .collect::<Vec<_>>();
    let error = vaak::vm::compile_with_host(&syntax, &host).expect_err("u16を越える");
    assert!(error.msg.contains("65536"), "{}", error.msg);
}

#[test]
fn prepared_host_layoutもvmのslot個数上限を先に検査する() {
    let values = (0..u16::MAX as usize + 1)
        .map(|index| (format!("v{index}"), HostItem::Value(ValueType::I64)))
        .collect();
    let value_error = match HostLayout::new(values) {
        Err(error) => error,
        Ok(_) => panic!("多すぎるhost値を受理してはならない"),
    };
    assert!(matches!(
        value_error,
        HostLayoutError::TooManyValueSlots {
            actual,
            ..
        } if actual == u16::MAX as usize + 1
    ));

    let functions = (0..u16::MAX as usize + 2)
        .map(|index| {
            (
                format!("f{index}"),
                HostItem::Fn(HostSig {
                    params: Vec::new(),
                    ret: None,
                }),
            )
        })
        .collect();
    let function_error = match HostLayout::new(functions) {
        Err(error) => error,
        Ok(_) => panic!("多すぎるhost関数を受理してはならない"),
    };
    assert!(matches!(
        function_error,
        HostLayoutError::TooManyFunctionSlots {
            actual,
            ..
        } if actual == u16::MAX as usize + 2
    ));
}

#[test]
fn host値slotの表現範囲を越えるlayoutはcompile前に拒む() {
    let syntax = vaak::parser::parse("0").expect("構文");
    let host = (0..u16::MAX as usize + 1)
        .map(|index| (format!("v{index}"), HostItem::Value(ValueType::I64)))
        .collect::<Vec<_>>();
    let error = vaak::vm::compile_with_host(&syntax, &host).expect_err("u16を越える");
    assert!(error.msg.contains("65535"), "{}", error.msg);
}

#[test]
fn 関数引数の表現範囲を越えるとcompileで拒む() {
    let arguments = std::iter::repeat("0")
        .take(u16::MAX as usize + 1)
        .collect::<Vec<_>>()
        .join(",");
    let syntax = vaak::parser::parse(&format!("f({arguments})")).expect("構文");
    let host = vec![(
        "f".into(),
        HostItem::Fn(HostSig {
            params: Vec::new(),
            ret: None,
        }),
    )];
    let error = vaak::vm::compile_with_host(&syntax, &host).expect_err("u16を越える");
    assert!(error.msg.contains("65535"), "{}", error.msg);
}

#[test]
fn 配列literalは予約した個数と衝突する前に拒む() {
    let items = std::iter::repeat("0")
        .take(u16::MAX as usize)
        .collect::<Vec<_>>()
        .join(",");
    let syntax = vaak::parser::parse(&format!("[{items}]")).expect("構文");
    let error = vaak::vm::compile(&syntax).expect_err("予約値と衝突する");
    assert!(error.msg.contains("65534"), "{}", error.msg);
}

#[test]
fn map_literalの組数が表現範囲を越えるとcompileで拒む() {
    let pairs = (0..u16::MAX as usize + 1)
        .map(|index| format!("{index} => 0"))
        .collect::<Vec<_>>()
        .join(",");
    let syntax = vaak::parser::parse(&format!("({pairs})")).expect("構文");
    let error = vaak::vm::compile(&syntax).expect_err("u16を越える");
    assert!(error.msg.contains("65535"), "{}", error.msg);
}

#[test]
fn 構造体の欄数が表現範囲を越えるとcompileで拒む() {
    let fields = (0..u16::MAX as usize + 1)
        .map(|index| format!("let f{index} : i64;"))
        .collect::<String>();
    let syntax = vaak::parser::parse(&format!("struct P {{ {fields} }}; new P ()")).expect("構文");
    let error = vaak::vm::compile(&syntax).expect_err("u16を越える");
    assert!(error.msg.contains("65535"), "{}", error.msg);
}

#[test]
fn host名の重複はlayoutを作る時点で拒む() {
    let error = HostLayout::new(vec![
        ("same".into(), HostItem::Value(ValueType::I64)),
        (
            "same".into(),
            HostItem::Fn(HostSig {
                params: Vec::new(),
                ret: None,
            }),
        ),
    ])
    .expect_err("値と関数でも同じ名前は一つに決まらない");
    assert_eq!(error, HostLayoutError::DuplicateName("same".into()));
}

#[test]
fn prepare診断は検査段を区別する() {
    let layout = HostLayout::new(Vec::new()).expect("空配置");
    let parse_error = prepare("(", &layout).expect_err("構文誤り");
    assert!(parse_error
        .diagnostics()
        .iter()
        .all(|diagnostic| diagnostic.stage == PrepareStage::Parse));

    let static_error = prepare("unknown + 1", &layout).expect_err("名前誤り");
    assert!(static_error
        .diagnostics()
        .iter()
        .any(|diagnostic| diagnostic.stage == PrepareStage::Check));
}

#[test]
fn vmのhost値layoutは集合体内のf80をprepare時に拒む() {
    let layout = HostLayout::new(vec![(
        "values".into(),
        HostItem::Value(ValueType::Array(Box::new(ValueType::F80))),
    )])
    .expect("配置");
    let error = prepare("0", &layout).expect_err("VMにF80値表現は無い");
    assert!(
        error.diagnostics().iter().any(|diagnostic| {
            diagnostic.stage == PrepareStage::Compile
                && diagnostic.message.to_ascii_lowercase().contains("f80")
        }),
        "{:?}",
        error.diagnostics()
    );
}

#[test]
fn vmのhost関数layoutは署名内のf80をprepare時に拒む() {
    let layout = HostLayout::new(vec![(
        "f".into(),
        HostItem::Fn(HostSig {
            params: vec![ValueType::Map(
                Box::new(ValueType::Str),
                Box::new(ValueType::F80),
            )],
            ret: Some(ValueType::F80),
        }),
    )])
    .expect("配置");
    let error = prepare("0", &layout).expect_err("VMにF80値表現は無い");
    assert!(error.diagnostics().iter().any(|diagnostic| {
        diagnostic.stage == PrepareStage::Compile && diagnostic.message.contains("F80")
    }));
}

#[test]
fn 未定義の名付きhost値型はprepare時に拒む() {
    let layout = HostLayout::new(vec![(
        "value".into(),
        HostItem::Value(ValueType::Named("Missing".into())),
    )])
    .expect("配置");
    let error = prepare("0", &layout).expect_err("opaqueな名付き型は未実装");
    assert!(error.diagnostics().iter().any(|diagnostic| {
        diagnostic.stage == PrepareStage::Compile && diagnostic.message.contains("Missing")
    }));
}

#[test]
fn host関数署名の集合体内にある未定義名付き型もprepare時に拒む() {
    let layout = HostLayout::new(vec![(
        "inspect".into(),
        HostItem::Fn(HostSig {
            params: vec![ValueType::Array(Box::new(ValueType::Named(
                "MissingParam".into(),
            )))],
            ret: Some(ValueType::Map(
                Box::new(ValueType::Str),
                Box::new(ValueType::Named("MissingResult".into())),
            )),
        }),
    )])
    .expect("配置");
    let error = prepare("0", &layout).expect_err("署名の内側もopaqueにはしない");
    assert!(error.diagnostics().iter().any(|diagnostic| {
        diagnostic.stage == PrepareStage::Compile
            && (diagnostic.message.contains("MissingParam")
                || diagnostic.message.contains("MissingResult"))
    }));
}

#[test]
fn 到達schemaを展開した深さも実値と同じ境界でprepareする() {
    let layout = 構造体一つの配置();

    let accepted_type = format!("i64{}", " array".repeat(255));
    let accepted = prepare(
        &format!("struct P {{ let x : {accepted_type}; }}; 0"),
        &layout,
    )
    .expect("struct rootを含めて深さ256までは表せる");
    let mut field_type = ValueType::I64;
    let mut field_value = Value::I64(1);
    for _ in 0..255 {
        field_value = Value::array(field_type.clone(), vec![field_value]);
        field_type = ValueType::Array(Box::new(field_type));
    }
    assert!(accepted
        .host_values(vec![Value::strukt(
            "P".into(),
            vec![("x".into(), field_value)]
        )])
        .is_ok());

    let rejected_type = format!("i64{}", " array".repeat(256));
    let error = prepare(
        &format!("struct P {{ let x : {rejected_type}; }}; 0"),
        &layout,
    )
    .expect_err("実値では深さ257になるschemaをprepareしてはならない");
    assert!(error.diagnostics().iter().any(|diagnostic| {
        diagnostic.stage == PrepareStage::Compile && diagnostic.message.contains("深さ上限")
    }));
}

#[test]
fn 長い名付き型chainも展開後の深さでprepare時に拒む() {
    let layout = HostLayout::new(vec![(
        "root".into(),
        HostItem::Value(ValueType::Named("P0".into())),
    )])
    .expect("配置");
    let mut source = String::new();
    for index in 0..256 {
        source.push_str(&format!(
            "struct P{index} {{ let next : P{}; }};",
            index + 1
        ));
    }
    source.push_str("struct P256 { let value : i64; }; 0");
    let error = prepare(&source, &layout).expect_err("Named展開で深さ257になる");
    assert!(error.diagnostics().iter().any(|diagnostic| {
        diagnostic.stage == PrepareStage::Compile && diagnostic.message.contains("深さ上限")
    }));
}

#[test]
fn 到達schema全体の要素数もprepare時に制限する() {
    let layout = 構造体一つの配置();
    let p_fields = (0..32_767)
        .map(|index| format!("let p{index} : i64;"))
        .collect::<String>();
    let q_fields = (0..32_768)
        .map(|index| format!("let q{index} : i64;"))
        .collect::<String>();
    let source = format!("struct P {{ let child : Q; {p_fields} }}; struct Q {{ {q_fields} }}; 0");
    let error = prepare(&source, &layout).expect_err("展開schemaが65536要素を越える");
    assert!(error.diagnostics().iter().any(|diagnostic| {
        diagnostic.stage == PrepareStage::Compile && diagnostic.message.contains("要素数上限")
    }));
}

#[test]
fn 到達schema内のf80もvm用prepare時に拒む() {
    let layout = 構造体一つの配置();
    let error = prepare("struct P { let x : f80; }; 0", &layout).expect_err("VMにF80値表現は無い");
    assert!(
        error.diagnostics().iter().any(|diagnostic| {
            diagnostic.stage == PrepareStage::Compile
                && diagnostic.message.to_ascii_lowercase().contains("f80")
        }),
        "{:?}",
        error.diagnostics()
    );
}

#[test]
fn 深すぎるhost型記述子はstackを使わずに拒んで解放する() {
    let mut ty = ValueType::I64;
    for _ in 0..100_000 {
        ty = ValueType::Array(Box::new(ty));
    }
    let error = match HostLayout::new(vec![("deep".into(), HostItem::Value(ty))]) {
        Err(error) => error,
        Ok(_) => panic!("深い型記述子を受理してはならない"),
    };
    assert!(matches!(
        error,
        HostLayoutError::TypeDescriptorTooDeep { .. }
    ));
}

#[test]
fn 要素数が多すぎるhost型記述子も反復検査で拒む() {
    let entries = (0..2)
        .map(|index| {
            (
                format!("f{index}"),
                HostItem::Fn(HostSig {
                    params: (0..u16::MAX as usize).map(|_| ValueType::I64).collect(),
                    ret: None,
                }),
            )
        })
        .collect();
    let error = match HostLayout::new(entries) {
        Err(error) => error,
        Ok(_) => panic!("大きすぎる型記述子を受理してはならない"),
    };
    assert!(matches!(
        error,
        HostLayoutError::TypeDescriptorTooLarge { .. }
    ));
}

#[test]
fn host関数署名の引数個数はu16範囲で固定する() {
    let signature = HostSig {
        params: (0..u16::MAX as usize + 1).map(|_| ValueType::I64).collect(),
        ret: None,
    };
    let error = match HostLayout::new(vec![("many".into(), HostItem::Fn(signature))]) {
        Err(error) => error,
        Ok(_) => panic!("VMが呼べない署名を受理してはならない"),
    };
    assert!(matches!(
        error,
        HostLayoutError::TooManyFunctionParameters {
            actual,
            ..
        } if actual == u16::MAX as usize + 1
    ));
}

#[test]
fn 低水準host値は非有限浮動小数点を内側まで拒む() {
    let scalar_layout =
        HostLayout::new(vec![("number".into(), HostItem::Value(ValueType::F64))]).expect("配置");
    let scalar = prepare("0", &scalar_layout).expect("prepare");
    assert!(matches!(
        scalar.host_values(vec![Value::F64(f64::NAN)]),
        Err(HostValuesError::MalformedValue { .. })
    ));

    let nested_layout = HostLayout::new(vec![(
        "numbers".into(),
        HostItem::Value(ValueType::Array(Box::new(ValueType::F32))),
    )])
    .expect("配置");
    let nested = prepare("0", &nested_layout).expect("prepare");
    assert!(matches!(
        nested.host_values(vec![Value::array(
            ValueType::F32,
            vec![Value::F32(f32::INFINITY)]
        )]),
        Err(HostValuesError::MalformedValue { .. })
    ));
}

struct 無限大を返す束縛;

impl HostBinding for 無限大を返す束縛 {
    fn type_of(&self) -> ValueType {
        ValueType::F64
    }

    fn read(&self) -> Value {
        Value::F64(f64::NEG_INFINITY)
    }

    fn write(&mut self, _value: &Value) {}
}

#[test]
fn 高水準bindingの非有限浮動小数点は実行前に拒む() {
    let layout =
        HostLayout::new(vec![("number".into(), HostItem::Value(ValueType::F64))]).expect("配置");
    let program = prepare("number", &layout).expect("prepare");
    let mut binding = 無限大を返す束縛;
    let mut slots = [HostSlot::value("number", &mut binding)];
    assert!(matches!(
        EmbeddingRunner::new().run_without_functions(&program, &mut slots),
        Err(EmbeddingRunError::HostValues(
            HostValuesError::MalformedValue { .. }
        ))
    ));
}

struct 非有限配列を返す関数;

impl HostFn for 非有限配列を返す関数 {
    fn sig(&self) -> HostSig {
        HostSig {
            params: Vec::new(),
            ret: Some(ValueType::Array(Box::new(ValueType::F32))),
        }
    }

    fn call(&mut self, _args: &[Value]) -> Option<Value> {
        Some(Value::array(ValueType::F32, vec![Value::F32(f32::NAN)]))
    }
}

#[test]
fn host関数返値の集合体内にある非有限値は呼出し直後に止める() {
    let layout = HostLayout::new(vec![(
        "bad".into(),
        HostItem::Fn(HostSig {
            params: Vec::new(),
            ret: Some(ValueType::Array(Box::new(ValueType::F32))),
        }),
    )])
    .expect("配置");
    let program = prepare("bad(); 42", &layout).expect("prepare");
    let mut values = program.host_values(Vec::new()).expect("host値");
    let mut function = 非有限配列を返す関数;
    let mut functions = program
        .host_functions(vec![HostFunctionSlot::new("bad", &mut function)])
        .expect("host関数");
    let result = EmbeddingRunner::new()
        .run_values(&program, &mut values, &mut functions)
        .expect("検査済みtoken");
    assert!(result.is_err(), "host契約違反はVM error");
    drop(functions);

    let mut function = 非有限配列を返す関数;
    let mut slots = [HostSlot::function("bad", &mut function)];
    assert!(matches!(
        EmbeddingRunner::new().run(&program, &mut slots),
        Err(EmbeddingRunError::Runtime(_))
    ));
}

fn 深い不正配列値() -> Value {
    let mut value = Value::I64(0);
    for _ in 0..100_000 {
        value = Value::array(ValueType::I64, vec![value]);
    }
    value
}

fn 深い型記述子() -> ValueType {
    let mut ty = ValueType::I64;
    for _ in 0..100_000 {
        ty = ValueType::Array(Box::new(ty));
    }
    ty
}

#[test]
fn 低水準host値の深い不正値と型記述子を反復解放する() {
    let layout = 数値一つの配置();
    let program = prepare("n", &layout).expect("prepare");
    assert!(matches!(
        program.host_values(vec![深い不正配列値()]),
        Err(HostValuesError::WrongType { .. })
    ));
    assert!(matches!(
        program.host_values(vec![Value::array(深い型記述子(), Vec::new())]),
        Err(HostValuesError::MalformedValue { .. })
    ));
}

struct 深い不正値を返す束縛;

impl HostBinding for 深い不正値を返す束縛 {
    fn type_of(&self) -> ValueType {
        ValueType::I64
    }

    fn read(&self) -> Value {
        深い不正配列値()
    }

    fn write(&mut self, _value: &Value) {}
}

#[test]
fn 高水準bindingの深い不正値はclone前に拒んで反復解放する() {
    let layout = 数値一つの配置();
    let program = prepare("n", &layout).expect("prepare");
    let mut binding = 深い不正値を返す束縛;
    let mut slots = [HostSlot::value("n", &mut binding)];
    assert!(matches!(
        EmbeddingRunner::new().run_without_functions(&program, &mut slots),
        Err(EmbeddingRunError::HostValues(
            HostValuesError::WrongType { .. }
        ))
    ));
}

struct 深い不正値を返す関数;

impl HostFn for 深い不正値を返す関数 {
    fn sig(&self) -> HostSig {
        HostSig {
            params: Vec::new(),
            ret: Some(ValueType::I64),
        }
    }

    fn call(&mut self, _args: &[Value]) -> Option<Value> {
        Some(深い不正配列値())
    }
}

#[test]
fn host関数の深い不正返値は呼出し直後に反復解放する() {
    let layout = HostLayout::new(vec![(
        "bad".into(),
        HostItem::Fn(HostSig {
            params: Vec::new(),
            ret: Some(ValueType::I64),
        }),
    )])
    .expect("配置");
    let program = prepare("bad(); 42", &layout).expect("prepare");
    let mut values = program.host_values(Vec::new()).expect("host値");
    let mut function = 深い不正値を返す関数;
    let mut functions = program
        .host_functions(vec![HostFunctionSlot::new("bad", &mut function)])
        .expect("host関数");
    let result = EmbeddingRunner::new()
        .run_values(&program, &mut values, &mut functions)
        .expect("検査済みtoken");
    assert!(result.is_err(), "host契約違反はVM error");
}

struct 深い署名を返す関数;

impl HostFn for 深い署名を返す関数 {
    fn sig(&self) -> HostSig {
        HostSig {
            params: Vec::new(),
            ret: Some(深い型記述子()),
        }
    }

    fn call(&mut self, _args: &[Value]) -> Option<Value> {
        None
    }
}

#[test]
fn 低水準host関数tokenは深い実署名をeq前に拒んで反復解放する() {
    let layout = HostLayout::new(vec![(
        "bad".into(),
        HostItem::Fn(HostSig {
            params: Vec::new(),
            ret: Some(ValueType::I64),
        }),
    )])
    .expect("配置");
    let program = prepare("bad()", &layout).expect("prepare");
    let mut function = 深い署名を返す関数;
    let error = match program.host_functions(vec![HostFunctionSlot::new("bad", &mut function)]) {
        Err(error) => error,
        Ok(_) => panic!("深い実署名を受理してはならない"),
    };
    assert!(matches!(error, HostFunctionsError::InvalidSignature { .. }));
}

struct 巨大長を申告する配列束縛 {
    full_reads: Rc<Cell<u32>>,
    element_reads: Rc<Cell<u32>>,
}

impl HostBinding for 巨大長を申告する配列束縛 {
    fn type_of(&self) -> ValueType {
        ValueType::Array(Box::new(ValueType::I64))
    }

    fn read(&self) -> Value {
        self.full_reads.set(self.full_reads.get() + 1);
        Value::array(ValueType::I64, vec![Value::I64(42)])
    }

    fn write(&mut self, _value: &Value) {}

    fn read_at(&self, _index: usize) -> Option<Value> {
        self.element_reads.set(self.element_reads.get() + 1);
        Some(Value::I64(42))
    }

    fn len(&self) -> Option<usize> {
        Some(usize::MAX)
    }
}

#[test]
fn 部分読みの巨大lenはdummy確保せず丸ごと読みに戻す() {
    let layout = HostLayout::new(vec![(
        "values".into(),
        HostItem::Value(ValueType::Array(Box::new(ValueType::I64))),
    )])
    .expect("配置");
    let program = prepare("values[0]", &layout).expect("prepare");
    let full_reads = Rc::new(Cell::new(0));
    let element_reads = Rc::new(Cell::new(0));
    let mut binding = 巨大長を申告する配列束縛 {
        full_reads: full_reads.clone(),
        element_reads: element_reads.clone(),
    };
    let mut slots = [HostSlot::value("values", &mut binding)];
    let result = EmbeddingRunner::new()
        .run_without_functions(&program, &mut slots)
        .expect("丸ごと経路");
    assert_eq!(整数の結果(result), 42);
    assert_eq!(full_reads.get(), 1);
    assert_eq!(element_reads.get(), 0);
}

struct 深い要素型の大配列束縛 {
    element: ValueType,
    element_reads: Rc<Cell<u32>>,
}

impl HostBinding for 深い要素型の大配列束縛 {
    fn type_of(&self) -> ValueType {
        ValueType::Array(Box::new(self.element.clone()))
    }

    fn read(&self) -> Value {
        panic!("定数添字一つのために大配列を丸ごと読んではならない")
    }

    fn write(&mut self, _value: &Value) {}

    fn read_at(&self, _index: usize) -> Option<Value> {
        self.element_reads.set(self.element_reads.get() + 1);
        let ValueType::Array(inner) = &self.element else {
            panic!("深い配列型")
        };
        Some(Value::array((**inner).clone(), Vec::new()))
    }

    fn len(&self) -> Option<usize> {
        Some(200_000)
    }
}

#[test]
fn 深い要素型の部分読みは未接触dummyへ型をcloneしない() {
    let mut element = ValueType::I64;
    for _ in 0..255 {
        element = ValueType::Array(Box::new(element));
    }
    let layout = HostLayout::new(vec![(
        "values".into(),
        HostItem::Value(ValueType::Array(Box::new(element.clone()))),
    )])
    .expect("外側を含めて深さ256");
    let program = prepare("values[0].len()", &layout).expect("prepare");
    let element_reads = Rc::new(Cell::new(0));
    let mut binding = 深い要素型の大配列束縛 {
        element,
        element_reads: element_reads.clone(),
    };
    let mut slots = [HostSlot::value("values", &mut binding)];
    let result = EmbeddingRunner::new()
        .run_without_functions(&program, &mut slots)
        .expect("cheap sentinelで部分読み");
    assert_eq!(整数の結果(result), 0);
    assert_eq!(element_reads.get(), 1);
}
