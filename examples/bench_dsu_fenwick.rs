//! DSU派生型とFenwick順位選択の、操作中allocationを抑えたworkloadを測る。
//!
//! `cargo run --release --locked --example bench_dsu_fenwick`。
//! `ROUNDS`で反復数を変えられる。parse/check/VM compileは計測外。

use std::collections::BTreeMap;
use std::hint::black_box;
use std::time::{Duration, Instant};
use vaak::interp::Eval;

const DSU: &str = include_str!("../stdlib/ds/dsu_i64.vaak");
const ROLLBACK: &str = include_str!("../stdlib/ds/rollback_dsu_i64.vaak");
const WEIGHTED: &str = include_str!("../stdlib/ds/weighted_dsu_i64.vaak");
const COUNT: &str = include_str!("../stdlib/ds/fenwick_count_i64.vaak");

struct Case {
    name: &'static str,
    library: &'static str,
    body: &'static str,
    paired_checksum: Option<&'static str>,
}

fn prepare(case: &Case) -> (vaak::ast::Program, vaak::vm::Program2) {
    let source = format!("{}\n{}", case.library, case.body);
    let program = vaak::parser::parse(&source)
        .unwrap_or_else(|error| panic!("{}: 構文: {}", case.name, error.msg));
    let errors: Vec<_> = vaak::check::check(&program)
        .into_iter()
        .chain(vaak::types::check_types(&program))
        .map(|error| error.msg)
        .collect();
    assert!(errors.is_empty(), "{}: {errors:?}", case.name);
    let vm = vaak::vm::compile(&program)
        .unwrap_or_else(|error| panic!("{}: VM: {}", case.name, error.msg));
    (program, vm)
}

fn value(result: Result<Eval, impl std::fmt::Debug>) -> i128 {
    match result {
        Ok(Eval::Value(value)) => value.as_int().expect("整数結果"),
        Ok(other) => panic!("値でない結果: {other:?}"),
        Err(error) => panic!("実行時: {error:?}"),
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
        .and_then(|source| source.parse::<u32>().ok())
        .filter(|rounds| *rounds > 0)
        .unwrap_or(3);

    let cases = [
        Case {
            name: "通常DSUを構築してfindを反復",
            library: DSU,
            body: r#"
                var dsu := dsu_i64_new(256) ?? new DsuI64(parent_or_size := [0]);
                nfor (i, 1, 256) { dsu_i64_merge(dsu, i - 1, i) ?? -1; };
                var acc := 0;
                nfor (q, 0, 4096) {
                    acc += dsu_i64_leader(dsu, (q * 37) mod 256) ?? -1;
                };
                acc += dsu_i64_size(dsu, 173) ?? 0;
                acc mod 251
            "#,
            paired_checksum: None,
        },
        Case {
            name: "rollback DSUのhistory容量を再利用",
            library: ROLLBACK,
            body: r#"
                var dsu := rollback_dsu_i64_new(128) ??
                    new RollbackDsuI64(parent_or_size := [0], history := [0]);
                nfor (i, 1, 128) { rollback_dsu_i64_merge(dsu, i - 1, i) ?? -1; };
                rollback_dsu_i64_rollback_to(dsu, 0) ?? false;
                var acc := 0;
                nfor (round, 0, 8) {
                    nfor (i, 1, 128) { rollback_dsu_i64_merge(dsu, i - 1, i) ?? -1; };
                    acc += rollback_dsu_i64_size(dsu, (round * 17) mod 128) ?? 0;
                    rollback_dsu_i64_rollback_to(dsu, 0) ?? false;
                };
                acc mod 251
            "#,
            paired_checksum: None,
        },
        Case {
            name: "weighted DSUを構築してdiffを反復",
            library: WEIGHTED,
            body: r#"
                var dsu := weighted_dsu_i64_new(128) ??
                    new WeightedDsuI64(parent_or_size := [0], weight_to_parent := [0]);
                nfor (i, 1, 128) {
                    weighted_dsu_i64_merge(dsu, i - 1, i, (i * 17) mod 101 - 50) ?? -1;
                };
                var acc := 0;
                nfor (q, 0, 2048) {
                    let a := (q * 29) mod 128;
                    let b := (q * 47) mod 128;
                    acc += weighted_dsu_i64_diff(dsu, a, b) ?? 0;
                };
                acc mod 251
            "#,
            paired_checksum: None,
        },
        Case {
            name: "配列走査で累積順位を選ぶ",
            library: "",
            body: r#"
                var counts : i64 array := new i64 array(256, 0);
                var total := 0;
                nfor (i, 0, counts.len()) {
                    counts[i] := (i * 17) mod 5;
                    total += counts[i] ?? 0;
                };
                var acc := 0;
                nfor (q, 0, 1024) {
                    let target := (q * 37) mod total + 1;
                    var at := 0;
                    var prefix := counts[0] ?? 0;
                    while (prefix < target) {
                        at += 1;
                        prefix += counts[at] ?? 0;
                    };
                    acc += at;
                };
                acc mod 251
            "#,
            paired_checksum: Some("累積順位"),
        },
        Case {
            name: "count Fenwickで累積順位を選ぶ",
            library: COUNT,
            body: r#"
                var counts : i64 array := new i64 array(256, 0);
                nfor (i, 0, counts.len()) { counts[i] := (i * 17) mod 5; };
                let tree := fenwick_count_i64_from_counts(counts) ??
                    new FenwickCountI64(data := [0], total := 0);
                var acc := 0;
                nfor (q, 0, 1024) {
                    let target := (q * 37) mod fenwick_count_i64_total(tree) + 1;
                    acc += fenwick_count_i64_lower_bound(tree, target) ?? 0;
                };
                acc mod 251
            "#,
            paired_checksum: Some("累積順位"),
        },
    ];

    println!("rounds={rounds}（parse/check/VM compileは計測外）");
    println!("{:<38} {:>12} {:>12} {:>8}", "case", "tree", "VM", "value");
    let mut paired = BTreeMap::new();
    for case in &cases {
        let (program, vm) = prepare(case);
        let (tree, a) = measure(
            || {
                value(
                    vaak::interp::Interp::new()
                        .run(&program)
                        .map_err(|error| error.msg),
                )
            },
            rounds,
        );
        let (vm_time, b) = measure(|| value(vaak::vm::run_program(&vm)), rounds);
        assert_eq!(a, b, "{}: 参照とVM", case.name);
        if let Some(group) = case.paired_checksum {
            if let Some(previous) = paired.insert(group, a) {
                assert_eq!(previous, a, "{group}: paired workloadのchecksum");
            }
        }
        println!("{:<38} {:>12?} {:>12?} {:>8}", case.name, tree, vm_time, a);
    }
}
