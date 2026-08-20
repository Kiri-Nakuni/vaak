//! Portable vaak の入口。**ブラウザから呼ぶ道**を native でも確かめる。

fn run(src: &str) -> (i32, String) {
    let b = src.as_bytes();
    let st = unsafe { vaak::portable::vaak_run(b.as_ptr(), b.len()) };
    let p = vaak::portable::vaak_last_output();
    let mut n = 0;
    let out = unsafe {
        while *p.add(n) != 0 {
            n += 1;
        }
        String::from_utf8_lossy(std::slice::from_raw_parts(p, n)).into_owned()
    };
    (st, out)
}

#[test]
fn 値が残る() {
    assert_eq!(run("1 + 2 * 3"), (0, "7".to_string()));
}

#[test]
fn 中身が空なら誤りではない() {
    // **ホストに委ねる**（C-31）
    assert_eq!(run("var x := 1;").0, 1);
}

#[test]
fn 構文の誤り() {
    assert_eq!(run("1 +").0, 2);
}

#[test]
fn 型の誤りは静的に出る() {
    let (st, msg) = run("if (0) 1 fi");
    assert_eq!(st, 2, "{msg}");
}

#[test]
fn 文字列も表示できる() {
    assert_eq!(run("\"かたち\"").1, "\"かたち\"");
}

#[test]
fn 借りた場所を返せる() {
    let p = vaak::portable::vaak_alloc(16);
    assert!(!p.is_null());
    unsafe { vaak::portable::vaak_free(p, 16) };
}
