//! TeX 風 DSL から通常計算だけを Vaak へ委譲する埋め込み実験。
//!
//! `BatchVm` は snapshot を読み、作用を表す `Command array` を返す。
//! `CallbackVm` は比較のため、同じ作用を同期 `HostFn` で直ちに実行する。

#![allow(dead_code)]

use vaak::ast::{HostItem, HostSig, ValueType};
use vaak::interp::Eval;
use vaak::value::{HostFns, NoHostFns, Value};
use vaak::vm::{Program2, Runner};

pub const BATCH_SOURCE: &str = include_str!("tex_batch.vaak");
pub const CALLBACK_SOURCE: &str = include_str!("tex_callback.vaak");
pub const LINEBREAK_BULK_SOURCE: &str = include_str!("tex_linebreak_bulk.vaak");
pub const LINEBREAK_NODEOPS_SOURCE: &str = include_str!("tex_linebreak_nodeops.vaak");

const COUNT_SET: i64 = 1;
const DIMEN_SET: i64 = 2;
const EMIT: i64 = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TexState {
    pub count: Vec<i32>,
    /// TeX の scaled point を模した整数値。
    pub dimen: Vec<i64>,
    pub emitted: Vec<String>,
}

impl TexState {
    pub fn sample(count: i32, dimen: i64) -> Self {
        Self {
            count: vec![count],
            dimen: vec![dimen],
            emitted: Vec::new(),
        }
    }

    fn snapshot(&self, fail: bool) -> Vec<Value> {
        vec![
            Value::array(
                ValueType::I32,
                self.count.iter().copied().map(Value::I32).collect(),
            ),
            Value::array(
                ValueType::I64,
                self.dimen.iter().copied().map(Value::I64).collect(),
            ),
            Value::U1(fail),
        ]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    pub kind: i64,
    pub index: i64,
    pub value: i64,
    pub text: String,
}

fn value_items() -> Vec<(String, HostItem)> {
    vec![
        (
            "count".into(),
            HostItem::Value(ValueType::Array(Box::new(ValueType::I32))),
        ),
        (
            "dimen".into(),
            HostItem::Value(ValueType::Array(Box::new(ValueType::I64))),
        ),
        ("fail".into(), HostItem::Value(ValueType::U1)),
    ]
}

fn callback_items() -> Vec<(String, HostItem)> {
    let mut items = value_items();
    items.extend([
        (
            "tex_count_set".into(),
            HostItem::Fn(HostSig {
                params: vec![ValueType::I64, ValueType::I64],
                ret: None,
            }),
        ),
        (
            "tex_dimen_set".into(),
            HostItem::Fn(HostSig {
                params: vec![ValueType::I64, ValueType::I64],
                ret: None,
            }),
        ),
        (
            "tex_emit".into(),
            HostItem::Fn(HostSig {
                params: vec![ValueType::Str],
                ret: None,
            }),
        ),
    ]);
    items
}

fn compile_checked(src: &str, items: &[(String, HostItem)]) -> Result<Program2, String> {
    let program = vaak::parser::parse(src).map_err(|e| e.msg)?;
    let mut errors: Vec<String> = vaak::check::check_with_host(&program, items)
        .into_iter()
        .map(|e| e.msg)
        .collect();
    errors.extend(
        vaak::types::check_types_with_host(&program, items)
            .into_iter()
            .map(|e| e.msg),
    );
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    vaak::vm::compile_with_host(&program, items).map_err(|e| e.msg)
}

/// 一度だけ解析・検査・バイトコード化し、`Runner` も繰り返し使う。
pub struct BatchVm {
    program: Program2,
    runner: Runner,
}

impl BatchVm {
    pub fn compile() -> Result<Self, String> {
        Ok(Self {
            program: compile_checked(BATCH_SOURCE, &value_items())?,
            runner: Runner::new(),
        })
    }

    /// snapshot は Vaak の所有値へ複製される。返った命令列を適用するまでは
    /// `state` に一切触らない。
    pub fn plan(&mut self, state: &TexState, fail: bool) -> Result<Vec<Command>, String> {
        let mut no_callbacks = NoHostFns;
        let (out, _) = self
            .runner
            .run_with(&self.program, state.snapshot(fail), &mut no_callbacks)
            .map_err(|e| e.msg)?;
        match out {
            Eval::Value(value) => decode_commands(value),
            Eval::Paradox(_) => Err("Vaak が paradox で終わった".into()),
            Eval::Akasha => Err("Vaak が値を返さなかった".into()),
            Eval::Escape(_) => Err("Vaak の脱出がフレームを越えた".into()),
        }
    }
}

fn field<'a>(fields: &'a [(String, Value)], name: &str) -> Result<&'a Value, String> {
    fields
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, value)| value)
        .ok_or_else(|| format!("Command.{name} が無い"))
}

fn integer(fields: &[(String, Value)], name: &str) -> Result<i64, String> {
    let n = field(fields, name)?
        .as_int()
        .ok_or_else(|| format!("Command.{name} が整数でない"))?;
    i64::try_from(n).map_err(|_| format!("Command.{name} が i64 に収まらない"))
}

fn decode_commands(value: Value) -> Result<Vec<Command>, String> {
    let Value::Array(array) = value else {
        return Err("Vaak が Command array を返さなかった".into());
    };
    array
        .items
        .into_iter()
        .map(|value| {
            let Value::Struct(record) = value else {
                return Err("命令が Command 構造体でない".into());
            };
            let text = match field(&record.fields, "text")? {
                Value::Str(bytes) => String::from_utf8((**bytes).clone())
                    .map_err(|_| "Command.text が UTF-8 でない".to_string())?,
                _ => return Err("Command.text が str でない".into()),
            };
            Ok(Command {
                kind: integer(&record.fields, "kind")?,
                index: integer(&record.fields, "index")?,
                value: integer(&record.fields, "value")?,
                text,
            })
        })
        .collect()
}

/// 全命令を状態のコピーへ適用し、全部成功したときだけ交換する。
pub fn apply_batch(state: &mut TexState, commands: &[Command]) -> Result<(), String> {
    let mut next = state.clone();
    for command in commands {
        match command.kind {
            COUNT_SET => {
                let index = usize::try_from(command.index)
                    .map_err(|_| "count の添字が負である".to_string())?;
                let slot = next
                    .count
                    .get_mut(index)
                    .ok_or_else(|| format!("count[{}] が範囲外", command.index))?;
                *slot = i32::try_from(command.value)
                    .map_err(|_| "count の値が i32 に収まらない".to_string())?;
            }
            DIMEN_SET => {
                let index = usize::try_from(command.index)
                    .map_err(|_| "dimen の添字が負である".to_string())?;
                let slot = next
                    .dimen
                    .get_mut(index)
                    .ok_or_else(|| format!("dimen[{}] が範囲外", command.index))?;
                *slot = command.value;
            }
            EMIT => next.emitted.push(command.text.clone()),
            other => return Err(format!("未知の command kind {other}")),
        }
    }
    *state = next;
    Ok(())
}

/// 同期 HostFn 版。同じプログラムを何度も走らせられるが、各 call は即時作用する。
pub struct CallbackVm {
    program: Program2,
    runner: Runner,
    names: Vec<String>,
}

impl CallbackVm {
    pub fn compile() -> Result<Self, String> {
        let program = compile_checked(CALLBACK_SOURCE, &callback_items())?;
        let names = program
            .host_fns
            .iter()
            .map(|(name, _)| name.clone())
            .collect();
        Ok(Self {
            program,
            runner: Runner::new(),
            names,
        })
    }

    pub fn run(&mut self, state: &mut TexState, fail: bool) -> Result<i128, String> {
        let mut callbacks = TexCallbacks {
            state,
            names: &self.names,
        };
        let (out, _) = self
            .runner
            .run_with(
                &self.program,
                callbacks.state.snapshot(fail),
                &mut callbacks,
            )
            .map_err(|e| e.msg)?;
        match out {
            Eval::Value(value) => value.as_int().ok_or_else(|| "整数を返さなかった".into()),
            Eval::Paradox(_) => Err("Vaak が paradox で終わった".into()),
            Eval::Akasha => Err("Vaak が値を返さなかった".into()),
            Eval::Escape(_) => Err("Vaak の脱出がフレームを越えた".into()),
        }
    }
}

struct TexCallbacks<'a> {
    state: &'a mut TexState,
    names: &'a [String],
}

impl HostFns for TexCallbacks<'_> {
    fn call(&mut self, index: u16, args: &[Value]) -> Option<Value> {
        let name = self.names.get(index as usize)?.as_str();
        match name {
            "tex_count_set" => {
                let i = usize::try_from(args.first()?.as_int()?).ok()?;
                let value = i32::try_from(args.get(1)?.as_int()?).ok()?;
                *self.state.count.get_mut(i)? = value;
            }
            "tex_dimen_set" => {
                let i = usize::try_from(args.first()?.as_int()?).ok()?;
                let value = i64::try_from(args.get(1)?.as_int()?).ok()?;
                *self.state.dimen.get_mut(i)? = value;
            }
            "tex_emit" => {
                let Value::Str(bytes) = args.first()? else {
                    return None;
                };
                self.state
                    .emitted
                    .push(String::from_utf8_lossy(bytes).into_owned());
            }
            _ => return None,
        }
        // 三関数とも返り値を置かない。呼び出し側の `;` が paradox を潰す。
        None
    }
}

/// C-96 の規約を小さく再現するための補助。
pub fn check_host_scope(src: &str) -> Result<(), String> {
    compile_checked(src, &value_items()).map(|_| ())
}

// --- node list / 行分割 phase kernel ---

pub const HOOKS_PER_NODE: i64 = 21;

/// Host/WASM 境界で一度に渡せる node arena の SoA snapshot。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeListSnapshot {
    /// 0 = box、1 = glue。
    pub kind: Vec<i64>,
    pub width: Vec<i64>,
    pub penalty: Vec<i64>,
    pub target_width: i64,
}

impl NodeListSnapshot {
    /// `words` 個の box と、その間の glue からなる段落を作る。
    pub fn paragraph(words: usize, target_width: i64) -> Self {
        let mut kind = Vec::with_capacity(words.saturating_mul(2));
        let mut width = Vec::with_capacity(words.saturating_mul(2));
        let mut penalty = Vec::with_capacity(words.saturating_mul(2));
        for word in 0..words {
            kind.push(0);
            width.push(4 + (word % 5) as i64);
            penalty.push(10_000);
            if word + 1 < words {
                kind.push(1);
                width.push(1);
                penalty.push(0);
            }
        }
        Self {
            kind,
            width,
            penalty,
            target_width,
        }
    }

    pub fn len(&self) -> usize {
        self.kind.len()
    }

    fn bulk_values(&self) -> Vec<Value> {
        vec![
            Value::array(
                ValueType::I64,
                self.kind.iter().copied().map(Value::I64).collect(),
            ),
            Value::array(
                ValueType::I64,
                self.width.iter().copied().map(Value::I64).collect(),
            ),
            Value::array(
                ValueType::I64,
                self.penalty.iter().copied().map(Value::I64).collect(),
            ),
            Value::I64(self.target_width),
        ]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineBox {
    pub start: i64,
    pub end: i64,
    pub natural: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayoutResult {
    pub lines: Vec<LineBox>,
    /// Vaak 内で実行した論理 hook の数。ABI crossing 数とは別である。
    pub logical_hook_calls: i64,
}

fn i64_array_type() -> ValueType {
    ValueType::Array(Box::new(ValueType::I64))
}

fn bulk_linebreak_items() -> Vec<(String, HostItem)> {
    vec![
        ("node_kind".into(), HostItem::Value(i64_array_type())),
        ("node_width".into(), HostItem::Value(i64_array_type())),
        ("node_penalty".into(), HostItem::Value(i64_array_type())),
        ("target_width".into(), HostItem::Value(ValueType::I64)),
    ]
}

fn nodeops_linebreak_items() -> Vec<(String, HostItem)> {
    vec![
        ("node_count".into(), HostItem::Value(ValueType::I64)),
        ("target_width".into(), HostItem::Value(ValueType::I64)),
        (
            "node_hook".into(),
            HostItem::Fn(HostSig {
                params: vec![ValueType::I64, ValueType::I64],
                ret: Some(ValueType::I64),
            }),
        ),
    ]
}

fn decode_lines(value: Value) -> Result<Vec<LineBox>, String> {
    let Value::Array(array) = value else {
        return Err("Layout.lines が Line array でない".into());
    };
    array
        .items
        .into_iter()
        .map(|value| {
            let Value::Struct(record) = value else {
                return Err("行が Line 構造体でない".into());
            };
            Ok(LineBox {
                start: integer(&record.fields, "start")?,
                end: integer(&record.fields, "end")?,
                natural: integer(&record.fields, "natural")?,
            })
        })
        .collect()
}

fn decode_layout(value: Value) -> Result<LayoutResult, String> {
    let Value::Struct(record) = value else {
        return Err("Vaak が Layout を返さなかった".into());
    };
    let logical_hook_calls = integer(&record.fields, "hook_calls")?;
    let lines = decode_lines(field(&record.fields, "lines")?.clone())?;
    Ok(LayoutResult {
        lines,
        logical_hook_calls,
    })
}

fn eval_layout(out: Eval, source: &str) -> Result<LayoutResult, String> {
    match out {
        Eval::Value(value) => decode_layout(value),
        Eval::Paradox(span) => {
            let (line, col) = vaak::span::line_col(source, span.start);
            Err(format!(
                "行分割 kernel が {line}:{col} の paradox で終わった"
            ))
        }
        Eval::Akasha => Err("行分割 kernel が値を返さなかった".into()),
        Eval::Escape(_) => Err("行分割 kernel の脱出がフレームを越えた".into()),
    }
}

/// replace result を host 側で全体検証する。
pub fn validate_line_replacement(
    nodes: &NodeListSnapshot,
    result: &LayoutResult,
) -> Result<(), String> {
    if nodes.kind.len() != nodes.width.len() || nodes.kind.len() != nodes.penalty.len() {
        return Err("node SoA の長さが揃っていない".into());
    }
    let mut next = 0usize;
    for line in &result.lines {
        let start = usize::try_from(line.start).map_err(|_| "行の start が負".to_string())?;
        let end = usize::try_from(line.end).map_err(|_| "行の end が負".to_string())?;
        if start != next || end <= start || end > nodes.len() {
            return Err(format!("行範囲 [{start}, {end}) が連続していない"));
        }
        let natural: i64 = nodes.width[start..end].iter().sum();
        if natural != line.natural {
            return Err(format!("行 [{start}, {end}) の natural width が一致しない"));
        }
        if natural > nodes.target_width {
            return Err(format!("行 [{start}, {end}) が target width を越える"));
        }
        next = end;
    }
    if next != nodes.len() {
        return Err("replace result が全 node を覆っていない".into());
    }
    Ok(())
}

/// 検証後だけ既存の行 list と交換する。
pub fn commit_line_replacement(
    current: &mut Vec<LineBox>,
    nodes: &NodeListSnapshot,
    result: &LayoutResult,
) -> Result<(), String> {
    validate_line_replacement(nodes, result)?;
    *current = result.lines.clone();
    Ok(())
}

/// WASM なら外側 ABI を越えるのは段落 phase の開始一回だけ。
pub struct BulkLineBreaker {
    program: Program2,
    runner: Runner,
    pub phase_calls: u64,
}

impl BulkLineBreaker {
    pub fn compile() -> Result<Self, String> {
        Ok(Self {
            program: compile_checked(LINEBREAK_BULK_SOURCE, &bulk_linebreak_items())?,
            runner: Runner::new(),
            phase_calls: 0,
        })
    }

    pub fn layout(&mut self, nodes: &NodeListSnapshot) -> Result<LayoutResult, String> {
        self.phase_calls += 1;
        let (out, _) = self
            .runner
            .run(&self.program, nodes.bulk_values())
            .map_err(|e| e.msg)?;
        let result = eval_layout(out, LINEBREAK_BULK_SOURCE)?;
        validate_line_replacement(nodes, &result)?;
        Ok(result)
    }
}

/// 同一 process のネイティブ埋め込み向け。21 hook/node を内部 NodeOps で答える。
/// 同じ API を WASM import にすると `nodeops_calls` 回だけ外側 ABI を越えてしまう。
pub struct NodeOpsLineBreaker {
    program: Program2,
    runner: Runner,
    node_hook_index: u16,
    pub phase_calls: u64,
    pub nodeops_calls: u64,
}

impl NodeOpsLineBreaker {
    pub fn compile() -> Result<Self, String> {
        let program = compile_checked(LINEBREAK_NODEOPS_SOURCE, &nodeops_linebreak_items())?;
        let node_hook_index = host_fn_index(&program, "node_hook")?;
        Ok(Self {
            program,
            runner: Runner::new(),
            node_hook_index,
            phase_calls: 0,
            nodeops_calls: 0,
        })
    }

    pub fn layout(&mut self, nodes: &NodeListSnapshot) -> Result<LayoutResult, String> {
        self.phase_calls += 1;
        let values = vec![
            Value::I64(i64::try_from(nodes.len()).map_err(|_| "node 数が i64 を越える")?),
            Value::I64(nodes.target_width),
        ];
        let mut callbacks = NodeCallbacks {
            nodes,
            node_hook_index: self.node_hook_index,
            calls: &mut self.nodeops_calls,
        };
        let (out, _) = self
            .runner
            .run_with(&self.program, values, &mut callbacks)
            .map_err(|e| e.msg)?;
        let result = eval_layout(out, LINEBREAK_NODEOPS_SOURCE)?;
        validate_line_replacement(nodes, &result)?;
        Ok(result)
    }
}

struct NodeCallbacks<'a> {
    nodes: &'a NodeListSnapshot,
    node_hook_index: u16,
    calls: &'a mut u64,
}

impl HostFns for NodeCallbacks<'_> {
    fn call(&mut self, index: u16, args: &[Value]) -> Option<Value> {
        if index != self.node_hook_index {
            return None;
        }
        *self.calls += 1;
        let hook = usize::try_from(args.first()?.as_int()?).ok()?;
        let node = usize::try_from(args.get(1)?.as_int()?).ok()?;
        let answer = match hook {
            0 => *self.nodes.width.get(node)?,
            1 => *self.nodes.kind.get(node)?,
            2 => *self.nodes.penalty.get(node)?,
            _ => 0,
        };
        Some(Value::I64(answer))
    }
}

// --- 現行 VM の内部 call 費用 ---

const EMPTY_LOOP_SOURCE: &str = "var sum := 0; nfor (i, 0, iterations) { sum += i; }; sum";
const LOCAL_CALL_SOURCE: &str =
    "fn hook (x : i64) { x } -> i64; var sum := 0; nfor (i, 0, iterations) { sum += hook(i); }; sum";
const HOST1_CALL_SOURCE: &str = "var sum := 0; nfor (i, 0, iterations) { sum += host1(i); }; sum";
const HOST2_CALL_SOURCE: &str =
    "var sum := 0; nfor (i, 0, iterations) { sum += host2(i, 0); }; sum";

#[derive(Clone, Debug)]
pub struct VmCallBenchmark {
    pub iterations: u64,
    pub samples: usize,
    pub empty_loop: std::time::Duration,
    pub local_call_1: std::time::Duration,
    pub host_call_1: std::time::Duration,
    pub host_call_2: std::time::Duration,
    /// 各 candidate の直前・直後の空 loop 平均を引いた paired 中央値。
    pub local_extra_ns: f64,
    pub host1_extra_ns: f64,
    pub host2_extra_ns: f64,
    pub host1_calls: u64,
    pub host2_calls: u64,
}

impl VmCallBenchmark {
    pub fn ns_per_iteration(duration: std::time::Duration, iterations: u64) -> f64 {
        duration.as_nanos() as f64 / iterations as f64
    }
}

fn iterations_item() -> Vec<(String, HostItem)> {
    vec![("iterations".into(), HostItem::Value(ValueType::I64))]
}

fn timed_host_items(name: &str, params: usize) -> Vec<(String, HostItem)> {
    let mut items = iterations_item();
    items.push((
        name.into(),
        HostItem::Fn(HostSig {
            params: vec![ValueType::I64; params],
            ret: Some(ValueType::I64),
        }),
    ));
    items
}

fn median(mut values: Vec<std::time::Duration>) -> std::time::Duration {
    values.sort_unstable();
    values[values.len() / 2]
}

fn median_f64(mut values: Vec<f64>) -> f64 {
    values.sort_by(|a, b| a.total_cmp(b));
    values[values.len() / 2]
}

fn host_fn_index(program: &Program2, wanted: &str) -> Result<u16, String> {
    let index = program
        .host_fns
        .iter()
        .position(|(name, _)| name == wanted)
        .ok_or_else(|| format!("HostFn {wanted} が組み立て結果に無い"))?;
    u16::try_from(index).map_err(|_| format!("HostFn {wanted} の index が u16 を越える"))
}

#[derive(Clone, Copy)]
enum TimedFn {
    One,
    Two,
}

#[derive(Clone, Copy)]
enum BenchCandidate {
    Local,
    Host1,
    Host2,
}

struct TimedFns<'a> {
    expected_index: u16,
    function: TimedFn,
    calls: &'a mut u64,
}

impl HostFns for TimedFns<'_> {
    fn call(&mut self, index: u16, args: &[Value]) -> Option<Value> {
        if index != self.expected_index {
            return None;
        }
        *self.calls += 1;
        match self.function {
            TimedFn::One => Some(Value::I64(i64::try_from(args.first()?.as_int()?).ok()?)),
            TimedFn::Two => {
                let a = i64::try_from(args.first()?.as_int()?).ok()?;
                let b = i64::try_from(args.get(1)?.as_int()?).ok()?;
                Some(Value::I64(a.wrapping_add(b)))
            }
        }
    }
}

/// 解析・検査・compile は計時外。各 program と Runner を一度作り、中央値を返す。
pub fn benchmark_vm_calls(iterations: u64, samples: usize) -> Result<VmCallBenchmark, String> {
    let n = i64::try_from(iterations).map_err(|_| "反復数が i64 を越える")?;
    let samples = samples.max(1);
    let empty = compile_checked(EMPTY_LOOP_SOURCE, &iterations_item())?;
    let local = compile_checked(LOCAL_CALL_SOURCE, &iterations_item())?;
    let host1_items = timed_host_items("host1", 1);
    let host2_items = timed_host_items("host2", 2);
    let host1 = compile_checked(HOST1_CALL_SOURCE, &host1_items)?;
    let host2 = compile_checked(HOST2_CALL_SOURCE, &host2_items)?;

    let mut empty_runner = Runner::new();
    let mut local_runner = Runner::new();
    let mut host1_runner = Runner::new();
    let mut host2_runner = Runner::new();
    // 容量確保と初回経路を計時から外す。
    empty_runner
        .run(&empty, vec![Value::I64(1)])
        .map_err(|e| e.msg)?;
    local_runner
        .run(&local, vec![Value::I64(1)])
        .map_err(|e| e.msg)?;

    let host1_index = host_fn_index(&host1, "host1")?;
    let host2_index = host_fn_index(&host2, "host2")?;
    let mut warm_calls = 0;
    host1_runner
        .run_with(
            &host1,
            vec![Value::I64(1)],
            &mut TimedFns {
                expected_index: host1_index,
                function: TimedFn::One,
                calls: &mut warm_calls,
            },
        )
        .map_err(|e| e.msg)?;
    host2_runner
        .run_with(
            &host2,
            vec![Value::I64(1)],
            &mut TimedFns {
                expected_index: host2_index,
                function: TimedFn::Two,
                calls: &mut warm_calls,
            },
        )
        .map_err(|e| e.msg)?;

    let mut empty_times = Vec::with_capacity(samples);
    let mut local_times = Vec::with_capacity(samples);
    let mut host1_times = Vec::with_capacity(samples);
    let mut host2_times = Vec::with_capacity(samples);
    let mut local_extra = Vec::with_capacity(samples);
    let mut host1_extra = Vec::with_capacity(samples);
    let mut host2_extra = Vec::with_capacity(samples);
    let mut host1_calls = 0;
    let mut host2_calls = 0;

    let candidates = [
        BenchCandidate::Local,
        BenchCandidate::Host1,
        BenchCandidate::Host2,
    ];
    for sample in 0..samples {
        // 固定順による周波数・常駐負荷 drift を避ける。
        for offset in 0..candidates.len() {
            let candidate = candidates[(sample + offset) % candidates.len()];

            let started = std::time::Instant::now();
            empty_runner
                .run(&empty, vec![Value::I64(n)])
                .map_err(|e| e.msg)?;
            let before = started.elapsed();
            empty_times.push(before);

            let started = std::time::Instant::now();
            match candidate {
                BenchCandidate::Local => {
                    local_runner
                        .run(&local, vec![Value::I64(n)])
                        .map_err(|e| e.msg)?;
                }
                BenchCandidate::Host1 => {
                    host1_runner
                        .run_with(
                            &host1,
                            vec![Value::I64(n)],
                            &mut TimedFns {
                                expected_index: host1_index,
                                function: TimedFn::One,
                                calls: &mut host1_calls,
                            },
                        )
                        .map_err(|e| e.msg)?;
                }
                BenchCandidate::Host2 => {
                    host2_runner
                        .run_with(
                            &host2,
                            vec![Value::I64(n)],
                            &mut TimedFns {
                                expected_index: host2_index,
                                function: TimedFn::Two,
                                calls: &mut host2_calls,
                            },
                        )
                        .map_err(|e| e.msg)?;
                }
            }
            let measured = started.elapsed();

            let started = std::time::Instant::now();
            empty_runner
                .run(&empty, vec![Value::I64(n)])
                .map_err(|e| e.msg)?;
            let after = started.elapsed();
            empty_times.push(after);

            let baseline_ns =
                (before.as_nanos() as f64 + after.as_nanos() as f64) / (2.0 * iterations as f64);
            let extra_ns = measured.as_nanos() as f64 / iterations as f64 - baseline_ns;
            match candidate {
                BenchCandidate::Local => {
                    local_times.push(measured);
                    local_extra.push(extra_ns);
                }
                BenchCandidate::Host1 => {
                    host1_times.push(measured);
                    host1_extra.push(extra_ns);
                }
                BenchCandidate::Host2 => {
                    host2_times.push(measured);
                    host2_extra.push(extra_ns);
                }
            }
        }
    }

    Ok(VmCallBenchmark {
        iterations,
        samples,
        empty_loop: median(empty_times),
        local_call_1: median(local_times),
        host_call_1: median(host1_times),
        host_call_2: median(host2_times),
        local_extra_ns: median_f64(local_extra),
        host1_extra_ns: median_f64(host1_extra),
        host2_extra_ns: median_f64(host2_extra),
        host1_calls,
        host2_calls,
    })
}
