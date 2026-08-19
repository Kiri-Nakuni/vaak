//! 意味論のスナップショットテスト。
//!
//! 設計書のコード例と docs/decisions.md の確定事項を、そのまま回帰テストにする。
//! 仕様書とテストが同じものを指していれば、今回のような例と本文の食い違いは
//! 構造的に起きなくなる。

use reod::run;

fn ok(src: &str) -> Vec<String> {
    run(src, vec![]).unwrap_or_else(|e| panic!("実行に失敗: {}", e))
}

// ---- 設計書のコード例 ----

#[test]
fn ex6_goto_as_checkpoint_reset() {
    let src = include_str!("../examples/ex6_goto_reset.reod");
    assert_eq!(ok(src), vec!["1"], "例6: foo は 1 に束縛される");
}

#[test]
fn ex5_comefrom_real_visit_detection() {
    let src = include_str!("../examples/ex5_comefrom.reod");
    assert_eq!(ok(src), vec!["0", "1", "2", "4"]);
}

#[test]
fn ex3_collatz_with_a_function() {
    let src = include_str!("../examples/ex3_collatz.reod");
    assert_eq!(ok(src), vec!["111", "118", "178"]);
}

#[test]
fn ex7_early_return() {
    let src = include_str!("../examples/ex7_early_return.reod");
    assert_eq!(ok(src), vec!["1", "1", "0", "0", "1"]);
}

// ---- D-1: `if` はスコープを作らない ----

#[test]
fn d1_if_body_is_not_a_scope() {
    // if の中の変更はトップレベル扱いのまま残る。
    let out = ok(r#"
        let n := 0;
        if 1 == 1 { n := 5; }
        print(n);
    "#);
    assert_eq!(out, vec!["5"]);
}

#[test]
fn d1_bare_block_is_a_scope() {
    let out = ok(r#"
        let n := 0;
        { n := 5; }
        print(n);
    "#);
    assert_eq!(out, vec!["0"]);
}

#[test]
fn d1_comefrom_retry_counter_terminates() {
    // 設計書 §3.5 の comefrom 例。retry_count += 1 は `if` の中だが
    // スコープを作らないので jump 巻き戻しの影響を受けず、3回で止まる。
    let out = ok(r#"
        let retry_count := 0;
        comefrom retry;
        print(retry_count);
        if retry_count < 3 {
            retry_count += 1;
            label retry;
        }
    "#);
    assert_eq!(out, vec!["0", "1", "2", "3"]);
}

// ---- D-2r: ループ本体はスコープではなくチェックポイント ----

#[test]
fn d2r_accumulation_survives_the_loop() {
    // journal は正常脱出時に適用せず破棄する。蓄積は確定する。
    let out = ok(r#"
        let acc := 0;
        loop {
            acc += 1;
            if acc == 3 break;
        }
        print(acc);
    "#);
    assert_eq!(out, vec!["3"]);
}

#[test]
fn d2r_jump_still_rolls_the_loop_body_back() {
    // チェックポイントが存在する唯一の理由。例6 と同じ形。
    let out = ok(r#"
        let x := 0;
        let hit := 0;
        loop {
            if hit == 1 break x;
            x += 1;
            if x == 1 {
                #[global]
                hit := 1;
                goto A;
            }
            label A;
        }
        print(x);
    "#);
    assert_eq!(out, vec!["0"], "jump は x をベースラインへ戻す");
}

#[test]
fn d2r_break_value_is_forced() {
    let out = ok(r#"
        let acc := 0;
        #[immediate]
        let r := loop {
            acc += 1;
            if acc == 3 break acc;
        };
        print(r);
        print(acc);
    "#);
    assert_eq!(out, vec!["3", "3"]);
}

// ---- D-4: global は TeX 方式（全 save level から undo エントリを除去） ----

#[test]
fn d4_global_write_erases_pending_undo_entries() {
    let out = ok(r#"
        let g := 0;
        {
            g := 5;
            #[global]
            g := 7;
        }
        print(g);
    "#);
    assert_eq!(out, vec!["7"]);
}

// ---- D-18r / D-19 撤回: global と immediate は直交する2つの操作 ----

#[test]
fn d19_withdrawn_global_alone_stays_lazy() {
    // global は書き込みレベルだけを変える。遅延のままなので、右辺に左辺が
    // 現れれば D-17 どおり自己参照 thunk になって発散する。
    let e = run(
        r#"
        let x := 8;
        {
            #[global]
            x := x / 2;
        }
        print(x);
    "#,
        vec![],
    )
    .unwrap_err();
    assert!(e.contains("自己参照"), "実際のエラー: {}", e);
}

#[test]
fn d18r_attribute_list() {
    // 2つの操作を同時に指定したいなら、リストで明示的に書く。
    let out = ok(r#"
        let x := 8;
        {
            #[global, immediate]
            x := x / 2;
        }
        print(x);
    "#);
    assert_eq!(out, vec!["4"]);
}

#[test]
fn d18r_attribute_must_terminate_at_end_of_line() {
    // 「次の行の評価方法を変更する」を文字通りにする。同一行は不正。
    let e = run("let x := 0;\n#[immediate] x := 1;\nprint(x);", vec![]).unwrap_err();
    assert!(e.contains("行末"), "実際のエラー: {}", e);
}

#[test]
fn d21r_function_definition_terminates_at_newline() {
    // 返り値型は改行で終端するので、型名表がなくても次の行を吸い込まない。
    let out = ok("fn twice (n : i64) { n * 2 } -> i64;\nprint(twice(21));");
    assert_eq!(out, vec!["42"]);
}

// ---- 関数層 ----

#[test]
fn function_definition_and_call() {
    let out = ok(r#"
        fn add (a : i64, b : i64) { a + b } -> i64;
        print(add(3, 4));
    "#);
    assert_eq!(out, vec!["7"]);
}

#[test]
fn recursion_works_without_rec_attribute() {
    // D-6: 名前解決が強制の瞬間まで先送りされるので、不動点コンビネータは要らない。
    let out = ok(r#"
        fn fact (n : u64) {
            if n <= 1 { 1 } else { n * fact(n - 1) }
        } -> u64;
        print(fact(10));
    "#);
    assert_eq!(out, vec!["3628800"]);
}

#[test]
fn mutual_recursion_works() {
    let out = ok(r#"
        fn is_even (n : u64) {
            if n == 0 { 1 } else { is_odd(n - 1) }
        } -> u64;
        fn is_odd (n : u64) {
            if n == 0 { 0 } else { is_even(n - 1) }
        } -> u64;
        print(is_even(10));
        print(is_odd(10));
    "#);
    assert_eq!(out, vec!["1", "0"]);
}

#[test]
fn function_body_is_a_scope_so_params_do_not_leak() {
    let out = ok(r#"
        let n := 100;
        fn f (n : i64) { n * 2 } -> i64;
        print(f(3));
        print(n);
    "#);
    assert_eq!(out, vec!["6", "100"]);
}

#[test]
fn d2r_loop_inside_a_function_accumulates_locally() {
    // D-2 のままだと count が巻き戻り、`#[global]` で逃がすと再帰が壊れた。
    // D-2r ではどちらも起きない。
    let out = ok(r#"
        fn count_down (n : i64) {
            let steps := 0;
            let k := n;
            loop {
                if k == 0 break;
                #[immediate]
                k := k - 1;
                steps += 1;
            }
            steps
        } -> i64;
        print(count_down(5));
        print(count_down(3));
    "#);
    assert_eq!(out, vec!["5", "3"]);
}

#[test]
fn nested_calls_do_not_clobber_the_callers_locals() {
    let out = ok(r#"
        fn inner (x : i64) { x + 1 } -> i64;
        fn outer (x : i64) {
            let y := inner(x * 10);
            x + y
        } -> i64;
        print(outer(2));
    "#);
    assert_eq!(out, vec!["23"]);
}

#[test]
fn d20_arguments_are_forced_at_the_call_site() {
    // 遅延のまま渡すと、引数名が呼ばれた側の束縛の下で解決されて自己参照になる。
    let out = ok(r#"
        fn f (x : i64) { x + 1 } -> i64;
        let x := 41;
        print(f(x));
    "#);
    assert_eq!(out, vec!["42"]);
}

#[test]
fn d27_let_rejects_a_function_definition() {
    // 値の束縛と関数定義は別の操作。`let f (x) { ... }` は書けない。
    let e = run("let f (x) { x } -> i64;\n", vec![]).unwrap_err();
    assert!(e.contains("`fn`"), "実際のエラー: {}", e);
}

#[test]
fn functions_are_not_first_class() {
    let e = run("fn f (x) { x } -> i64;\nlet g := f;\nprint(g);", vec![]).unwrap_err();
    assert!(e.contains("第一級"), "実際のエラー: {}", e);
}

#[test]
fn arity_mismatch_is_rejected() {
    let e = run("fn f (a, b) { a } -> i64;\nprint(f(1));", vec![]).unwrap_err();
    assert!(e.contains("引数"), "実際のエラー: {}", e);
}

#[test]
fn call_site_return_type_annotation_parses() {
    // D-8: 呼出側アノテーションは `->`。Tier 1 は単相なので読み捨てる。
    let out = ok(r#"
        fn make (n : u64) { n * 2 } -> u64;
        let v := make(21) -> u64;
        print(v);
    "#);
    assert_eq!(out, vec!["42"]);
}

// ---- D-28: break はそのブロックの値を正格に束縛して終了する ----

#[test]
fn d28_break_returns_from_a_function() {
    // `return` は要らない。break はどのブロックでも同じ意味を持つ。
    let out = ok(r#"
        fn clamp (n : i64) {
            if n > 100 break 100;
            if n < 0 break 0;
            n
        } -> i64;
        print(clamp(500));
        print(clamp(-7));
        print(clamp(42));
    "#);
    assert_eq!(out, vec!["100", "0", "42"]);
}

#[test]
fn d28_if_bodies_are_transparent_to_break() {
    // `if` の本体は状態に対して透明（D-1）なので、break に対しても透明。
    // 外側の本物のブロック（ここではループ）が受け止める。
    let out = ok(r#"
        let i := 0;
        loop {
            i += 1;
            if i == 3 { break; }
        }
        print(i);
    "#);
    assert_eq!(out, vec!["3"]);
}

#[test]
fn d28_bare_block_absorbs_its_own_break() {
    // 裸の `{ }` は本物のブロックなので、その break はそこで止まる。
    let out = ok(r#"
        let i := 0;
        loop {
            i += 1;
            { break; }
            if i == 3 break;
        }
        print(i);
    "#);
    assert_eq!(out, vec!["3"]);
}

// ---- D-33 / D-34: getdepth と break の入れ子 ----

#[test]
fn d34_nested_break_leaves_several_blocks() {
    // 内側で1段吸われ、残り1段で外側のブロックも抜ける。
    // 裸のブロックはスコープなので、変数ではなく印字で観測する（D-1）。
    let out = ok(r#"
        {
            {
                break break 2;
            }
            print(1);
        }
        print(2);
    "#);
    assert_eq!(out, vec!["2"]);
}

#[test]
fn d34_single_break_only_leaves_its_own_block() {
    let out = ok(r#"
        {
            {
                break 2;
            }
            print(1);
        }
        print(2);
    "#);
    assert_eq!(out, vec!["1", "2"]);
}

#[test]
fn d34_nested_break_carries_its_value_out() {
    let out = ok(r#"
        #[immediate]
        let v := loop {
            {
                break break 7;
            }
        };
        print(v);
    "#);
    assert_eq!(out, vec!["7"]);
}

#[test]
fn d33_getdepth_counts_what_break_counts() {
    // `if` の本体は break に対して透明（D-28）なので数に入らない。
    let out = ok(r#"
        print(getdepth());
        {
            print(getdepth());
            if 1 == 1 {
                print(getdepth());
            }
            loop {
                print(getdepth());
                break;
            }
        }
    "#);
    assert_eq!(out, vec!["1", "2", "2", "3"]);
}

#[test]
fn d33_getdepth_is_relative_to_the_call_frame() {
    // 関数本体で 1。break 1段で関数を抜けられる、という意味。
    let out = ok(r#"
        fn f (x : i64) {
            print(getdepth());
            {
                print(getdepth());
            }
            x
        } -> i64;
        {
            {
                print(f(0));
            }
        }
    "#);
    assert_eq!(out, vec!["1", "2", "0"]);
}

#[test]
fn d34_break_cannot_cross_a_function_boundary() {
    let e = run(
        "fn f (x) { break break x; } -> i64;\nprint(f(1));",
        vec![],
    )
    .unwrap_err();
    assert!(e.contains("段数"), "実際のエラー: {}", e);
}

#[test]
fn d33_user_written_return_from_the_body_level() {
    // 「安全に寝る自由」。関数本体の直下では getdepth() == 1 なので
    // break 1段が return になる。
    let out = ok(r#"
        fn clamp (n : i64) {
            if n > 100 break 100;
            if n < 0 break 0;
            n
        } -> i64;
        print(clamp(500));
        print(clamp(-7));
    "#);
    assert_eq!(out, vec!["100", "0"]);
}

// ---- D-38: 作用素式 ----

#[test]
fn d38_nest_gives_a_dynamic_break_depth() {
    // `$repeat(break, getdepth())` は「今いる深さがいくつであれ関数を抜ける」。
    // これが return であり、段数がリテラルの break では書けない。
    let out = ok(r#"
        fn first_multiple (limit : i64, m : i64) {
            let i := 1;
            loop {
                if i > limit {
                    $repeat(break, getdepth()) 0;
                }
                if i mod m == 0 {
                    $repeat(break, getdepth()) i;
                }
                i += 1;
            }
            0
        } -> i64;
        print(first_multiple(100, 7));
        print(first_multiple(3, 7));
    "#);
    assert_eq!(out, vec!["7", "0"]);
}

#[test]
fn d38_nest_matches_written_out_breaks() {
    let a = ok("{ { $repeat(break, 2) 5; } print(1); } print(2);");
    let b = ok("{ { break break 5; } print(1); } print(2);");
    assert_eq!(a, b);
    assert_eq!(a, vec!["2"]);
}

#[test]
fn d38_juxtaposition_covers_fusion() {
    // `$repeat(break, n) continue` が fusion(nest(break,n), continue) を書く。
    // 内側ループを抜けて外側ループを次の周回へ。
    let out = ok(r#"
        let outer := 0;
        let inner := 0;
        loop {
            outer += 1;
            if outer > 3 break;
            loop {
                inner += 1;
                $repeat(break, 1) continue;
            }
        }
        print(outer);
        print(inner);
    "#);
    assert_eq!(out, vec!["4", "3"]);
}

#[test]
fn d38_break_goto_respects_the_block_but_goto_does_not() {
    // D-2r: ループ本体の journal は正常脱出で破棄、jump で適用。
    // `break goto` は正常脱出なので蓄積が残り、裸の `goto` は巻き戻る。
    let disciplined = ok(r#"
        let acc := 0;
        loop {
            acc += 1;
            break goto done;
        }
        label done;
        print(acc);
    "#);
    assert_eq!(disciplined, vec!["1"], "break goto は閉じてから跳ぶので acc が残る");

    let wild = ok(r#"
        let acc := 0;
        loop {
            acc += 1;
            goto done;
        }
        label done;
        print(acc);
    "#);
    assert_eq!(wild, vec!["0"], "裸の goto は jump なので acc が巻き戻る");
}

#[test]
fn d38_operators_are_not_first_class() {
    // 作用素は値ではないので束縛に入れられない（D-27 と同じ線）。
    let e = run("let f := $break;\n", vec![]).unwrap_err();
    assert!(!e.is_empty(), "束縛は拒否されるべき");
}

#[test]
fn d38_goto_cannot_be_nested() {
    // 行き先がラベルで決まるので重ねる意味がない。
    let e = run("loop { $repeat(goto a, 2) ; }\nlabel a;", vec![]).unwrap_err();
    assert!(e.contains("重ねられません"), "実際のエラー: {}", e);
}

#[test]
fn d38_continue_can_be_nested() {
    // `continue` は反復を1つ進める操作なので、重ねれば n 個進む。
    // `loop` は進める状態がないので退化する（嘘ではなく `x * 1` と同じ）。
    let out = ok(r#"
        let n := 0;
        loop {
            n += 1;
            if n == 3 break;
            $repeat(continue, 2);
        }
        print(n);
    "#);
    assert_eq!(out, vec!["3"]);
}

#[test]
fn d39_alias_body_may_carry_its_terminal() {
    // インラインで書けるものは名前を付けられる。
    let out = ok(r#"
        flow skip_two = $repeat(break, 1) $repeat(continue, 2);
        let n := 0;
        loop {
            n += 1;
            if n == 3 break;
            { skip_two; }
        }
        print(n);
    "#);
    assert_eq!(out, vec!["3"]);
}

#[test]
fn d39_terminal_cannot_be_doubled() {
    // 別名が終端を持っているなら、使用位置に重ねられない。
    let e = run(
        "flow done = $repeat(break, 1) continue;\nloop { done 5; }",
        vec![],
    )
    .unwrap_err();
    assert!(e.contains("既に終端を持っています"), "実際のエラー: {}", e);
}

// ---- D-26r: return は標準ライブラリ層の糖衣 ----

#[test]
fn d26r_return_equals_its_desugaring() {
    // `return E;` ≡ `$repeat(break, getdepth()) E;`
    let sugar = ok(r#"
        fn first_multiple (limit : i64, m : i64) {
            let i := 1;
            loop {
                if i > limit { return 0; }
                if i mod m == 0 { return i; }
                i += 1;
            }
            0
        } -> i64;
        print(first_multiple(100, 7));
        print(first_multiple(3, 7));
    "#);
    let core = ok(r#"
        fn first_multiple (limit : i64, m : i64) {
            let i := 1;
            loop {
                if i > limit { $repeat(break, getdepth()) 0; }
                if i mod m == 0 { $repeat(break, getdepth()) i; }
                i += 1;
            }
            0
        } -> i64;
        print(first_multiple(100, 7));
        print(first_multiple(3, 7));
    "#);
    assert_eq!(sugar, core);
    assert_eq!(sugar, vec!["7", "0"]);
}

#[test]
fn d26r_return_leaves_the_function_from_any_depth() {
    let out = ok(r#"
        fn deep (x : i64) {
            {
                loop {
                    {
                        return x * 2;
                    }
                }
            }
        } -> i64;
        print(deep(21));
    "#);
    assert_eq!(out, vec!["42"]);
}

// ---- フレーム境界（jump は depth 基準を跨げない） ----

#[test]
fn goto_cannot_see_labels_outside_the_frame() {
    // フレームの外の label は**見えない**。goto は行き先が要るのでエラー。
    let e = run(
        "fn f () { goto outside; 0 } -> i64;\nprint(f());\nlabel outside;",
        vec![],
    )
    .unwrap_err();
    assert!(e.contains("見つかりません"), "実際のエラー: {}", e);
}

#[test]
fn comefrom_registered_outside_is_invisible_inside() {
    // 見えないのはエラーではなく隠蔽。§4.1-3 により label は何もしない。
    let out = ok(r#"
        comefrom outer;
        fn f () {
            label outer;
            1
        } -> i64;
        print(f());
    "#);
    assert_eq!(out, vec!["1"], "label は何もせず素通りする");
}

#[test]
fn label_names_are_frame_local() {
    // 名前空間を持たない言語で、ラベルだけは自動的に閉じる。
    let out = ok(r#"
        fn a () { goto done; label done; 1 } -> i64;
        fn b () { goto done; label done; 2 } -> i64;
        print(a());
        print(b());
    "#);
    assert_eq!(out, vec!["1", "2"]);
}

#[test]
fn frame_operator_resets_the_depth_origin() {
    let out = ok(r#"
        print(getdepth());
        { print(getdepth()); print($frame { getdepth() }); }
    "#);
    assert_eq!(out, vec!["1", "2", "1"]);
}

#[test]
fn return_inside_frame_stops_at_the_frame() {
    let out = ok(r#"
        fn f () {
            let r := $frame { return 5; };
            r + 1
        } -> i64;
        print(f());
    "#);
    assert_eq!(out, vec!["6"]);
}

// ---- break comefrom（唯一、実行が続く終端） ----

#[test]
fn break_comefrom_registers_in_the_outer_context() {
    // 閉じたブロックの直後が戻り先になる。ループ構文を使わない無限反復。
    let out = ok(r#"
        fn f () {
            let n := 0;
            { break comefrom T; };
            n += 1;
            if n < 3 { label T; }
            n
        } -> i64;
        print(f());
    "#);
    assert_eq!(out, vec!["3"]);
}

// ---- トップレベル ----

#[test]
fn toplevel_break_value_becomes_the_exit_value() {
    let (out, code) = reod::run_exit("print(1);\nbreak 9;\nprint(2);", vec![]).unwrap();
    assert_eq!(out, vec!["1"]);
    assert_eq!(code, 9);
}

#[test]
fn toplevel_break_overflow_is_detected() {
    let e = run("break break break;", vec![]).unwrap_err();
    assert!(e.contains("段数が足りません"), "実際のエラー: {}", e);
}

#[test]
fn function_call_as_a_statement() {
    let out = ok(r#"
        fn side () { print(9); 0 } -> i64;
        side();
        print(1);
    "#);
    assert_eq!(out, vec!["9", "1"]);
}

// ---- D-67 / D-68: #[flat] とブロックコメント ----

#[test]
fn d67_flat_ignores_block_structure() {
    let out = ok(r#"
        #[flat]
        goto a;
        { } { } label a;
        print(7);
    "#);
    assert_eq!(out, vec!["7"]);
}

#[test]
fn d67_flat_is_rejected_outside_goto() {
    let e = run("#[flat]\nlet x := 1;\n", vec![]).unwrap_err();
    assert!(e.contains("`#[flat]`"), "実際のエラー: {}", e);
}

#[test]
fn d68_block_comment_from_the_standard_library() {
    // 飛ばされる区間は構文が壊れていても名前が未定義でも括弧が不均衡でもよい。
    let out = ok(r#"
        print(1);
        comment;
            ここは壊れていてよい { { {
            undefined( 1 2 3
        label _comment_;
        print(42);
    "#);
    assert_eq!(out, vec!["1", "42"]);
}

#[test]
fn d68_lazy_attribute_defers_to_the_use_site() {
    // 遅延しない #[flat] は flow 宣言に効かないので、別名は flat にならない。
    let e = run(
        r#"
        flow plain = goto _c_;
        plain;
        { label _c_; }
        print(1);
    "#,
        vec![],
    )
    .unwrap_err();
    assert!(e.contains("見つかりません"), "実際のエラー: {}", e);
}

// ---- D-29: continue ----

#[test]
fn d29_continue_starts_the_next_iteration() {
    let out = ok(r#"
        let i := 0;
        let evens := 0;
        loop {
            i += 1;
            if i > 10 break;
            if i mod 2 == 1 continue;
            evens += 1;
        }
        print(evens);
    "#);
    assert_eq!(out, vec!["5"]);
}

#[test]
fn d29_continue_does_not_rewind() {
    // continue は jump だがブロックを出ないので、非 global 状態は巻き戻らない。
    // 同じ形を goto で書くと巻き戻る（D-2r）ことと対比する。
    let out = ok(r#"
        let acc := 0;
        let i := 0;
        loop {
            i += 1;
            if i > 3 break acc;
            acc += 10;
            continue;
        }
        print(acc);
    "#);
    assert_eq!(out, vec!["30"]);
}

#[test]
fn d29_continue_outside_a_loop_is_rejected() {
    let e = run("fn f (x) { continue; x } -> i64;\nprint(f(1));", vec![]).unwrap_err();
    assert!(e.contains("continue"), "実際のエラー: {}", e);
}

// ---- D-31: jump は呼び出しフレームより下へは巻き戻らない ----

#[test]
fn d31_goto_inside_a_function_keeps_its_parameters() {
    // フレームの底で止めないと、引数束縛ごと消える。
    let out = ok(r#"
        fn solve (n : i64) {
            goto fallback;
            missing_module(n);
            label fallback;
            n * 2
        } -> i64;
        print(solve(21));
    "#);
    assert_eq!(out, vec!["42"]);
}

// ---- §4.2: comefrom トリガーの無効化範囲 ----

#[test]
fn comefrom_is_rejected_inside_a_function_body() {
    let e = run("fn f (x) { comefrom c; x } -> i64;\nprint(f(1));", vec![]).unwrap_err();
    assert!(e.contains("§4.2"), "実際のエラー: {}", e);
}

#[test]
fn comefrom_is_rejected_inside_a_loop_body() {
    let e = run("loop { comefrom c; break; }", vec![]).unwrap_err();
    assert!(e.contains("§4.2"), "実際のエラー: {}", e);
}

// ---- §4.6: 名前解決は強制の瞬間まで先送りされる ----

#[test]
fn late_name_resolution_and_clone_binding() {
    let out = ok(r#"
        let a := 5;
        let b := a;
        let c ^= a;
        {
            let a := 999;
            print(b);
        }
        print(c);
    "#);
    assert_eq!(out, vec!["999", "5"], "b は名前 a を、c は a の計算を持つ");
}

#[test]
fn alias_binding_shares_the_thunk() {
    let out = ok(r#"
        let a := 1;
        let b &= a;
        a := 9;
        print(b);
    "#);
    // `a := 9` は a の束縛を新しい thunk に差し替えるだけで、
    // b が共有しているセルは元のまま。
    assert_eq!(out, vec!["1"]);
}

// ---- D-6: `#[forward]` なしで再帰的な参照が解決される ----

#[test]
fn d30_forward_reference_is_rejected_without_rec() {
    // §3.3「束縛は逐次的に導入され、通常は前方参照を認めない」。
    let e = run("let b := a + 1;\nlet a := 41;\nprint(b);", vec![]).unwrap_err();
    assert!(e.contains("未束縛の名前: a"), "実際のエラー: {}", e);
}

#[test]
fn d30_rec_suppresses_the_check_and_late_resolution_does_the_rest() {
    // `#[forward]` が抑止するのは検査だけ。再帰そのものは名前の遅延解決で動く。
    let out = ok(r#"
        #[forward]
        let b := a + 1;
        let a := 41;
        print(b);
    "#);
    assert_eq!(out, vec!["42"]);
}

#[test]
fn d30_rec_is_rejected_on_fn() {
    // 関数本体は検査しないので、抑止するものがない。
    let e = run("#[forward]\nfn f (x) { x } -> i64;\n", vec![]).unwrap_err();
    assert!(e.contains("`#[forward]`"), "実際のエラー: {}", e);
}

#[test]
fn d30_fn_bodies_are_not_checked() {
    // §4.3 / 例4：goto で読み飛ばされる区間に未定義の名前があってよい。
    // これを守るために fn 本体は検査しない。
    let out = ok(r#"
        fn solve (n : i64) {
            goto fallback;
            missing_module(n);
            label fallback;
            n * 2
        } -> i64;
        print(solve(21));
    "#);
    assert_eq!(out, vec!["42"]);
}

#[test]
fn self_referential_thunk_is_detected() {
    // 検査が先に捕まえる。抑止すると本物の発散になる。
    let e = run("let x := x + 1;\nprint(x);", vec![]).unwrap_err();
    assert!(e.contains("未束縛の名前: x"), "実際のエラー: {}", e);

    let e = run("#[forward]\nlet x := x + 1;\nprint(x);", vec![]).unwrap_err();
    assert!(e.contains("自己参照"), "実際のエラー: {}", e);
}

// ---- §4.3 / 例4: goto 走査は文を実行しないので、壊れた区間を素通りできる ----

#[test]
fn goto_skips_over_code_that_would_fail_if_executed() {
    let out = ok(r#"
        let use_fast_path := 0;
        if use_fast_path == 0 goto fallback;

        % このモジュールは存在しない。実行されれば未束縛エラーになるが、
        % goto の走査は読み飛ばすだけなので問題にならない。
        accelerated_solve(nonexistent_arg);
        undefined_target := 1;
        goto done;

        label fallback;
        print(1);

        label done;
    "#);
    assert_eq!(out, vec!["1"]);
}

#[test]
fn scan_does_not_land_inside_a_deeper_block() {
    // 走査は相対深さ 0 以下でのみ照合する。
    let e = run(
        r#"
        goto inner;
        {
            label inner;
        }
    "#,
        vec![],
    )
    .unwrap_err();
    assert!(e.contains("見つかりません"), "実際のエラー: {}", e);
}

// ---- §4.5: read() の消費は強制の時点まで遅れる ----

#[test]
fn read_is_consumed_at_force_time() {
    // r1 は遅延、r2 は #[immediate]。強制順は r2 → r1 なので、
    // 最初の入力を受け取るのは r2 になる。
    let out = run(
        r#"
        let r1 := read();
        #[immediate]
        let r2 := read();
        print(r2);
        print(r1);
    "#,
        vec![10, 20],
    )
    .unwrap();
    assert_eq!(out, vec!["10", "20"]);
}

// ---- D-7 / D-11: 剰余は mod、短絡評価 ----

#[test]
fn mod_operator_and_percent_is_comment_only() {
    let out = ok("print(7 mod 3); % 剰余は mod\nprint(10 mod 4);");
    assert_eq!(out, vec!["1", "2"]);
}

#[test]
fn short_circuit_avoids_forcing_the_right_hand_side() {
    // 右辺が未束縛でも、短絡すれば評価されない。
    let out = ok("print(0 && undefined_name); print(1 || undefined_name);");
    assert_eq!(out, vec!["0", "1"]);
}

// ---- 自己言及代入：デフォルト遅延の帰結（設計書に記述なし） ----

#[test]
fn self_referential_assignment_diverges_without_immediate() {
    // `x := x / 2` は「x を更新」ではなく「`x / 2` というトークン列を指す
    // thunk を x に束縛」。§4.6 の名前遅延解決により、強制時に自分へ到達する。
    let e = run("let x := 8; x := x / 2; print(x);", vec![]).unwrap_err();
    assert!(e.contains("自己参照"), "実際のエラー: {}", e);
}

#[test]
fn immediate_makes_self_referential_assignment_work() {
    // 強制は束縛の**前**に起きるので、右辺の x はまだ旧束縛を指している。
    let out = ok(r#"
        let x := 8;
        #[immediate]
        x := x / 2;
        print(x);
    "#);
    assert_eq!(out, vec!["4"]);
}

#[test]
fn compound_assignment_forces_the_current_value_eagerly() {
    let out = ok("let x := 8; x -= 3; print(x);");
    assert_eq!(out, vec!["5"]);
}

// ---- 例3: Collatz（関数がないので loop に展開） ----

#[test]
fn collatz_steps_without_functions() {
    let out = ok(r#"
        let x := 27;
        let count := 0;
        loop {
            if x == 1 break;
            if x mod 2 == 0 {
                #[immediate]
                #[global]
                x := x / 2;
            } else {
                #[immediate]
                #[global]
                x := 3 * x + 1;
            }
            #[global]
            count += 1;
        }
        print(count);
    "#);
    assert_eq!(out, vec!["111"]);
}
