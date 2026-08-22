#![forbid(unsafe_code)]

#[path = "../examples/support/rust_lisp.rs"]
mod rust_lisp;

fn 両方で評価(source: &str) -> (i64, i64) {
    let naive = rust_lisp::naive::parse(source)
        .unwrap_or_else(|error| panic!("naive が `{source}` を解析できない: {error}"))
        .eval()
        .unwrap_or_else(|error| panic!("naive が `{source}` を評価できない: {error}"));
    let tuned = rust_lisp::tuned::parse(source)
        .unwrap_or_else(|error| panic!("tuned が `{source}` を解析できない: {error}"))
        .eval()
        .unwrap_or_else(|error| panic!("tuned が `{source}` を評価できない: {error}"));
    (naive, tuned)
}

#[test]
fn vaak版の主例と同じ四十二になる() {
    let vaak_example =
        std::fs::read_to_string("examples/vaak/06-LISP.vaak").expect("Vaak 版の LISP 例を読める");
    assert!(
        vaak_example.contains(rust_lisp::MAIN_PROGRAM),
        "比較入力が Vaak 版からずれている"
    );
    assert_eq!(両方で評価(rust_lisp::MAIN_PROGRAM), (42, 42));
}

#[test]
fn 算術と比較と空の畳み込みが一致する() {
    let cases = [
        ("(- (* 5 5) (/ 9 3) 2)", 20),
        ("(+)", 0),
        ("(*)", 1),
        ("(- 7)", -7),
        ("(/ 9)", 9),
        ("(/ -7 3)", -3),
        ("(= 4 (+ 2 2))", 1),
        ("(< 4 4)", 0),
        ("(do)", 0),
    ];
    for (source, expected) in cases {
        assert_eq!(両方で評価(source), (expected, expected), "{source}");
    }
}

#[test]
fn 字句束縛は内側から探して外側を壊さない() {
    let source = "(let x 6 (do (let x 40 (+ x 2)) (+ x 36)))";
    assert_eq!(両方で評価(source), (42, 42));
}

#[test]
fn 条件分岐は選ばない腕を評価しない() {
    assert_eq!(両方で評価("(if (< 3 -2) (/ 1 0) (/ 20 4))"), (5, 5));
}

#[test]
fn 大きな整数リテラルと算術はvaak同様に折り返す() {
    assert_eq!(
        両方で評価("(+ 9223372036854775807 1)"),
        (i64::MIN, i64::MIN)
    );
    assert_eq!(両方で評価("9223372036854775808"), (i64::MIN, i64::MIN));
}

#[test]
fn 日本語の名前も同じ入力上でinternできる() {
    assert_eq!(両方で評価("(let 答え 42 答え)"), (42, 42));
}

#[test]
fn 生成した深い束縛を両実装で評価できる() {
    let (source, expected) = rust_lisp::nested_let_source(96);
    assert_eq!(両方で評価(&source), (expected, expected));

    let tuned = rust_lisp::tuned::parse(&source).expect("調整版で解析できる");
    let mut evaluator = tuned.evaluator();
    for _ in 0..10 {
        assert_eq!(evaluator.eval(&tuned), Ok(expected));
    }
}

#[test]
fn 不正な入力は両方で拒む() {
    let cases = ["", "(", "(+ 1", "(+ 1))", "(unknown 1)", "(= 1 2 3)"];
    for source in cases {
        assert!(rust_lisp::naive::parse(source).is_err(), "naive: {source}");
        assert!(rust_lisp::tuned::parse(source).is_err(), "tuned: {source}");
    }
}

#[test]
fn 実行時の誤りも両方で拒む() {
    for source in ["missing", "(/ 1 0)"] {
        let naive = rust_lisp::naive::parse(source).expect("naive で解析できる");
        let tuned = rust_lisp::tuned::parse(source).expect("tuned で解析できる");
        assert!(naive.eval().is_err(), "naive: {source}");
        assert!(tuned.eval().is_err(), "tuned: {source}");
    }
}
