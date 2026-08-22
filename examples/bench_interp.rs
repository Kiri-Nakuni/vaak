//! 木を辿る実装の速度。**hash を足す前後で比べる**ため。
use std::time::Instant;

/// **最小値で比べる。** 中央値より外乱に強い
fn run(src: &str, reps: u32) -> f64 {
    let prog = vaak::parser::parse(src).expect("構文");
    let mut best = f64::MAX;
    for round in 0..7 {
        let t = Instant::now();
        for _ in 0..reps {
            let mut it = vaak::interp::Interp::new();
            let _ = it.run(&prog);
        }
        let ns = t.elapsed().as_secs_f64() * 1e9 / reps as f64;
        if round > 1 && ns < best {
            best = ns;
        }
    }
    best
}

fn main() {
    let cases: &[(&str, &str, u32)] = &[
        ("算術ループ", "var s := 0; var i := 0; while (i < 20000) { s += i * 3; i += 1; }; s", 20),
        ("関数呼び", "fn f (a : i64) { a * 2 + 1 } -> i64; var s := 0; var i := 0; while (i < 20000) { s += f(i); i += 1; }; s", 20),
        ("配列", "var a := new i64 array(1024, 0); var i := 0; while (i < 20000) { a[i & 1023] := i; i += 1; }; a[7]", 20),
        ("写像", "var m := new i64 i64 map ( ); var i := 0; while (i < 4000) { m[i] := i; i += 1; }; m[7]", 20),
        ("構造体", "struct P { var x : i64 := 0; var y : i64 := 0; }; var p := new P (); var i := 0; while (i < 20000) { p.x += i; p.y := p.x; i += 1; }; p.y", 20),
        ("起動のみ", "1", 20000),
    ];
    for (n, src, reps) in cases {
        println!("{n:<12} {:>12.0} ns", run(src, *reps));
    }
}
