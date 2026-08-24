//! pure Vaak CSR/SCC/BFS/topological/2-SATの参照実装・VM・STEEL差分試験。

use std::collections::{BTreeSet, VecDeque};
use std::fmt::Write as _;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use vaak::interp::Eval;

const GRAPH: &str = include_str!("../stdlib/graph/csr_scc_two_sat_i64.vaak");
const ASCII_IO: &str = include_str!("../stdlib/io/ascii_i64.vaak");
const GRAPH_IO_EXAMPLE: &str = include_str!("../stdlib/examples/graph_scc_io.vaak");
static TEMP_ID: AtomicU64 = AtomicU64::new(0);

fn source(body: &str) -> String {
    format!("{GRAPH}\n{body}")
}

#[track_caller]
fn checked(body: &str) -> String {
    let source = source(body);
    let program = vaak::parser::parse(&source).expect("graphライブラリを含むソースを解析できる");
    let errors: Vec<_> = vaak::check::check(&program)
        .into_iter()
        .chain(vaak::types::check_types(&program))
        .map(|error| format!("{} @{:?}", error.msg, error.span))
        .collect();
    assert!(errors.is_empty(), "静的検査: {errors:?}\n{body}");
    source
}

fn shape(result: Result<Eval, String>) -> String {
    match result {
        Ok(Eval::Value(value)) => format!("値 {}", value.show()),
        Ok(Eval::Paradox(_)) => "paradox".into(),
        Ok(Eval::Akasha) => "虚無".into(),
        Ok(Eval::Escape(_)) => "脱出".into(),
        Err(error) => format!("エラー {error}"),
    }
}

#[track_caller]
fn reference_and_vm(body: &str, expected: &str) {
    let source = checked(body);
    for (name, result) in [
        ("参照", vaak::interp::run(&source)),
        ("VM", vaak::vm::run(&source)),
    ] {
        assert_eq!(shape(result), expected, "{name}: {body}");
    }
}

#[track_caller]
fn steel_native(body: &str, expected_exit: i32) {
    let source = checked(body);
    let program = vaak::parser::parse(&source).expect("構文");
    let ir = vaak::steel::compile(&program).unwrap_or_else(|error| panic!("STEEL: {}", error.msg));
    if Command::new("clang").arg("--version").output().is_err() {
        return;
    }

    let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join(format!("vaak-graph-{}-{id}", std::process::id()));
    std::fs::create_dir(&directory).expect("専用一時ディレクトリを作れる");
    let llvm = directory.join("program.ll");
    let executable = directory.join(if cfg!(windows) {
        "program.exe"
    } else {
        "program"
    });
    std::fs::write(&llvm, ir).expect("LLVM IRを書ける");
    let built = Command::new("clang")
        .arg("-O2")
        .arg("-o")
        .arg(&executable)
        .arg(&llvm)
        .output()
        .expect("clangを起動できる");
    if !built.status.success() {
        let message = String::from_utf8_lossy(&built.stderr).into_owned();
        let _ = std::fs::remove_dir_all(&directory);
        panic!("clang: {message}");
    }
    let status = Command::new(&executable)
        .status()
        .expect("STEEL生成物を実行できる");
    std::fs::remove_dir_all(&directory).expect("専用一時ディレクトリを片付けられる");
    assert_eq!(status.code(), Some(expected_exit));
}

#[test]
fn csrは空自己辺多重辺と追加順を保つ() {
    let body = r#"
        var empty_builder := csr_i64_builder_new(0) ?? new CsrBuilderI64(vertex_count := 1, from := [0], to := [0]);
        let empty := csr_i64_build(empty_builder) ?? new CsrI64(start := [9], to := [9]);
        var builder := csr_i64_builder_new(3) ?? empty_builder;
        csr_i64_builder_add_directed(builder, 1, 2) ?? false;
        csr_i64_builder_add_directed(builder, 0, 2) ?? false;
        csr_i64_builder_add_directed(builder, 1, 0) ?? false;
        csr_i64_builder_add_directed(builder, 1, 2) ?? false;
        let graph := csr_i64_build(builder) ?? empty;
        var self_builder := csr_i64_builder_new(1) ?? empty_builder;
        csr_i64_builder_add_undirected(self_builder, 0, 0) ?? false;
        let self_graph := csr_i64_build(self_builder) ?? empty;
        if (csr_i64_is_valid(empty) && csr_i64_vertex_count(empty) == 0 &&
            csr_i64_edge_count(empty) == 0 && empty.start.len() == 1 && empty.start[0] == 0 &&
            csr_i64_is_valid(graph) && csr_i64_edge_count(graph) == 4 &&
            csr_i64_degree(graph, 0) == 1 && csr_i64_neighbor(graph, 0, 0) == 2 &&
            csr_i64_degree(graph, 1) == 3 && csr_i64_neighbor(graph, 1, 0) == 2 &&
            csr_i64_neighbor(graph, 1, 1) == 0 && csr_i64_neighbor(graph, 1, 2) == 2 &&
            csr_i64_degree(self_graph, 0) == 2 && csr_i64_neighbor(self_graph, 0, 0) == 0 &&
            csr_i64_neighbor(self_graph, 0, 1) == 0) 42 else 0 fi
    "#;
    reference_and_vm(body, "値 42");
    steel_native(body, 42);
}

#[test]
fn csrの負数範囲外と壊れた公開欄はparadoxになる() {
    reference_and_vm("csr_i64_builder_new(-1)", "paradox");
    reference_and_vm(
        "var b := csr_i64_builder_new(2) ?? new CsrBuilderI64(vertex_count := 0, from := [], to := []); csr_i64_builder_add_directed(b, 2, 0)",
        "paradox",
    );
    reference_and_vm(
        "let g := new CsrI64(start := [0, 2], to := [0]); csr_i64_vertex_count(g)",
        "paradox",
    );
    reference_and_vm(
        "let g := new CsrI64(start := [0, 1], to := [1]); scc_i64(g)",
        "paradox",
    );
}

#[test]
fn 反復sccは切断成分と位相順の番号を返す() {
    let body = r#"
        var builder := csr_i64_builder_new(8) ?? new CsrBuilderI64(vertex_count := 0, from := [], to := []);
        csr_i64_builder_add_directed(builder, 0, 1) ?? false;
        csr_i64_builder_add_directed(builder, 1, 0) ?? false;
        csr_i64_builder_add_directed(builder, 1, 2) ?? false;
        csr_i64_builder_add_directed(builder, 1, 2) ?? false;
        csr_i64_builder_add_directed(builder, 2, 2) ?? false;
        csr_i64_builder_add_directed(builder, 2, 3) ?? false;
        csr_i64_builder_add_directed(builder, 3, 4) ?? false;
        csr_i64_builder_add_directed(builder, 4, 3) ?? false;
        csr_i64_builder_add_directed(builder, 6, 7) ?? false;
        csr_i64_builder_add_directed(builder, 7, 6) ?? false;
        let graph := csr_i64_build(builder) ?? new CsrI64(start := [0], to := []);
        let result := scc_i64(graph) ?? new SccResultI64(group_count := -1, group_of := []);
        if (result.group_count == 5 && result.group_of.len() == 8 &&
            scc_i64_same(result, 0, 1) && ! scc_i64_same(result, 1, 2) &&
            scc_i64_same(result, 3, 4) && scc_i64_same(result, 6, 7) &&
            result.group_of[0] < result.group_of[2] &&
            result.group_of[2] < result.group_of[3]) 42 else 0 fi
    "#;
    reference_and_vm(body, "値 42");
    steel_native(body, 42);

    reference_and_vm(
        "let g := new CsrI64(start := [0], to := []); let r := scc_i64(g) ?? new SccResultI64(group_count := -1, group_of := [0]); if (r.group_count == 0 && r.group_of.len() == 0) 42 else 0 fi",
        "値 42",
    );
}

#[test]
fn bfsはcsr辺順の親と未到達の負一を返す() {
    let body = r#"
        var builder := csr_i64_builder_new(7) ?? new CsrBuilderI64(vertex_count := 0, from := [], to := []);
        csr_i64_builder_add_directed(builder, 0, 0) ?? false;
        csr_i64_builder_add_directed(builder, 0, 2) ?? false;
        csr_i64_builder_add_directed(builder, 0, 1) ?? false;
        csr_i64_builder_add_directed(builder, 0, 2) ?? false;
        csr_i64_builder_add_directed(builder, 2, 3) ?? false;
        csr_i64_builder_add_directed(builder, 1, 3) ?? false;
        csr_i64_builder_add_directed(builder, 3, 4) ?? false;
        csr_i64_builder_add_directed(builder, 5, 6) ?? false;
        let graph := csr_i64_build(builder) ?? new CsrI64(start := [0], to := []);
        let result := bfs_i64(graph, 0) ?? new BfsResultI64(distance := [], parent := []);
        if (result.distance.len() == 7 && result.parent.len() == 7 &&
            result.distance[0] == 0 && result.parent[0] == 0 &&
            result.distance[1] == 1 && result.parent[1] == 0 &&
            result.distance[2] == 1 && result.parent[2] == 0 &&
            result.distance[3] == 2 && result.parent[3] == 2 &&
            result.distance[4] == 3 && result.parent[4] == 3 &&
            result.distance[5] == -1 && result.parent[5] == -1 &&
            result.distance[6] == -1 && result.parent[6] == -1) 42 else 0 fi
    "#;
    reference_and_vm(body, "値 42");
    steel_native(body, 42);
    reference_and_vm(
        "let graph := new CsrI64(start := [0], to := []); bfs_i64(graph, 0)",
        "paradox",
    );
    reference_and_vm(
        "let graph := new CsrI64(start := [0, 0], to := []); bfs_i64(graph, -1)",
        "paradox",
    );
}

#[test]
fn topological_sortは辞書順最小でcycleを通常結果にする() {
    let body = r#"
        var builder := csr_i64_builder_new(4) ?? new CsrBuilderI64(vertex_count := 0, from := [], to := []);
        csr_i64_builder_add_directed(builder, 0, 1) ?? false;
        csr_i64_builder_add_directed(builder, 0, 1) ?? false;
        let graph := csr_i64_build(builder) ?? new CsrI64(start := [0], to := []);
        let result := topological_sort_i64(graph) ?? new TopologicalResultI64(acyclic := false, order := []);
        let empty_graph := new CsrI64(start := [0], to := []);
        let empty := topological_sort_i64(empty_graph) ?? new TopologicalResultI64(acyclic := false, order := [9]);
        if (result.acyclic && result.order.len() == 4 &&
            result.order[0] == 0 && result.order[1] == 1 &&
            result.order[2] == 2 && result.order[3] == 3 &&
            empty.acyclic && empty.order.len() == 0) 42 else 0 fi
    "#;
    reference_and_vm(body, "値 42");
    steel_native(body, 42);

    reference_and_vm(
        r#"var builder := csr_i64_builder_new(3) ?? new CsrBuilderI64(vertex_count := 0, from := [], to := []);
           csr_i64_builder_add_directed(builder, 0, 1) ?? false;
           csr_i64_builder_add_directed(builder, 1, 0) ?? false;
           csr_i64_builder_add_directed(builder, 2, 2) ?? false;
           let graph := csr_i64_build(builder) ?? new CsrI64(start := [0], to := []);
           let result := topological_sort_i64(graph) ?? new TopologicalResultI64(acyclic := true, order := [9]);
           if (! result.acyclic && result.order.len() == 0) 42 else 0 fi"#,
        "値 42",
    );
}

#[test]
fn two_satは充足不能をparadoxと区別する() {
    let satisfiable = r#"
        var solver := two_sat_i64_new(3) ?? new TwoSatI64(variable_count := 0, from := [], to := []);
        two_sat_i64_add_clause(solver, 0, true, 1, true) ?? false;
        two_sat_i64_add_clause(solver, 0, false, 2, true) ?? false;
        two_sat_i64_set_value(solver, 1, false) ?? false;
        two_sat_i64_set_value(solver, 2, true) ?? false;
        let result := two_sat_i64_solve(solver) ?? new TwoSatResultI64(satisfiable := false, assignment := []);
        if (result.satisfiable && result.assignment.len() == 3 &&
            result.assignment[0] && ! result.assignment[1] && result.assignment[2]) 42 else 0 fi
    "#;
    reference_and_vm(satisfiable, "値 42");
    steel_native(satisfiable, 42);

    reference_and_vm(
        r#"var solver := two_sat_i64_new(1) ?? new TwoSatI64(variable_count := 0, from := [], to := []);
           two_sat_i64_set_value(solver, 0, true) ?? false;
           two_sat_i64_set_value(solver, 0, false) ?? false;
           let result := two_sat_i64_solve(solver) ?? new TwoSatResultI64(satisfiable := true, assignment := [true]);
           if (! result.satisfiable && result.assignment.len() == 0) 42 else 0 fi"#,
        "値 42",
    );
    reference_and_vm(
        "var solver := two_sat_i64_new(1) ?? new TwoSatI64(variable_count := 0, from := [], to := []); two_sat_i64_add_clause(solver, 1, true, 0, false)",
        "paradox",
    );
    reference_and_vm(
        "let solver := new TwoSatI64(variable_count := 1, from := [0], to := [2]); two_sat_i64_solve(solver)",
        "paradox",
    );
}

#[test]
fn scc例は一括入力strから一括出力strへ接続できる() {
    let body = format!("{ASCII_IO}\n{GRAPH_IO_EXAMPLE}");
    reference_and_vm(&body, "値 42");
    steel_native(&body, 42);
}

#[derive(Clone, Copy)]
struct Deterministic(u64);

impl Deterministic {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }
}

fn reachability(vertex_count: usize, edges: &[(usize, usize)]) -> Vec<Vec<bool>> {
    let mut reachable = vec![vec![false; vertex_count]; vertex_count];
    for vertex in 0..vertex_count {
        reachable[vertex][vertex] = true;
    }
    for &(from, to) in edges {
        reachable[from][to] = true;
    }
    for through in 0..vertex_count {
        for from in 0..vertex_count {
            for to in 0..vertex_count {
                reachable[from][to] |= reachable[from][through] && reachable[through][to];
            }
        }
    }
    reachable
}

fn bfs_oracle(
    vertex_count: usize,
    edges: &[(usize, usize)],
    source: usize,
) -> (Vec<i64>, Vec<i64>) {
    let mut adjacency = vec![Vec::new(); vertex_count];
    for &(from, to) in edges {
        adjacency[from].push(to);
    }
    let mut distance = vec![-1; vertex_count];
    let mut parent = vec![-1; vertex_count];
    let mut queue = VecDeque::new();
    distance[source] = 0;
    parent[source] = source as i64;
    queue.push_back(source);
    while let Some(vertex) = queue.pop_front() {
        for &to in &adjacency[vertex] {
            if distance[to] < 0 {
                distance[to] = distance[vertex] + 1;
                parent[to] = vertex as i64;
                queue.push_back(to);
            }
        }
    }
    (distance, parent)
}

fn topological_oracle(vertex_count: usize, edges: &[(usize, usize)]) -> Option<Vec<usize>> {
    let mut adjacency = vec![Vec::new(); vertex_count];
    let mut indegree = vec![0usize; vertex_count];
    for &(from, to) in edges {
        adjacency[from].push(to);
        indegree[to] += 1;
    }
    let mut ready = BTreeSet::new();
    for (vertex, &degree) in indegree.iter().enumerate() {
        if degree == 0 {
            ready.insert(vertex);
        }
    }
    let mut order = Vec::new();
    while let Some(vertex) = ready.iter().next().copied() {
        ready.remove(&vertex);
        order.push(vertex);
        for &to in &adjacency[vertex] {
            indegree[to] -= 1;
            if indegree[to] == 0 {
                ready.insert(to);
            }
        }
    }
    (order.len() == vertex_count).then_some(order)
}

#[test]
fn sccの決定的ランダムグラフを到達可能性oracleと三backendで照合する() {
    let vertex_count = 10usize;
    let mut random = Deterministic(0x5641_414b_5343_4321);
    let mut edges = Vec::new();
    for _ in 0..48 {
        let bits = random.next();
        edges.push((
            (bits % vertex_count as u64) as usize,
            ((bits >> 32) % vertex_count as u64) as usize,
        ));
    }
    let reachable = reachability(vertex_count, &edges);
    let mut body = format!(
        "var builder := csr_i64_builder_new({vertex_count}) ?? new CsrBuilderI64(vertex_count := 0, from := [], to := []);\n"
    );
    for &(from, to) in &edges {
        writeln!(
            body,
            "csr_i64_builder_add_directed(builder, {from}, {to}) ?? false;"
        )
        .unwrap();
    }
    body.push_str(
        "let graph := csr_i64_build(builder) ?? new CsrI64(start := [0], to := []);\n\
         let result := scc_i64(graph) ?? new SccResultI64(group_count := -1, group_of := []);\n\
         var ok := true;\n",
    );
    for a in 0..vertex_count {
        for b in 0..vertex_count {
            let same = reachable[a][b] && reachable[b][a];
            writeln!(
                body,
                "if (scc_i64_same(result, {a}, {b}) != {same}) ok := false; fi;"
            )
            .unwrap();
        }
    }
    for &(from, to) in &edges {
        writeln!(
            body,
            "if (! scc_i64_same(result, {from}, {to}) && result.group_of[{from}] >= result.group_of[{to}]) ok := false; fi;"
        )
        .unwrap();
    }
    body.push_str("if (ok) 42 else 0 fi");
    reference_and_vm(&body, "値 42");
    steel_native(&body, 42);
}

#[test]
fn bfsの決定的ランダムグラフをrust_oracleと三backendで照合する() {
    let vertex_count = 11usize;
    let source_vertex = 4usize;
    let mut random = Deterministic(0x5641_414b_4246_5321);
    let mut edges = Vec::new();
    for _ in 0..60 {
        let bits = random.next();
        edges.push((
            (bits % vertex_count as u64) as usize,
            ((bits >> 32) % vertex_count as u64) as usize,
        ));
    }
    let (distance, parent) = bfs_oracle(vertex_count, &edges, source_vertex);
    let mut body = format!(
        "var builder := csr_i64_builder_new({vertex_count}) ?? new CsrBuilderI64(vertex_count := 0, from := [], to := []);\n"
    );
    for &(from, to) in &edges {
        writeln!(
            body,
            "csr_i64_builder_add_directed(builder, {from}, {to}) ?? false;"
        )
        .unwrap();
    }
    writeln!(
        body,
        "let graph := csr_i64_build(builder) ?? new CsrI64(start := [0], to := []);\n\
         let result := bfs_i64(graph, {source_vertex}) ?? new BfsResultI64(distance := [], parent := []);\n\
         var ok := result.distance.len() == {vertex_count} && result.parent.len() == {vertex_count};"
    )
    .unwrap();
    for vertex in 0..vertex_count {
        writeln!(
            body,
            "if (result.distance[{vertex}] != {} || result.parent[{vertex}] != {}) ok := false; fi;",
            distance[vertex], parent[vertex]
        )
        .unwrap();
    }
    body.push_str("if (ok) 42 else 0 fi");
    reference_and_vm(&body, "値 42");
    steel_native(&body, 42);
}

#[test]
fn topological_sortの決定的dagをrust_oracleと三backendで照合する() {
    let vertex_count = 13usize;
    let mut random = Deterministic(0x5641_414b_544f_504f);
    let mut edges = Vec::new();
    for _ in 0..64 {
        let bits = random.next();
        let a = (bits % vertex_count as u64) as usize;
        let b = ((bits >> 32) % vertex_count as u64) as usize;
        if a != b {
            edges.push((a.min(b), a.max(b)));
        }
    }
    let expected = topological_oracle(vertex_count, &edges).expect("番号増加方向だけなのでDAG");
    let mut body = format!(
        "var builder := csr_i64_builder_new({vertex_count}) ?? new CsrBuilderI64(vertex_count := 0, from := [], to := []);\n"
    );
    for &(from, to) in &edges {
        writeln!(
            body,
            "csr_i64_builder_add_directed(builder, {from}, {to}) ?? false;"
        )
        .unwrap();
    }
    body.push_str(
        "let graph := csr_i64_build(builder) ?? new CsrI64(start := [0], to := []);\n\
         let result := topological_sort_i64(graph) ?? new TopologicalResultI64(acyclic := false, order := []);\n\
         var ok := result.acyclic;\n",
    );
    writeln!(
        body,
        "if (result.order.len() != {vertex_count}) ok := false; fi;"
    )
    .unwrap();
    for (position, vertex) in expected.iter().enumerate() {
        writeln!(
            body,
            "if (result.order[{position}] != {vertex}) ok := false; fi;"
        )
        .unwrap();
    }
    body.push_str("if (ok) 42 else 0 fi");
    reference_and_vm(&body, "値 42");
    steel_native(&body, 42);
}

#[derive(Clone, Copy)]
struct Clause {
    variable_i: usize,
    value_i: bool,
    variable_j: usize,
    value_j: bool,
}

fn satisfies(assignment: usize, clause: Clause) -> bool {
    (((assignment >> clause.variable_i) & 1) != 0) == clause.value_i
        || (((assignment >> clause.variable_j) & 1) != 0) == clause.value_j
}

#[test]
fn two_satの決定的ランダム節を全割当oracleと三backendで照合する() {
    let variable_count = 7usize;
    let mut random = Deterministic(0x5641_414b_3253_4154);
    let mut clauses = Vec::new();
    for _ in 0..24 {
        let bits = random.next();
        clauses.push(Clause {
            variable_i: (bits % variable_count as u64) as usize,
            value_i: (bits >> 16) & 1 != 0,
            variable_j: ((bits >> 32) % variable_count as u64) as usize,
            value_j: (bits >> 48) & 1 != 0,
        });
    }
    let satisfiable = (0..(1usize << variable_count))
        .any(|assignment| clauses.iter().all(|&clause| satisfies(assignment, clause)));

    let mut body = format!(
        "var solver := two_sat_i64_new({variable_count}) ?? new TwoSatI64(variable_count := 0, from := [], to := []);\n"
    );
    for clause in &clauses {
        writeln!(
            body,
            "two_sat_i64_add_clause(solver, {}, {}, {}, {}) ?? false;",
            clause.variable_i, clause.value_i, clause.variable_j, clause.value_j
        )
        .unwrap();
    }
    body.push_str(
        "let result := two_sat_i64_solve(solver) ?? new TwoSatResultI64(satisfiable := false, assignment := []);\n\
         var ok := true;\n",
    );
    writeln!(
        body,
        "if (result.satisfiable != {satisfiable}) ok := false; fi;"
    )
    .unwrap();
    if satisfiable {
        writeln!(
            body,
            "if (result.assignment.len() != {variable_count}) ok := false; fi;"
        )
        .unwrap();
        for clause in &clauses {
            writeln!(
                body,
                "if (! ((result.assignment[{}] == {}) || (result.assignment[{}] == {}))) ok := false; fi;",
                clause.variable_i, clause.value_i, clause.variable_j, clause.value_j
            )
            .unwrap();
        }
    } else {
        body.push_str("if (result.assignment.len() != 0) ok := false; fi;\n");
    }
    body.push_str("if (ok) 42 else 0 fi");
    reference_and_vm(&body, "値 42");
    steel_native(&body, 42);
}

#[test]
fn 長い有向路の探索はvaak再帰上限に依存しない() {
    let body = r#"
        let n := 4096;
        var builder := csr_i64_builder_new(n) ?? new CsrBuilderI64(vertex_count := 0, from := [], to := []);
        nfor (i, 0, n - 1) { csr_i64_builder_add_directed(builder, i, i + 1) ?? false; };
        let graph := csr_i64_build(builder) ?? new CsrI64(start := [0], to := []);
        let components := scc_i64(graph) ?? new SccResultI64(group_count := -1, group_of := []);
        let breadth := bfs_i64(graph, 0) ?? new BfsResultI64(distance := [], parent := []);
        let topology := topological_sort_i64(graph) ?? new TopologicalResultI64(acyclic := false, order := []);
        if (components.group_count == n && components.group_of[0] == 0 &&
            components.group_of[n - 1] == n - 1 &&
            breadth.distance[n - 1] == n - 1 && breadth.parent[n - 1] == n - 2 &&
            topology.acyclic && topology.order.len() == n &&
            topology.order[0] == 0 && topology.order[n - 1] == n - 1) 42 else 0 fi
    "#;
    reference_and_vm(body, "値 42");
    steel_native(body, 42);
}
