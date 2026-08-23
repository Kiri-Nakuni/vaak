#[path = "experiments/embedding_host.rs"]
mod embedding_host;

use embedding_host::{
    apply_batch, benchmark_vm_calls, BatchVm, BulkLineBreaker, CallbackVm, NodeListSnapshot,
    NodeOpsLineBreaker, TexState, VmCallBenchmark,
};
use std::time::Instant;

fn main() -> Result<(), String> {
    let mut batch_vm = BatchVm::compile()?;
    let mut state = TexState::sample(41, 10 * 65536);

    let commands = batch_vm.plan(&state, false)?;
    println!("plan 後（未適用）: {state:?}");
    apply_batch(&mut state, &commands)?;
    println!("batch 適用後:      {state:?}");

    let before_failure = state.clone();
    assert!(batch_vm.plan(&state, true).is_err());
    assert_eq!(state, before_failure);
    println!("batch 失敗後:      変化なし");

    let mut callback_vm = CallbackVm::compile()?;
    let mut callback_state = TexState::sample(41, 10 * 65536);
    assert!(callback_vm.run(&mut callback_state, true).is_err());
    println!("callback 失敗後:   {callback_state:?}");

    // Program2 と Runner は上で一度だけ作ったものを使い続ける。
    let started = Instant::now();
    for n in 0..10_000 {
        let snapshot = TexState::sample(n, i64::from(n) * 65536);
        let _ = batch_vm.plan(&snapshot, false)?;
    }
    println!("compile once / 10,000 runs: {:?}", started.elapsed());

    // node list / linebreak は同一 process の内蔵 Vaak が主経路。
    // host は段落ごとに一度入り、Vaak が整数 handle で NodeOps を引く。
    let paragraph = NodeListSnapshot::paragraph(5_000, 80);
    let mut native = NodeOpsLineBreaker::compile()?;
    let native_started = Instant::now();
    let native_result = native.layout(&paragraph)?;
    let native_elapsed = native_started.elapsed();

    // 外向き WASM lane が必要な場合のデータ形を native VM で模す鏡像。
    // 実 WASM timing ではない。21 logical hooks/node は probe 内に保つ。
    let mut bulk = BulkLineBreaker::compile()?;
    let bulk_started = Instant::now();
    let bulk_result = bulk.layout(&paragraph)?;
    let bulk_elapsed = bulk_started.elapsed();
    assert_eq!(native_result.lines, bulk_result.lines);

    println!(
        "native linebreak: {} nodes, phase entries {}, internal NodeOps {}, {:?}",
        paragraph.len(),
        native.phase_calls,
        native.nodeops_calls,
        native_elapsed,
    );
    println!(
        "modeled WASM bulk shape (native VM): phase entries {}, logical hooks {}, {:?}",
        bulk.phase_calls, bulk_result.logical_hook_calls, bulk_elapsed,
    );

    let measured = benchmark_vm_calls(1_000_000, 5)?;
    let empty = VmCallBenchmark::ns_per_iteration(measured.empty_loop, measured.iterations);
    let local = VmCallBenchmark::ns_per_iteration(measured.local_call_1, measured.iterations);
    let host1 = VmCallBenchmark::ns_per_iteration(measured.host_call_1, measured.iterations);
    let host2 = VmCallBenchmark::ns_per_iteration(measured.host_call_2, measured.iterations);
    println!(
        "VM median ({} x {}): empty {:.1}, local1 {:.1}, HostCall1 {:.1}, HostCall2 {:.1} ns/iteration",
        measured.iterations, measured.samples, empty, local, host1, host2,
    );
    println!(
        "paired extra: local {:.1}, HostCall1 {:.1}, HostCall2 {:.1} ns/call",
        measured.local_extra_ns, measured.host1_extra_ns, measured.host2_extra_ns,
    );
    println!(
        "21 calls/node paired extra: HostCall1 {:.1} ns, HostCall2 {:.1} ns; absolute host/local ratios {:.3}/{:.3}",
        measured.host1_extra_ns * 21.0,
        measured.host2_extra_ns * 21.0,
        host1 / local,
        host2 / local,
    );
    Ok(())
}
