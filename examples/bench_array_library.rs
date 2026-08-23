//! 細粒度配列ライブラリの関数枠・alias・固定演算を小さく測る。
//!
//! `cargo run --release --example bench_array_library`。`ROUNDS` で反復数を変えられる。

use std::hint::black_box;
use std::time::{Duration, Instant};
use vaak::interp::Eval;

const BINARY: &str = include_str!("../stdlib/array/i64/search_binary.vaak");
const FENWICK: &str = include_str!("../stdlib/ds/fenwick_i64.vaak");
const FENWICK_FLAT: &str = include_str!("../stdlib/ds/fenwick_i64_flat.vaak");

struct Case {
    name: &'static str,
    library: &'static str,
    body: &'static str,
}

fn prepare(case: &Case) -> (vaak::ast::Program, vaak::vm::Program2) {
    let src = format!("{}\n{}", case.library, case.body);
    let prog =
        vaak::parser::parse(&src).unwrap_or_else(|e| panic!("{}: 構文: {}", case.name, e.msg));
    let errors: Vec<_> = vaak::check::check(&prog)
        .into_iter()
        .chain(vaak::types::check_types(&prog))
        .map(|e| e.msg)
        .collect();
    assert!(errors.is_empty(), "{}: {errors:?}", case.name);
    let vm = vaak::vm::compile(&prog).unwrap_or_else(|e| panic!("{}: VM: {}", case.name, e.msg));
    (prog, vm)
}

fn value(result: Result<Eval, impl std::fmt::Debug>) -> i128 {
    match result {
        Ok(Eval::Value(v)) => v.as_int().expect("整数結果"),
        Ok(other) => panic!("値でない結果: {other:?}"),
        Err(e) => panic!("実行時: {e:?}"),
    }
}

fn measure(mut run: impl FnMut() -> i128, rounds: u32) -> (Duration, i128) {
    let expected = black_box(run());
    let start = Instant::now();
    for _ in 0..rounds {
        assert_eq!(black_box(run()), expected);
    }
    (start.elapsed() / rounds, expected)
}

fn main() {
    let rounds = std::env::var("ROUNDS")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(5);

    let cases = [
        Case {
            name: "lenを直接読む",
            library: "",
            body: r#"
                let xs : i64 array := new i64 array(1024, 0);
                var acc := 0;
                nfor (i, 0, 20000) { acc += xs.len(); };
                acc mod 251
            "#,
        },
        Case {
            name: "alias関数枠からlenを読む",
            library: "",
            body: r#"
                fn array_probe_len (xs : i64 array alias) { xs.len() } -> i64;
                let xs : i64 array := new i64 array(1024, 0);
                var acc := 0;
                nfor (i, 0, 20000) { acc += array_probe_len(xs); };
                acc mod 251
            "#,
        },
        Case {
            name: "二分探索をその場に書く",
            library: "",
            body: r#"
                var xs : i64 array := new i64 array(512, 0);
                nfor (i, 0, xs.len()) { xs[i] := i * 2; };
                var acc := 0;
                nfor (q, 0, 2000) {
                    let target := (q * 37) mod 1024;
                    var lo := 0;
                    var hi := xs.len();
                    while (lo < hi) {
                        let mid := lo + (hi - lo) / 2;
                        if ((xs[mid] ?? 0) < target) lo := mid + 1;
                        else hi := mid;
                        fi;
                    };
                    acc += lo;
                };
                acc mod 251
            "#,
        },
        Case {
            name: "二分探索をalias関数で呼ぶ",
            library: BINARY,
            body: r#"
                var xs : i64 array := new i64 array(512, 0);
                nfor (i, 0, xs.len()) { xs[i] := i * 2; };
                var acc := 0;
                nfor (q, 0, 2000) {
                    let target := (q * 37) mod 1024;
                    acc += array_i64_lower_bound(xs, target) ?? 0;
                };
                acc mod 251
            "#,
        },
        Case {
            name: "prefix和を毎回走査する",
            library: "",
            body: r#"
                var xs : i64 array := new i64 array(512, 0);
                nfor (i, 0, xs.len()) { xs[i] := (i * 17) mod 101 - 50; };
                var acc := 0;
                nfor (q, 0, 2048) {
                    let last := (q * 29) mod 513;
                    var sum := 0;
                    nfor (i, 0, last) { sum += xs[i] ?? 0; };
                    acc += sum;
                };
                acc mod 251
            "#,
        },
        Case {
            name: "Fenwick固定加算でprefix和",
            library: FENWICK,
            body: r#"
                var tree := fenwick_i64_new(512) ?? new FenwickI64(data := [0]);
                nfor (i, 0, 512) {
                    fenwick_i64_add(tree, i, (i * 17) mod 101 - 50) ?? false;
                };
                var acc := 0;
                nfor (q, 0, 2048) {
                    let last := (q * 29) mod 513;
                    acc += fenwick_i64_prefix_sum(tree, last) ?? 0;
                };
                acc mod 251
            "#,
        },
        Case {
            name: "flat Fenwickでprefix和",
            library: FENWICK_FLAT,
            body: r#"
                var data := fenwick_i64_flat_new(512) ?? [0];
                nfor (i, 0, 512) {
                    fenwick_i64_flat_add(data, i, (i * 17) mod 101 - 50) ?? false;
                };
                var acc := 0;
                nfor (q, 0, 2048) {
                    let last := (q * 29) mod 513;
                    acc += fenwick_i64_flat_prefix_sum(data, last) ?? 0;
                };
                acc mod 251
            "#,
        },
    ];

    println!("rounds={rounds}（parse/check/VM compile は計測外）");
    println!("{:<34} {:>12} {:>12} {:>8}", "case", "tree", "VM", "value");
    for case in &cases {
        let (prog, vm) = prepare(case);
        let (tree, a) = measure(
            || value(vaak::interp::Interp::new().run(&prog).map_err(|e| e.msg)),
            rounds,
        );
        let (vm_time, b) = measure(|| value(vaak::vm::run_program(&vm)), rounds);
        assert_eq!(a, b, "{}: 参照とVM", case.name);
        println!("{:<34} {:>12?} {:>12?} {:>8}", case.name, tree, vm_time, a);
    }
}
