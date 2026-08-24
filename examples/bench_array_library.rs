//! 細粒度配列ライブラリの関数枠・alias・固定演算を小さく測る。
//!
//! `cargo run --release --example bench_array_library`。`ROUNDS` で反復数を変えられる。

use std::hint::black_box;
use std::time::{Duration, Instant};
use vaak::interp::Eval;

const BINARY: &str = include_str!("../stdlib/array/i64/search_binary.vaak");
const FENWICK: &str = include_str!("../stdlib/ds/fenwick_i64.vaak");
const FENWICK_FLAT: &str = include_str!("../stdlib/ds/fenwick_i64_flat.vaak");
const HEAP: &str = include_str!("../stdlib/ds/heap_i64.vaak");
const DEQUE: &str = include_str!("../stdlib/ds/deque_i64.vaak");
const SEGTREE: &str = include_str!("../stdlib/ds/segtree_i64.vaak");
const LAZY_SEGTREE: &str = include_str!("../stdlib/ds/lazy_segtree_i64.vaak");

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
        Case {
            name: "任意range和を毎回走査する",
            library: "",
            body: r#"
                var xs : i64 array := new i64 array(512, 0);
                nfor (i, 0, xs.len()) { xs[i] := (i * 17) mod 101 - 50; };
                var acc := 0;
                nfor (q, 0, 2048) {
                    var first := (q * 29) mod 513;
                    var last := (q * 47) mod 513;
                    if (first > last) {
                        let saved := first; first := last; last := saved;
                    } fi;
                    var i := first;
                    while (i < last) { acc += xs[i] ?? 0; i += 1; };
                };
                acc mod 251
            "#,
        },
        Case {
            name: "flat segment和をその場に書く",
            library: "",
            body: r#"
                var data : i64 array := new i64 array(1024, 0);
                nfor (i, 0, 512) { data[512 + i] := (i * 17) mod 101 - 50; };
                var node := 512;
                while (node > 1) {
                    node -= 1; data[node] := (data[node * 2] ?? 0) + (data[node * 2 + 1] ?? 0);
                };
                var acc := 0;
                nfor (q, 0, 2048) {
                    var first := (q * 29) mod 513;
                    var last := (q * 47) mod 513;
                    if (first > last) {
                        let saved := first; first := last; last := saved;
                    } fi;
                    var left := first + 512;
                    var right := last + 512;
                    while (left < right) {
                        if ((left & 1) == 1) { acc += data[left] ?? 0; left += 1; } fi;
                        if ((right & 1) == 1) { right -= 1; acc += data[right] ?? 0; } fi;
                        left /= 2; right /= 2;
                    };
                };
                acc mod 251
            "#,
        },
        Case {
            name: "SumSegtreeI64でrange和",
            library: SEGTREE,
            body: r#"
                var values : i64 array := new i64 array(512, 0);
                nfor (i, 0, values.len()) { values[i] := (i * 17) mod 101 - 50; };
                let tree := sum_segtree_i64_from(values) ??
                    new SumSegtreeI64(length := 0, size := 1, data := [0, 0]);
                var acc := 0;
                nfor (q, 0, 2048) {
                    var first := (q * 29) mod 513;
                    var last := (q * 47) mod 513;
                    if (first > last) {
                        let saved := first; first := last; last := saved;
                    } fi;
                    acc += sum_segtree_i64_prod(tree, first, last) ?? 0;
                };
                acc mod 251
            "#,
        },
        Case {
            name: "配列走査でrange add/sum",
            library: "",
            body: r#"
                var values : i64 array := new i64 array(128, 0);
                nfor (i, 0, values.len()) { values[i] := (i * 17) mod 101 - 50; };
                var acc := 0;
                nfor (q, 0, 128) {
                    let first := (q * 19) mod 97;
                    let last := first + 32;
                    let delta := (q * 31) mod 101 - 50;
                    var i := first;
                    while (i < last) { values[i] += delta; i += 1; };
                    i := first;
                    while (i < last) { acc += values[i] ?? 0; i += 1; };
                };
                acc mod 251
            "#,
        },
        Case {
            name: "lazy treeでrange add/sum",
            library: LAZY_SEGTREE,
            body: r#"
                var values : i64 array := new i64 array(128, 0);
                nfor (i, 0, values.len()) { values[i] := (i * 17) mod 101 - 50; };
                var tree := range_add_sum_segtree_i64_from(values) ??
                    new RangeAddSumSegtreeI64(length := 0, size := 1, data := [0, 0], lazy := [0, 0]);
                var acc := 0;
                nfor (q, 0, 128) {
                    let first := (q * 19) mod 97;
                    let last := first + 32;
                    let delta := (q * 31) mod 101 - 50;
                    range_add_sum_segtree_i64_range_add(tree, first, last, delta) ?? false;
                    acc += range_add_sum_segtree_i64_prod(tree, first, last) ?? 0;
                };
                acc mod 251
            "#,
        },
        Case {
            name: "min heapを往復する",
            library: HEAP,
            body: r#"
                var heap := min_heap_i64_new() ?? new MinHeapI64(data := [0]);
                nfor (i, 0, 1024) {
                    min_heap_i64_push(heap, (i * 1009) mod 2039 - 1019) ?? false;
                };
                var acc := 0;
                nfor (i, 0, 1024) {
                    acc += min_heap_i64_pop(heap) ?? 0;
                };
                acc mod 251
            "#,
        },
        Case {
            name: "配列先頭removeでFIFO",
            library: "",
            body: r#"
                var queue : i64 array := new i64 array(0, 0);
                nfor (i, 0, 1024) { queue.push((i * 37) mod 101); };
                var acc := 0;
                nfor (i, 0, 1024) { acc += queue.remove(0) ?? 0; };
                acc mod 251
            "#,
        },
        Case {
            name: "ring dequeでFIFO",
            library: DEQUE,
            body: r#"
                var queue := deque_i64_new() ?? new DequeI64(data := [0], head := 0, size := 0);
                nfor (i, 0, 1024) {
                    deque_i64_push_back(queue, (i * 37) mod 101) ?? false;
                };
                var acc := 0;
                nfor (i, 0, 1024) { acc += deque_i64_pop_front(queue) ?? 0; };
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
