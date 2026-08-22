#[path = "../examples/experiments/embedding_host.rs"]
mod embedding_host;

use embedding_host::{
    apply_batch, benchmark_vm_calls, check_host_scope, commit_line_replacement, BatchVm,
    BulkLineBreaker, CallbackVm, Command, LayoutResult, LineBox, NodeListSnapshot,
    NodeOpsLineBreaker, TexState, HOOKS_PER_NODE,
};

const SP: i64 = 65536;

#[test]
fn バッチ版は成功した命令列だけを一括適用する() {
    let mut vm = BatchVm::compile().expect("埋め込みプログラムを組み立てられる");
    let mut state = TexState::sample(41, 10 * SP);

    let commands = vm.plan(&state, false).expect("命令列を返せる");
    assert_eq!(commands.len(), 3);
    assert_eq!(
        state,
        TexState::sample(41, 10 * SP),
        "plan は snapshot を読むだけ"
    );

    apply_batch(&mut state, &commands).expect("検証済み命令を適用できる");
    assert_eq!(state.count, vec![42]);
    assert_eq!(state.dimen, vec![52 * SP]);
    assert_eq!(state.emitted, vec!["batch committed"]);
}

#[test]
fn バッチ版は後続失敗なら途中命令を適用しない() {
    let mut vm = BatchVm::compile().expect("埋め込みプログラムを組み立てられる");
    let state = TexState::sample(41, 10 * SP);
    let before = state.clone();

    assert!(
        vm.plan(&state, true).is_err(),
        "命令を三つ作った後で失敗する"
    );
    assert_eq!(state, before, "返り値が無ければホストは apply しない");
}

#[test]
fn ホスト側の命令検証も全体を原子的に扱う() {
    let mut state = TexState::sample(1, SP);
    let before = state.clone();
    let commands = vec![
        Command {
            kind: 1,
            index: 0,
            value: 2,
            text: String::new(),
        },
        Command {
            kind: 1,
            index: 99,
            value: 3,
            text: String::new(),
        },
    ];

    assert!(apply_batch(&mut state, &commands).is_err());
    assert_eq!(state, before, "先頭の有効な命令も commit されない");
}

#[test]
fn 同期ホスト関数は後続失敗でも途中副作用を残す() {
    let mut vm = CallbackVm::compile().expect("callback 比較版を組み立てられる");
    let mut state = TexState::sample(41, 10 * SP);

    assert!(
        vm.run(&mut state, true).is_err(),
        "三 callback の後で失敗する"
    );
    assert_eq!(state.count, vec![42]);
    assert_eq!(state.dimen, vec![52 * SP]);
    assert_eq!(state.emitted, vec!["callback ran"]);
}

#[test]
fn vmは一度組み立てたプログラムを異なるsnapshotで繰り返せる() {
    let mut vm = BatchVm::compile().expect("組み立ては一度だけ");

    let first = vm
        .plan(&TexState::sample(1, 2 * SP), false)
        .expect("一回目");
    let second = vm
        .plan(&TexState::sample(9, 4 * SP), false)
        .expect("二回目");

    assert_eq!(first[0].value, 2);
    assert_eq!(first[1].value, 4 * SP);
    assert_eq!(second[0].value, 10);
    assert_eq!(second[1].value, 14 * SP);
}

#[test]
fn ホスト値は関数へalias引数で明示する() {
    let hidden = check_host_scope("fn read () { count[0] } -> i32; read()").unwrap_err();
    assert!(hidden.contains("C-96"), "理由を規則名で知らせる: {hidden}");

    check_host_scope("fn read (c : i32 array alias) { c[0] } -> i32; read(count)")
        .expect("alias 引数へ明示すれば関数から読める");
}

#[test]
fn native組版phaseは一回入りnodeopsを高頻度に内部呼出しする() {
    let nodes = NodeListSnapshot::paragraph(96, 80);
    let mut kernel = NodeOpsLineBreaker::compile().expect("native NodeOps kernel");

    let result = kernel
        .layout(&nodes)
        .expect("handle で node arena を引ける");

    assert_eq!(
        kernel.phase_calls, 1,
        "host から Vaak へ入るのは段落ごとに一回"
    );
    assert!(
        kernel.nodeops_calls >= nodes.len() as u64 * HOOKS_PER_NODE as u64,
        "幅の数え直し分を除いても 21 internal calls/node"
    );
    assert_eq!(kernel.nodeops_calls, result.logical_hook_calls as u64);
    assert_eq!(result.lines.last().unwrap().end, nodes.len() as i64);
}

#[test]
fn 外向きwasm車線を模したbulk入力は論理hookをprobe内に保つ() {
    let nodes = NodeListSnapshot::paragraph(96, 80);
    let mut native = NodeOpsLineBreaker::compile().expect("native NodeOps kernel");
    let mut bulk = BulkLineBreaker::compile().expect("bulk mirror kernel");

    let native_result = native.layout(&nodes).expect("native 行分割");
    let bulk_result = bulk.layout(&nodes).expect("bulk 行分割");

    assert_eq!(native_result.lines, bulk_result.lines);
    assert_eq!(bulk.phase_calls, 1);
    assert_eq!(
        bulk_result.logical_hook_calls,
        nodes.len() as i64 * HOOKS_PER_NODE
    );
}

#[test]
fn 行分割replace結果は全体検証後だけcommitする() {
    let nodes = NodeListSnapshot::paragraph(12, 40);
    let mut current = vec![LineBox {
        start: 0,
        end: 1,
        natural: nodes.width[0],
    }];
    let before = current.clone();
    let invalid = LayoutResult {
        lines: vec![LineBox {
            start: 0,
            end: nodes.len() as i64,
            natural: -1,
        }],
        logical_hook_calls: 0,
    };

    assert!(commit_line_replacement(&mut current, &nodes, &invalid).is_err());
    assert_eq!(
        current, before,
        "不正な replace result では既存 line list を保つ"
    );

    let mut kernel = NodeOpsLineBreaker::compile().expect("native NodeOps kernel");
    let valid = kernel.layout(&nodes).expect("妥当な replace result");
    commit_line_replacement(&mut current, &nodes, &valid).expect("検証後に交換できる");
    assert_eq!(current, valid.lines);
}

#[test]
fn vm呼出し測定は一引数と二引数のhostcall回数を数える() {
    let measured = benchmark_vm_calls(5_000, 1).expect("短い回帰用測定を走らせられる");
    assert_eq!(measured.host1_calls, 5_000);
    assert_eq!(measured.host2_calls, 5_000);
    assert_eq!(measured.iterations, 5_000);
    assert_eq!(measured.samples, 1);
    assert!(measured.local_extra_ns.is_finite());
    assert!(measured.host1_extra_ns.is_finite());
    assert!(measured.host2_extra_ns.is_finite());
}
