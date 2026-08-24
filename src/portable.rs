//! Portable vaak — WASM から呼ぶための入口。
//!
//! **二つの姿がある。**
//!
//! | | 誰が使うか | どう入れるか |
//! |---|---|---|
//! | **WASI**（`portable` の実行ファイル） | `wasmtime` / Node の `node:wasi` | 標準入力からソース、終了コードが答え |
//! | **素の WASM**（この module） | ブラウザ | 線形メモリに書いて番地を渡す |
//!
//! 素の側は **C の呼び出し規約だけ**を使う——`wasm-bindgen` を入れない。
//! 依存を増やさないという方針をここでも守る。
//!
//! ```js
//! const p = vaak_alloc(bytes.length);
//! new Uint8Array(memory.buffer, p, bytes.length).set(bytes);
//! const r = vaak_run(p, bytes.length);      // 下位 32 ビットが結果
//! const s = vaak_last_output();             // 表示（NUL 終端）
//! ```

use std::cell::RefCell;

thread_local! {
    /// 最後の結果の表示。**呼び出し側が読み終わるまで持っておく。**
    static OUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// 走らせた結果。**最上位の外界面は言語の意味論ではない**（C-31）ので、
/// **どう解釈するかは呼び出し側が決める**（C-95 契約 4）。
#[repr(i32)]
pub enum Status {
    /// 値が残った。表示は `vaak_last_output`
    Value = 0,
    /// 中身が空で終わった。**エラーではない**
    Empty = 1,
    /// 静的な誤り。表示に理由が入る
    Static = 2,
    /// 実行時の誤り
    Runtime = 3,
}

fn set_out(s: String) {
    OUT.with(|o| {
        let mut o = o.borrow_mut();
        o.clear();
        o.extend_from_slice(s.as_bytes());
        o.push(0);
    });
}

/// ソースを走らせる。返るのは [`Status`]。
///
/// # 安全性
///
/// `ptr` から `len` バイトが有効な UTF-8 でなければならない。
/// **呼び出し側が守る**——線形メモリの中身を知っているのは呼び出し側だけである。
#[cfg_attr(feature = "portable-exports", no_mangle)]
pub unsafe extern "C" fn vaak_run(ptr: *const u8, len: usize) -> i32 {
    let src = match std::str::from_utf8(std::slice::from_raw_parts(ptr, len)) {
        Ok(s) => s,
        Err(_) => {
            set_out("ソースが UTF-8 ではない".into());
            return Status::Static as i32;
        }
    };
    run_str(src)
}

fn run_str(src: &str) -> i32 {
    let prog = match crate::parser::parse(src) {
        Ok(p) => p,
        Err(e) => {
            let (l, c) = crate::span::line_col(src, e.span.start);
            set_out(format!("{l}:{c}: 構文: {}", e.msg));
            return Status::Static as i32;
        }
    };
    let mut errs: Vec<String> = crate::check::check(&prog).into_iter().map(|e| e.msg).collect();
    errs.extend(crate::types::check_types(&prog).into_iter().map(|e| e.msg));
    if !errs.is_empty() {
        set_out(errs.join("\n"));
        return Status::Static as i32;
    }
    let p2 = match crate::vm::compile(&prog) {
        Ok(p) => p,
        Err(e) => {
            set_out(e.msg);
            return Status::Static as i32;
        }
    };
    match crate::vm::run_program(&p2) {
        Ok(crate::interp::Eval::Value(v)) => {
            set_out(v.show());
            Status::Value as i32
        }
        Ok(_) => {
            set_out(String::new());
            Status::Empty as i32
        }
        Err(e) => {
            let (l, c) = crate::span::line_col(src, e.span.start);
            set_out(format!("{l}:{c}: {}", e.msg));
            Status::Runtime as i32
        }
    }
}

/// 最後の表示への番地。**NUL 終端。** 次に走らせるまで有効。
#[cfg_attr(feature = "portable-exports", no_mangle)]
pub extern "C" fn vaak_last_output() -> *const u8 {
    OUT.with(|o| o.borrow().as_ptr())
}

/// 線形メモリを借りる。**返した番地は [`vaak_free`] に返すこと。**
#[cfg_attr(feature = "portable-exports", no_mangle)]
pub extern "C" fn vaak_alloc(len: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(len);
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// # 安全性
///
/// `ptr` と `len` は [`vaak_alloc`] が返したものでなければならない。
#[cfg_attr(feature = "portable-exports", no_mangle)]
pub unsafe extern "C" fn vaak_free(ptr: *mut u8, len: usize) {
    drop(Vec::from_raw_parts(ptr, 0, len));
}
