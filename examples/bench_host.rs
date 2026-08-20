use std::time::Instant;
use vaak::ast::ValueType;
use vaak::value::Value;

fn arr(n: usize) -> Value {
    vaak::interp::coerce_to(
        Value::Array { items: (0..n).map(|_| Value::I64(0)).collect(), elem: ValueType::I64 },
        &ValueType::Array(Box::new(ValueType::I32)),
    )
}

fn main() {
    for (n, b) in vaak::vm::sizes() { println!("{n:<26} {b:>3} バイト"); }

    let src = std::env::args().nth(1).unwrap_or_else(|| " count[5] * 2 + count[6] ".into());
    let n: u32 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(20000);
    let ex = vec![
        ("count".to_string(), ValueType::Array(Box::new(ValueType::I32))),
        ("dimen".to_string(), ValueType::Array(Box::new(ValueType::I32))),
    ];

    let t = Instant::now();
    let prog = vaak::parser::parse(&src).unwrap();
    let p2 = vaak::vm::compile_with_host(&prog, &ex).unwrap();
    println!("組み立て 1 回      {:>8.1} µs", t.elapsed().as_secs_f64() * 1e6);

    let mut host = Some(vec![arr(256), arr(256)]);
    let mut runner = vaak::vm::Runner::new();
    let t = Instant::now();
    for _ in 0..n {
        let h = host.take().unwrap();
        let (_ev, after) = runner.run(&p2, h).unwrap();
        host = Some(after);
    }
    println!("走らせるだけ       {:>8.3} µs/回", t.elapsed().as_secs_f64() * 1e6 / n as f64);

    let t = Instant::now();
    for _ in 0..n {
        let prog = vaak::parser::parse(&src).unwrap();
        std::hint::black_box(&prog);
    }
    println!("読み取り           {:>8.3} µs/回", t.elapsed().as_secs_f64() * 1e6 / n as f64);

    let t = Instant::now();
    for _ in 0..n {
        std::hint::black_box(arr(256));
    }
    println!("256 個作る         {:>8.3} µs/回", t.elapsed().as_secs_f64() * 1e6 / n as f64);
}
