use vaak::interp::Eval;

const CASES: &[&str] = &[
    include_str!("../examples/experiments/tagged_inline.vaak"),
    include_str!("../examples/experiments/tagged_function.vaak"),
    include_str!("../examples/experiments/tagged_parallel_arrays.vaak"),
    include_str!("../examples/experiments/tagged_struct_array.vaak"),
];

fn show(x: Eval) -> String {
    match x {
        Eval::Value(v) => v.show(),
        Eval::Paradox(_) => "paradox".into(),
        Eval::Akasha => "akasha".into(),
        Eval::Escape(_) => "escape".into(),
    }
}

#[test]
fn tagとswitchの四配置が同じ意味になる() {
    let mut answer = None;
    for src in CASES {
        // 通常試験は意味だけを見る。規模を変えた実測は
        // `cargo run --release --example bench_tagged` に残す。
        let prog = vaak::parser::parse(src).expect("構文");
        let mut errs = vaak::check::check(&prog);
        errs.extend(vaak::types::check_types(&prog));
        assert!(
            errs.is_empty(),
            "{:?}",
            errs.iter().map(|e| &e.msg).collect::<Vec<_>>()
        );

        let tree = show(vaak::interp::Interp::new().run(&prog).expect("参照実装"));
        let bytecode = vaak::vm::compile(&prog).expect("VM 組み立て");
        let vm = show(vaak::vm::run_program(&bytecode).expect("VM"));
        assert_eq!(tree, vm);
        match &answer {
            None => answer = Some(tree),
            Some(want) => assert_eq!(want, &tree),
        }
    }
}

#[test]
fn 構造体配列の入れ子欄を書き換えられる() {
    let src = r#"
        struct Cell { var tag : i64 := 0; };
        var cells := new Cell array(4, new Cell());
        cells[2].tag := 7;
        cells[2].tag
    "#;
    assert_eq!(show(vaak::interp::run(src).expect("参照実装")), "7");
    assert_eq!(show(vaak::vm::run(src).expect("VM")), "7");
}

#[test]
fn 不明なtagはparadoxとして畳める() {
    let src = "let tag := 9; (switch (tag) case 0 => 10 case 1 => 20) ?? -1";
    assert_eq!(show(vaak::interp::run(src).expect("参照実装")), "-1");
    assert_eq!(show(vaak::vm::run(src).expect("VM")), "-1");
}
