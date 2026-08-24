//! 細粒度 pure Vaak 配列ライブラリの参照実装・VM・STEEL差分試験。

use std::process::Command;
use vaak::interp::Eval;

const RANGE: &str = include_str!("../stdlib/range/check.vaak");
const LINEAR: &str = include_str!("../stdlib/array/i64/search_linear.vaak");
const BINARY: &str = include_str!("../stdlib/array/i64/search_binary.vaak");
const REVERSE: &str = include_str!("../stdlib/array/i64/reverse.vaak");
const PREFIX: &str = include_str!("../stdlib/array/i64/prefix_sum.vaak");
const INSERTION_SORT: &str = include_str!("../stdlib/array/i64/sort/insertion.vaak");
const HEAP_SORT: &str = include_str!("../stdlib/array/i64/sort/heap.vaak");
const MERGE_SORT: &str = include_str!("../stdlib/array/i64/sort/merge.vaak");
const SORTED_UNIQUE: &str = include_str!("../stdlib/array/i64/partition/sorted_unique.vaak");
const COMPRESS: &str = include_str!("../stdlib/array/i64/compress.vaak");
const DSU: &str = include_str!("../stdlib/ds/dsu_i64.vaak");
const ROLLBACK_DSU: &str = include_str!("../stdlib/ds/rollback_dsu_i64.vaak");
const WEIGHTED_DSU: &str = include_str!("../stdlib/ds/weighted_dsu_i64.vaak");
const FENWICK: &str = include_str!("../stdlib/ds/fenwick_i64.vaak");
const FENWICK_COUNT: &str = include_str!("../stdlib/ds/fenwick_count_i64.vaak");
const FENWICK_FLAT: &str = include_str!("../stdlib/ds/fenwick_i64_flat.vaak");
const HEAP: &str = include_str!("../stdlib/ds/heap_i64.vaak");
const DEQUE: &str = include_str!("../stdlib/ds/deque_i64.vaak");
const SEGTREE: &str = include_str!("../stdlib/ds/segtree_i64.vaak");
const LAZY_SEGTREE: &str = include_str!("../stdlib/ds/lazy_segtree_i64.vaak");
const SPARSE_TABLE: &str = include_str!("../stdlib/ds/sparse_table_i64.vaak");
const DISJOINT_SPARSE_TABLE: &str = include_str!("../stdlib/ds/disjoint_sparse_table_i64.vaak");
const ORDERED_MULTISET: &str = include_str!("../stdlib/ds/ordered_multiset_i64.vaak");
const ASCII_I64: &str = include_str!("../stdlib/io/ascii_i64.vaak");

fn source(parts: &[&str], body: &str) -> String {
    let mut src = String::new();
    for part in parts {
        src.push_str(part);
        src.push('\n');
    }
    src.push_str(body);
    src
}

#[track_caller]
fn checked(parts: &[&str], body: &str) -> String {
    let src = source(parts, body);
    let prog = vaak::parser::parse(&src).expect("配列ライブラリを含むソースを解析できる");
    let errors: Vec<_> = vaak::check::check(&prog)
        .into_iter()
        .chain(vaak::types::check_types(&prog))
        .map(|e| format!("{} @{:?}", e.msg, e.span))
        .collect();
    assert!(errors.is_empty(), "静的検査: {errors:?}\n{body}");
    src
}

fn shape(result: Result<Eval, String>) -> String {
    match result {
        Ok(Eval::Value(v)) => format!("値 {}", v.show()),
        Ok(Eval::Paradox(_)) => "paradox".into(),
        Ok(Eval::Akasha) => "虚無".into(),
        Ok(Eval::Escape(_)) => "脱出".into(),
        Err(e) => format!("エラー {e}"),
    }
}

#[track_caller]
fn both(parts: &[&str], body: &str, expected: &str) {
    let src = checked(parts, body);
    for (name, result) in [
        ("参照", vaak::interp::run(&src)),
        ("VM", vaak::vm::run(&src)),
    ] {
        assert_eq!(shape(result), expected, "{name}: {body}");
    }
}

#[test]
fn 各ソースは単独または明示した依存だけで前置きできる() {
    both(
        &[RANGE],
        "if (range_is_valid(3, 0, 3)) 42 else 0 fi",
        "値 42",
    );
    both(
        &[LINEAR],
        "let xs := [7]; array_i64_find(xs, 7) ?? 0",
        "値 0",
    );
    both(
        &[BINARY],
        "let xs := [7]; array_i64_lower_bound(xs, 7)",
        "値 0",
    );
    both(
        &[REVERSE],
        "var xs := [1, 2]; array_i64_reverse(xs) ?? false; xs[0]",
        "値 2",
    );
    both(
        &[PREFIX],
        "let xs := [42]; let p := array_i64_prefix_sum(xs) ?? [0]; p[1]",
        "値 42",
    );
    both(
        &[INSERTION_SORT],
        "var xs := [2, 1]; array_i64_insertion_sort(xs) ?? false; xs[0]",
        "値 1",
    );
    both(
        &[HEAP_SORT],
        "var xs := [2, 1]; array_i64_heap_sort(xs) ?? false; xs[0]",
        "値 1",
    );
    both(
        &[MERGE_SORT],
        "var xs := [2, 1]; array_i64_merge_sort(xs) ?? false; xs[0]",
        "値 1",
    );
    both(
        &[SORTED_UNIQUE],
        "var xs := [1, 1]; array_i64_sorted_unique_in_place(xs) ?? 0",
        "値 1",
    );
    both(
        &[BINARY, MERGE_SORT, SORTED_UNIQUE, COMPRESS],
        "let xs := [7, 3, 7]; let c := array_i64_coordinate_compress(xs) ?? new CoordinateCompressionI64(unique_values := [0], ranks := [0]); coordinate_compression_i64_len(c)",
        "値 2",
    );
    both(
        &[DSU],
        "let d := dsu_i64_new(3) ?? new DsuI64(parent_or_size := [0]); dsu_i64_len(d)",
        "値 3",
    );
    both(
        &[ROLLBACK_DSU],
        "let d := rollback_dsu_i64_new(3) ?? new RollbackDsuI64(parent_or_size := [0], history := [0]); rollback_dsu_i64_len(d)",
        "値 3",
    );
    both(
        &[WEIGHTED_DSU],
        "let d := weighted_dsu_i64_new(3) ?? new WeightedDsuI64(parent_or_size := [0], weight_to_parent := [0]); weighted_dsu_i64_len(d)",
        "値 3",
    );
    both(
        &[FENWICK],
        "let f := fenwick_i64_new(3) ?? new FenwickI64(data := [0]); fenwick_i64_len(f)",
        "値 3",
    );
    both(
        &[FENWICK_COUNT],
        "let f := fenwick_count_i64_new(3) ?? new FenwickCountI64(data := [0], total := 0); fenwick_count_i64_len(f)",
        "値 3",
    );
    both(
        &[FENWICK_FLAT],
        "let f := fenwick_i64_flat_new(3) ?? [0]; f.len()",
        "値 3",
    );
    both(
        &[HEAP],
        "var h := min_heap_i64_new() ?? new MinHeapI64(data := [0]); min_heap_i64_push(h, 7) ?? false; min_heap_i64_peek(h)",
        "値 7",
    );
    both(
        &[DEQUE],
        "var q := deque_i64_new() ?? new DequeI64(data := [0], head := 0, size := 0); deque_i64_push_front(q, 7) ?? false; deque_i64_peek_back(q)",
        "値 7",
    );
    both(
        &[SEGTREE],
        "let xs := [4, 2, 7]; let t := min_segtree_i64_from(xs) ?? new MinSegtreeI64(length := 0, size := 1, data := [0, 0]); min_segtree_i64_all_prod(t)",
        "値 2",
    );
    both(
        &[LAZY_SEGTREE],
        "let xs := [1, 2]; var t := range_add_sum_segtree_i64_from(xs) ?? new RangeAddSumSegtreeI64(length := 0, size := 1, data := [0, 0], lazy := [0, 0]); range_add_sum_segtree_i64_range_add(t, 0, 2, 3) ?? false; range_add_sum_segtree_i64_all_prod(t)",
        "値 9",
    );
    both(
        &[SPARSE_TABLE],
        "let xs := [4, 2, 7]; let t := sparse_min_i64_from(xs) ?? new SparseMinI64(length := 0, levels := 0, data := []); sparse_min_i64_prod(t, 0, 3)",
        "値 2",
    );
    both(
        &[DISJOINT_SPARSE_TABLE],
        "let xs := [4, 2, 7]; let t := disjoint_sparse_sum_i64_from(xs) ?? new DisjointSparseSumI64(length := 0, levels := 0, data := []); disjoint_sparse_sum_i64_prod(t, 0, 3)",
        "値 13",
    );
    both(
        &[ORDERED_MULTISET],
        "let keys := [3, 7]; var set := ordered_multiset_i64_new(keys) ?? new OrderedMultisetI64(keys := [], fenwick := [], total := 0); ordered_multiset_i64_insert(set, 7) ?? false; ordered_multiset_i64_kth(set, 0)",
        "値 7",
    );
    both(
        &[ASCII_I64],
        r#"let input := "  -42 "; var at := 0; io_ascii_i64_read(input, at)"#,
        "値 -42",
    );
}

#[test]
fn 各ソースはsteelにも単独で前置きできる() {
    let cases = [
        (RANGE, "range_length(3, 0, 3)"),
        (LINEAR, "let xs := [7]; array_i64_find(xs, 7) ?? 0"),
        (BINARY, "let xs := [7]; array_i64_lower_bound(xs, 7)"),
        (
            REVERSE,
            "var xs := [1, 2]; array_i64_reverse(xs) ?? false; xs[0]",
        ),
        (
            PREFIX,
            "let xs := [42]; let p := array_i64_prefix_sum(xs) ?? [0]; p[1]",
        ),
        (
            INSERTION_SORT,
            "var xs := [2, 1]; array_i64_insertion_sort(xs) ?? false; xs[0]",
        ),
        (
            HEAP_SORT,
            "var xs := [2, 1]; array_i64_heap_sort(xs) ?? false; xs[0]",
        ),
        (
            MERGE_SORT,
            "var xs := [2, 1]; array_i64_merge_sort(xs) ?? false; xs[0]",
        ),
        (
            SORTED_UNIQUE,
            "var xs := [1, 1]; array_i64_sorted_unique_in_place(xs) ?? 0",
        ),
        (
            DSU,
            "let d := dsu_i64_new(3) ?? new DsuI64(parent_or_size := [0]); dsu_i64_len(d)",
        ),
        (
            ROLLBACK_DSU,
            "let d := rollback_dsu_i64_new(3) ?? new RollbackDsuI64(parent_or_size := [0], history := [0]); rollback_dsu_i64_len(d)",
        ),
        (
            WEIGHTED_DSU,
            "let d := weighted_dsu_i64_new(3) ?? new WeightedDsuI64(parent_or_size := [0], weight_to_parent := [0]); weighted_dsu_i64_len(d)",
        ),
        (
            FENWICK,
            "let f := fenwick_i64_new(3) ?? new FenwickI64(data := [0]); fenwick_i64_len(f)",
        ),
        (
            FENWICK_COUNT,
            "let f := fenwick_count_i64_new(3) ?? new FenwickCountI64(data := [0], total := 0); fenwick_count_i64_len(f)",
        ),
        (
            FENWICK_FLAT,
            "let f := fenwick_i64_flat_new(3) ?? [0]; f.len()",
        ),
        (
            HEAP,
            "var h := max_heap_i64_new() ?? new MaxHeapI64(data := [0]); max_heap_i64_push(h, 7) ?? false; max_heap_i64_peek(h)",
        ),
        (
            DEQUE,
            "var q := deque_i64_new() ?? new DequeI64(data := [0], head := 0, size := 0); deque_i64_push_back(q, 7) ?? false; deque_i64_peek_front(q)",
        ),
        (
            SEGTREE,
            "let xs := [4, 2, 7]; let t := max_segtree_i64_from(xs) ?? new MaxSegtreeI64(length := 0, size := 1, data := [0, 0]); max_segtree_i64_all_prod(t)",
        ),
        (
            LAZY_SEGTREE,
            "let xs := [1, 2]; var t := range_add_sum_segtree_i64_from(xs) ?? new RangeAddSumSegtreeI64(length := 0, size := 1, data := [0, 0], lazy := [0, 0]); range_add_sum_segtree_i64_range_add(t, 0, 1, 4) ?? false; range_add_sum_segtree_i64_all_prod(t)",
        ),
        (
            SPARSE_TABLE,
            "let xs := [4, 2, 7]; let t := sparse_max_i64_from(xs) ?? new SparseMaxI64(length := 0, levels := 0, data := []); sparse_max_i64_prod(t, 0, 3)",
        ),
        (
            DISJOINT_SPARSE_TABLE,
            "let xs := [4, 2, 7]; let t := disjoint_sparse_sum_i64_from(xs) ?? new DisjointSparseSumI64(length := 0, levels := 0, data := []); disjoint_sparse_sum_i64_prod(t, 0, 3)",
        ),
        (
            ORDERED_MULTISET,
            "let keys := [3, 7]; var set := ordered_multiset_i64_new(keys) ?? new OrderedMultisetI64(keys := [], fenwick := [], total := 0); ordered_multiset_i64_insert(set, 7) ?? false; ordered_multiset_i64_kth(set, 0)",
        ),
        (
            ASCII_I64,
            r#"let input := "42"; var at := 0; io_ascii_i64_read(input, at)"#,
        ),
    ];
    for (library, body) in cases {
        let src = checked(&[library], body);
        let prog = vaak::parser::parse(&src).expect("構文");
        vaak::steel::compile(&prog).unwrap_or_else(|e| panic!("{body}: STEEL: {}", e.msg));
    }
    let body = "let xs := [7, 3, 7]; let c := array_i64_coordinate_compress(xs) ?? new CoordinateCompressionI64(unique_values := [0], ranks := [0]); coordinate_compression_i64_len(c)";
    let src = checked(&[BINARY, MERGE_SORT, SORTED_UNIQUE, COMPRESS], body);
    let prog = vaak::parser::parse(&src).expect("構文");
    vaak::steel::compile(&prog).unwrap_or_else(|e| panic!("座標圧縮: STEEL: {}", e.msg));
}

#[test]
fn 全ソースを同時に前置きしても名前が衝突しない() {
    let libraries = [
        RANGE,
        LINEAR,
        BINARY,
        REVERSE,
        PREFIX,
        INSERTION_SORT,
        HEAP_SORT,
        MERGE_SORT,
        SORTED_UNIQUE,
        COMPRESS,
        DSU,
        ROLLBACK_DSU,
        WEIGHTED_DSU,
        FENWICK,
        FENWICK_COUNT,
        FENWICK_FLAT,
        HEAP,
        DEQUE,
        SEGTREE,
        LAZY_SEGTREE,
        SPARSE_TABLE,
        DISJOINT_SPARSE_TABLE,
        ORDERED_MULTISET,
        ASCII_I64,
    ];
    both(&libraries, "42", "値 42");
    let src = checked(&libraries, "42");
    let prog = vaak::parser::parse(&src).expect("構文");
    vaak::steel::compile(&prog).unwrap_or_else(|e| panic!("STEEL: {}", e.msg));
}

#[test]
fn 半開区間は空を許し逆転と範囲外を畳む() {
    both(
        &[RANGE],
        "if (range_is_valid(5, 2, 2) && range_length(5, 2, 2) == 0 && range_is_index(5, 4)) 42 else 0 fi",
        "値 42",
    );
    both(&[RANGE], "range_length(5, 4, 3)", "paradox");
    both(&[RANGE], "range_length(5, 0, 6)", "paradox");
}

#[test]
fn 線形探索は最初と最後と個数を区別する() {
    both(
        &[LINEAR],
        r#"let xs := [3, 1, 3, 4];
           let first := array_i64_find(xs, 3) ?? -10;
           let last := array_i64_rfind(xs, 3) ?? -10;
           let count := array_i64_count(xs, 3) ?? -10;
           if (first == 0 && last == 2 && count == 2 &&
               ! array_i64_contains_range(xs, 3, 1, 2) &&
               array_i64_count_range(xs, 3, 2, 2) == 0) 42 else 0 fi"#,
        "値 42",
    );
    both(
        &[LINEAR],
        "let xs := [1, 2]; array_i64_find(xs, 9)",
        "paradox",
    );
    both(
        &[LINEAR],
        "let xs := [1, 2]; array_i64_count_range(xs, 1, -1, 1)",
        "paradox",
    );
}

#[test]
fn 二分探索は重複と空区間の境界を返す() {
    both(
        &[BINARY],
        r#"let xs := [1, 2, 2, 2, 5];
           let lower := array_i64_lower_bound(xs, 2) ?? -10;
           let upper := array_i64_upper_bound(xs, 2) ?? -10;
           let found := array_i64_binary_find(xs, 2) ?? -10;
           let empty := array_i64_lower_bound_range(xs, 2, 3, 3) ?? -10;
           if (lower == 1 && upper == 4 && found == 1 && empty == 3 &&
               array_i64_binary_contains(xs, 5)) 42 else 0 fi"#,
        "値 42",
    );
    both(
        &[BINARY],
        "let xs := [1, 2, 5]; array_i64_binary_find(xs, 3)",
        "paradox",
    );
    both(
        &[BINARY],
        "let xs := [1, 2, 5]; array_i64_lower_bound_range(xs, 2, 0, 4)",
        "paradox",
    );
}

#[test]
fn 反転はvar_aliasを通して元の半開区間だけを変える() {
    both(
        &[REVERSE],
        r#"var xs := [1, 2, 3, 4, 5];
           let ok := array_i64_reverse_range(xs, 1, 4) ?? false;
           let empty_ok := array_i64_reverse_range(xs, 2, 2) ?? false;
           if (ok && empty_ok && xs[0] == 1 && xs[1] == 4 &&
               xs[2] == 3 && xs[3] == 2 && xs[4] == 5) 42 else 0 fi"#,
        "値 42",
    );
    both(
        &[REVERSE],
        "var xs := [1, 2]; array_i64_reverse_range(xs, -1, 2)",
        "paradox",
    );
}

#[test]
fn 累積和は先頭零と空区間を持つ() {
    both(
        &[PREFIX],
        r#"let xs := [3, -1, 4];
           let p := array_i64_prefix_sum(xs) ?? [0];
           let empty_xs : i64 array := new i64 array(0, 0);
           let empty_p := array_i64_prefix_sum(empty_xs) ?? [9];
           if (p.len() == 4 && p[0] == 0 && p[3] == 6 &&
               array_i64_prefix_range_sum(p, 1, 3) == 3 &&
               array_i64_prefix_range_sum(p, 2, 2) == 0 &&
               empty_p.len() == 1 && empty_p[0] == 0) 42 else 0 fi"#,
        "値 42",
    );
    both(
        &[PREFIX],
        "let p := [0, 1]; array_i64_prefix_range_sum(p, 0, 2)",
        "paradox",
    );
}

#[test]
fn dsuは大きさ併合と経路圧縮を配列一本で行う() {
    both(
        &[DSU],
        r#"var dsu := dsu_i64_new(5) ?? new DsuI64(parent_or_size := [0]);
           dsu_i64_merge(dsu, 0, 1) ?? -1;
           dsu_i64_merge(dsu, 1, 2) ?? -1;
           if (dsu_i64_same(dsu, 0, 2) && ! dsu_i64_same(dsu, 0, 4) &&
               dsu_i64_size(dsu, 1) == 3 && dsu_i64_group_count(dsu) == 3) 42 else 0 fi"#,
        "値 42",
    );
    both(
        &[DSU],
        "var dsu := dsu_i64_new(2) ?? new DsuI64(parent_or_size := [0]); dsu_i64_leader(dsu, 2)",
        "paradox",
    );
    both(&[DSU], "dsu_i64_new(-1)", "paradox");
}

#[test]
fn fenwickは固定加算で半開区間和を取る() {
    both(
        &[FENWICK],
        r#"var tree := fenwick_i64_new(5) ?? new FenwickI64(data := [0]);
           fenwick_i64_add(tree, 0, 3) ?? false;
           fenwick_i64_add(tree, 2, 4) ?? false;
           fenwick_i64_add(tree, 4, -1) ?? false;
           if (fenwick_i64_prefix_sum(tree, 3) == 7 &&
               fenwick_i64_range_sum(tree, 1, 5) == 3 &&
               fenwick_i64_range_sum(tree, 2, 2) == 0 &&
               fenwick_i64_get(tree, 2) == 4) 42 else 0 fi"#,
        "値 42",
    );
    both(
        &[FENWICK],
        "var tree := fenwick_i64_new(2) ?? new FenwickI64(data := [0]); fenwick_i64_add(tree, 2, 1)",
        "paradox",
    );
    both(&[FENWICK], "fenwick_i64_new(-1)", "paradox");
}

#[test]
fn 平坦表現のfenwickも同じ固定演算契約を持つ() {
    both(
        &[FENWICK_FLAT],
        r#"var data := fenwick_i64_flat_new(5) ?? [0];
           fenwick_i64_flat_add(data, 0, 3) ?? false;
           fenwick_i64_flat_add(data, 2, 4) ?? false;
           fenwick_i64_flat_add(data, 4, -1) ?? false;
           if (fenwick_i64_flat_prefix_sum(data, 3) == 7 &&
               fenwick_i64_flat_range_sum(data, 1, 5) == 3 &&
               fenwick_i64_flat_range_sum(data, 2, 2) == 0 &&
               fenwick_i64_flat_get(data, 2) == 4) 42 else 0 fi"#,
        "値 42",
    );
    both(
        &[FENWICK_FLAT],
        "var data := fenwick_i64_flat_new(2) ?? [0]; fenwick_i64_flat_add(data, -1, 1)",
        "paradox",
    );
}

#[test]
fn steelでも固定演算の代表例をnative実行できる() {
    let body = r#"
        let sorted := [1, 2, 2, 5, 8];
        let at := array_i64_lower_bound(sorted, 2) ?? -1;
        var values := [3, 1, 4, 1, 5];
        array_i64_reverse_range(values, 1, 4) ?? false;
        let prefix := array_i64_prefix_sum(values) ?? [0];
        var dsu := dsu_i64_new(4) ?? new DsuI64(parent_or_size := [0]);
        dsu_i64_merge(dsu, 0, 3) ?? -1;
        var tree := fenwick_i64_new(4) ?? new FenwickI64(data := [0]);
        fenwick_i64_add(tree, 1, 40) ?? false;
        if (at == 1 && values[1] == 1 && values[3] == 1 &&
            array_i64_prefix_range_sum(prefix, 0, 5) == 14 &&
            dsu_i64_same(dsu, 0, 3) && fenwick_i64_get(tree, 1) == 40) 42 else 0 fi
    "#;
    let src = checked(&[BINARY, REVERSE, PREFIX, DSU, FENWICK], body);
    let prog = vaak::parser::parse(&src).expect("構文");
    let ir = vaak::steel::compile(&prog).unwrap_or_else(|e| panic!("STEEL: {}", e.msg));

    if Command::new("clang").arg("--version").output().is_err() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("vaak-array-library-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("一時ディレクトリ");
    let ll = dir.join("array.ll");
    let exe = dir.join("array.exe");
    let pdb = dir.join("array.pdb");
    std::fs::write(&ll, ir).expect("LLVM IR");
    let built = Command::new("clang")
        .arg("-O2")
        .arg("-o")
        .arg(&exe)
        .arg(&ll)
        .output()
        .expect("clang");
    if !built.status.success() {
        let message = String::from_utf8_lossy(&built.stderr).into_owned();
        let _ = std::fs::remove_file(&ll);
        let _ = std::fs::remove_file(&exe);
        let _ = std::fs::remove_file(&pdb);
        let _ = std::fs::remove_dir(&dir);
        panic!("clang: {message}");
    }
    let status = Command::new(&exe).status().expect("native実行");
    let _ = std::fs::remove_file(&ll);
    let _ = std::fs::remove_file(&exe);
    let _ = std::fs::remove_file(&pdb);
    let _ = std::fs::remove_dir(&dir);
    assert_eq!(status.code(), Some(42));
}
