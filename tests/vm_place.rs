//! 代入左辺の bytecode が集合体の根を値スタックへ写さないこと。

use vaak::vm::Op;

#[test]
fn const別名は一つの字句境界につき一度だけ凍らせる() {
    let syntax = vaak::parser::parse(
        "fn f (const x : i64 alias) { x; };
         var a := 1;
         { const b : i64 alias &= a; };
         f(a); a",
    )
    .expect("構文");
    let program = vaak::vm::compile(&syntax).expect("VM 組み立て");
    let freezes: Vec<usize> = program
        .chunks
        .iter()
        .map(|chunk| {
            chunk
                .ops
                .iter()
                .filter(|op| matches!(op, Op::Freeze(..)))
                .count()
        })
        .collect();

    assert_eq!(freezes, vec![1, 1], "局所別名と別名引数を二重に凍らせない");
}

#[test]
fn 入れ子代入は根をloadして書き戻す命令列へ戻らない() {
    let source = "struct Item { var value : i64 := 0; };
         var items : Item array := new Item array(8, new Item());
         items[3].value := 9;
         0";
    let syntax = vaak::parser::parse(source).expect("構文");
    let program = vaak::vm::compile(&syntax).expect("VM 組み立て");
    let ops = &program.chunks[program.top as usize].ops;

    assert!(
        ops.iter().any(|op| matches!(op, Op::PlaceRoot(..))),
        "根を実セルへ解く"
    );
    assert!(
        ops.iter().any(|op| matches!(op, Op::PlaceIndex(..))),
        "添字を経路へ足す"
    );
    assert!(
        ops.iter().any(|op| matches!(op, Op::PlaceField(..))),
        "欄を経路へ足す"
    );
    assert!(
        ops.iter().any(|op| matches!(op, Op::StorePlace(..))),
        "末端へ直接書く"
    );

    assert!(
        !ops.iter()
            .any(|op| matches!(op, Op::SetIndex(..) | Op::SetField(..))),
        "根まで集合体を組み直す旧経路へ戻ると、反復が O(N²) になる: {ops:?}"
    );
    // この台本で `items` を値として読む必要はない。旧実装は書き戻しのためだけに
    // `Load(items)` し、全要素を深く複製していた。
    assert!(
        !ops.iter().any(|op| matches!(op, Op::Load(..))),
        "代入のためだけに根を深く複製している: {ops:?}"
    );
}

#[test]
fn 一段の添字代入は経路を確保せず直接セルへ書く() {
    let syntax = vaak::parser::parse("var values := [1, 2]; values[0] += 3; 0").expect("構文");
    let program = vaak::vm::compile(&syntax).expect("VM 組み立て");
    let ops = &program.chunks[program.top as usize].ops;

    assert!(
        ops.iter().any(|op| matches!(op, Op::UpdateIndex(..))),
        "一段の添字は直接更新する: {ops:?}"
    );
    assert!(
        !ops.iter().any(|op| matches!(op, Op::PlaceRoot(..))),
        "一段の添字ごとに経路を確保してはならない: {ops:?}"
    );
}

#[test]
fn 入れ子の読み取りと長さも根をloadしない() {
    let syntax = vaak::parser::parse(
        "struct Bucket { var data : i64 array; };
         var bucket := new Bucket (data := [1, 2, 3]);
         bucket.data[1] + bucket.data.len()",
    )
    .expect("構文");
    let program = vaak::vm::compile(&syntax).expect("VM 組み立て");
    let ops = &program.chunks[program.top as usize].ops;

    assert!(
        ops.iter().any(|op| matches!(op, Op::LoadPlace(..))),
        "入れ子の末端だけを読む: {ops:?}"
    );
    assert!(
        ops.iter().any(|op| matches!(op, Op::LoadPlaceLen(..))),
        "入れ子の集合体は長さだけを読む: {ops:?}"
    );
    assert!(
        !ops.iter().any(|op| matches!(op, Op::LoadField(..))),
        "中間の配列を深く複製してはならない: {ops:?}"
    );
}
