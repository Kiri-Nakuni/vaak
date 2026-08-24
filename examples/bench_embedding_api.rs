//! PraTeXが使うprepared embedding境界を、prepare・検査済みtoken・再束縛・raw VMへ分ける。
//!
//! ```text
//! cargo run --release --locked --example bench_embedding_api -- CASE [ITERATIONS]
//! perf stat -r 5 -e task-clock,cycles,instructions,branches,branch-misses,cache-misses \
//!   target/release/examples/bench_embedding_api CASE ITERATIONS
//! ```

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;
use vaak::ast::{HostItem, ValueType};
use vaak::embedding::{prepare, EmbeddingRunner, HostLayout};
use vaak::interp::Eval;
use vaak::value::Value;

struct CountingAllocator;

static COUNT_ALLOCATIONS: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static ALLOCATED_BYTES: AtomicU64 = AtomicU64::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNT_ALLOCATIONS.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        System.alloc(layout)
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if COUNT_ALLOCATIONS.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        System.alloc_zeroed(layout)
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        System.dealloc(pointer, layout);
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if COUNT_ALLOCATIONS.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            ALLOCATED_BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
        }
        System.realloc(pointer, layout, new_size)
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

const ARRAY_SOURCE: &str = "count[5] * 2 + count[6]";
const SCALAR_SOURCE: &str = "n + 1";

fn array_layout() -> HostLayout {
    let registers = ValueType::Array(Box::new(ValueType::I32));
    HostLayout::new(vec![
        ("count".into(), HostItem::Value(registers.clone())),
        ("dimen".into(), HostItem::Value(registers)),
    ])
    .expect("array layout")
}

fn scalar_layout() -> HostLayout {
    HostLayout::new(vec![("n".into(), HostItem::Value(ValueType::I64))]).expect("scalar layout")
}

fn register_values() -> Vec<Value> {
    let mut count = vec![Value::I32(0); 256];
    count[5] = Value::I32(20);
    count[6] = Value::I32(2);
    vec![
        Value::array(ValueType::I32, count),
        Value::array(ValueType::I32, vec![Value::I32(0); 256]),
    ]
}

fn integer(eval: Eval) -> i128 {
    let Eval::Value(value) = eval else {
        panic!("整数値でない: {eval:?}");
    };
    value.as_int().expect("整数")
}

fn measure(iterations: u64, mut run: impl FnMut() -> i128) {
    let expected = black_box(run());
    assert_eq!(black_box(run()), expected, "warm result");
    let count_allocations = std::env::var_os("VAAK_BENCH_NO_ALLOC_STATS").is_none();
    ALLOCATIONS.store(0, Ordering::Relaxed);
    ALLOCATED_BYTES.store(0, Ordering::Relaxed);
    COUNT_ALLOCATIONS.store(count_allocations, Ordering::Relaxed);
    let started = Instant::now();
    let mut checksum = 0_i128;
    for _ in 0..iterations {
        checksum = checksum.wrapping_add(black_box(run()));
    }
    let elapsed = started.elapsed();
    COUNT_ALLOCATIONS.store(false, Ordering::Relaxed);
    let allocations = ALLOCATIONS.load(Ordering::Relaxed);
    let allocated_bytes = ALLOCATED_BYTES.load(Ordering::Relaxed);
    println!("iterations={iterations}");
    println!("elapsed={elapsed:?}");
    println!(
        "ns/iteration={:.3}",
        elapsed.as_nanos() as f64 / iterations as f64
    );
    println!("checksum={checksum}");
    if count_allocations {
        println!(
            "allocations={allocations} ({:.3}/iteration)",
            allocations as f64 / iterations as f64
        );
        println!(
            "allocated_bytes={allocated_bytes} ({:.3}/iteration)",
            allocated_bytes as f64 / iterations as f64
        );
    } else {
        println!("allocations=disabled");
    }
}

fn prepare_array(iterations: u64) {
    let layout = array_layout();
    measure(iterations, || {
        let program = prepare(black_box(ARRAY_SOURCE), black_box(&layout)).expect("prepare");
        black_box(program.source().len() as i128)
    });
}

fn validate_array(iterations: u64) {
    let layout = array_layout();
    let program = prepare(ARRAY_SOURCE, &layout).expect("prepare");
    let mut values = Some(register_values());
    measure(iterations, || {
        let token = program
            .host_values(values.take().expect("values"))
            .expect("host values");
        let raw = token.into_values();
        let result = raw.len() as i128;
        values = Some(raw);
        result
    });
}

fn run_token_array(iterations: u64) {
    let layout = array_layout();
    let program = prepare(ARRAY_SOURCE, &layout).expect("prepare");
    let mut values = program.host_values(register_values()).expect("host values");
    let mut runner = EmbeddingRunner::new();
    measure(iterations, || {
        integer(
            runner
                .run_values_without_functions(&program, &mut values)
                .expect("layout")
                .expect("runtime"),
        )
    });
}

fn run_rebind_array(iterations: u64) {
    let layout = array_layout();
    let program = prepare(ARRAY_SOURCE, &layout).expect("prepare");
    let mut values = Some(register_values());
    let mut runner = EmbeddingRunner::new();
    measure(iterations, || {
        let mut token = program
            .host_values(values.take().expect("values"))
            .expect("host values");
        let result = integer(
            runner
                .run_values_without_functions(&program, &mut token)
                .expect("layout")
                .expect("runtime"),
        );
        values = Some(token.into_values());
        result
    });
}

fn run_raw_array(iterations: u64) {
    let layout = array_layout();
    let syntax = vaak::parser::parse(ARRAY_SOURCE).expect("parse");
    let program = vaak::vm::compile_with_host(&syntax, layout.entries()).expect("compile");
    let mut values = Some(register_values());
    let mut runner = vaak::vm::Runner::new();
    measure(iterations, || {
        let (eval, after) = runner
            .run(&program, values.take().expect("values"))
            .expect("runtime");
        values = Some(after);
        integer(eval)
    });
}

fn run_token_scalar(iterations: u64) {
    let layout = scalar_layout();
    let program = prepare(SCALAR_SOURCE, &layout).expect("prepare");
    let mut values = program
        .host_values(vec![Value::I64(41)])
        .expect("host values");
    let mut runner = EmbeddingRunner::new();
    measure(iterations, || {
        integer(
            runner
                .run_values_without_functions(&program, &mut values)
                .expect("layout")
                .expect("runtime"),
        )
    });
}

fn usage() -> ! {
    eprintln!(
        "CASE: prepare-array | validate-array | run-token-array | run-rebind-array | run-raw-array | run-token-scalar"
    );
    std::process::exit(2);
}

fn main() {
    let mut arguments = std::env::args().skip(1);
    let case = arguments.next().unwrap_or_else(|| usage());
    let default_iterations = if case == "prepare-array" {
        10_000
    } else {
        1_000_000
    };
    let iterations = arguments
        .next()
        .map(|value| value.parse::<u64>().expect("ITERATIONSは正の整数"))
        .unwrap_or(default_iterations);
    if iterations == 0 || arguments.next().is_some() {
        usage();
    }
    println!("case={case}");
    match case.as_str() {
        "prepare-array" => prepare_array(iterations),
        "validate-array" => validate_array(iterations),
        "run-token-array" => run_token_array(iterations),
        "run-rebind-array" => run_rebind_array(iterations),
        "run-raw-array" => run_raw_array(iterations),
        "run-token-scalar" => run_token_scalar(iterations),
        _ => usage(),
    }
}
