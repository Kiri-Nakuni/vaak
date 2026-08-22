//! 入れ子代入の大きさに対する伸びを測る。
//!
//! `cargo run --release --example bench_vm_place`。
//! 一要素ごとに根配列を深く複製すると、N を二倍にした時間は約四倍になる。
//! 解決済み Place なら末端だけを変更するため、約二倍に留まる。

use std::time::{Duration, Instant};

fn source(n: usize) -> String {
    format!(
        "struct Item {{ var value : i64 := 0; }};
         var items : Item array := new Item array({n}, new Item());
         nfor (i, 0, {n}) {{ items[i].value := i; }};
         items[{last}].value",
        last = n - 1,
    )
}

fn measure(n: usize) -> Duration {
    let syntax = vaak::parser::parse(&source(n)).expect("構文");
    let program = vaak::vm::compile(&syntax).expect("VM 組み立て");
    let mut runner = vaak::vm::Runner::new();
    let mut best = Duration::MAX;
    for _ in 0..3 {
        let start = Instant::now();
        let (answer, _) = runner.run(&program, Vec::new()).expect("VM 実行");
        std::hint::black_box(answer);
        best = best.min(start.elapsed());
    }
    best
}

fn main() {
    let mut previous = None;
    for n in [10_000, 20_000, 40_000, 80_000] {
        let elapsed = measure(n);
        let ratio = previous.map(|p: Duration| elapsed.as_secs_f64() / p.as_secs_f64());
        match ratio {
            Some(ratio) => println!(
                "N={n:>5}: {:>9.3} ms, 前回比 {ratio:.2}x",
                elapsed.as_secs_f64() * 1_000.0
            ),
            None => println!("N={n:>5}: {:>9.3} ms", elapsed.as_secs_f64() * 1_000.0),
        }
        previous = Some(elapsed);
    }
}
