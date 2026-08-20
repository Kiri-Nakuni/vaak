//! ホスト界面（S-4）。
//!
//! **ホストが値の実体を持ち、この言語はそれを操作する。**
//! スクリプトは登録された名前を通してホストの状態に触れる。
//!
//! # 差し替え可能にしてある
//!
//! ホストが見せるものは [`HostBinding`] という**трейт**であり、
//! この言語は**中身の実体を知らない**。
//! rtex なら `\count` レジスタ、別のホストなら別のもの——
//! **「型・読み・書き」の三つを答えられれば何でもよい。**
//!
//! # 契約
//!
//! 1. **走っている間、ホストは見せたものを移動も解放もしない。**
//!    破れば未定義である——言語には守る手段が無い
//! 2. ホストは**実行の前後でだけ**差し替えられる
//! 3. 剥がしたいものは [`Host::invalidate`] する。**以後は名前ごと見えなくなる**
//! 4. ホストは**最上位の外界面**を受け取る（C-31）。値・paradox・脱出のいずれか

use crate::ast::ValueType;
use crate::interp::{Eval, Interp};
use crate::value::Value;

/// ホストが見せるもの。**中身の実体は Vaak の外にある。**
///
/// 実装するのは三つだけ:
///
/// - [`type_of`](HostBinding::type_of) — 検査器に渡す型。走る前に一度
/// - [`read`](HostBinding::read) — 走る前に一度。**写しを渡す**
/// - [`write`](HostBinding::write) — 走った後に一度。**変わっていれば書き戻す**
///
/// **呼ばれるのは実行の前後に一度ずつ**なので、動的分配の費用は無視できる。
///
/// 写しを渡す形なので、**契約 1（走っている間は動かさない）は自動的に守られる**——
/// Vaak が触るのは写しであり、ホストの実体ではない。
pub trait HostBinding {
    /// この名前の型。**検査器に渡す**ので、走る前に確定していなければならない。
    fn type_of(&self) -> ValueType;

    /// 走る前に読む。Vaak はこの値の写しを持って走る。
    fn read(&self) -> Value;

    /// 走った後に書き戻す。**同じなら呼ばれない。**
    fn write(&mut self, v: &Value);
}

/// もっとも単純な実装。**値をそのまま持つ。**
///
/// ホストが「ただの数」を見せたいだけならこれで足りる。
/// レジスタの束のような、**書き戻しに手続きが要るもの**は自分で実装する。
pub struct Cell {
    pub value: Value,
}

impl Cell {
    pub fn new(value: Value) -> Self {
        Self { value }
    }
}

impl HostBinding for Cell {
    fn type_of(&self) -> ValueType {
        self.value.type_of()
    }
    fn read(&self) -> Value {
        self.value.clone()
    }
    fn write(&mut self, v: &Value) {
        self.value = v.clone();
    }
}

/// 実行の結果。**最上位の外界面は言語の意味論ではない**（C-31）——
/// これをどう扱うかは**ホストが決める。**
#[derive(Clone, Debug)]
pub enum Outcome {
    Value(Value),
    /// 消費されなかった paradox。**発生点を受け取る**（C-46）。
    Paradox { line: usize, col: usize },
    /// 何も残らなかった（内面が空のまま終わった）。
    Empty,
    /// 静的エラー。**走らせる前に分かる。**
    Static(Vec<String>),
    /// 実行時エラー。
    Runtime { msg: String, line: usize, col: usize },
}

/// ホストの側。**名前と見せるものの対応を持つ。**
pub struct Host {
    bindings: Vec<(String, Box<dyn HostBinding>, bool)>,
    /// 検査を通してから走らせるか。**検査を通したプログラムを受け取る前提**（C-31）。
    pub check: bool,
    /// バイトコード VM で走らせるか。既定は木を辿る参照実装。
    pub use_vm: bool,
}

impl Host {
    pub fn new() -> Self {
        Self { bindings: Vec::new(), check: true, use_vm: false }
    }

    /// 名前を見せる。**同じ名前なら差し替える。**
    pub fn expose(&mut self, name: &str, b: Box<dyn HostBinding>) {
        match self.bindings.iter_mut().find(|(n, _, _)| n == name) {
            Some(slot) => {
                slot.1 = b;
                slot.2 = true;
            }
            None => self.bindings.push((name.to_string(), b, true)),
        }
    }

    /// 値をそのまま見せる短縮形。
    pub fn expose_value(&mut self, name: &str, v: Value) {
        self.expose(name, Box::new(Cell::new(v)));
    }

    /// 剥がす。**以後は名前ごと見えなくなる**（契約 3）。
    pub fn invalidate(&mut self, name: &str) {
        if let Some(slot) = self.bindings.iter_mut().find(|(n, _, _)| n == name) {
            slot.2 = false;
        }
    }

    /// 見せているものを読む。
    pub fn get(&self, name: &str) -> Option<&dyn HostBinding> {
        self.bindings
            .iter()
            .find(|(n, _, live)| n == name && *live)
            .map(|(_, b, _)| b.as_ref())
    }

    /// 走らせる。**実行の前後でだけ**見せたものが動く（契約 2）。
    pub fn run(&mut self, src: &str) -> Outcome {
        let prog = match crate::parser::parse(src) {
            Ok(p) => p,
            Err(e) => return Outcome::Static(vec![e.msg]),
        };
        let exposed: Vec<(String, ValueType)> = self
            .bindings
            .iter()
            .filter(|(_, _, live)| *live)
            .map(|(n, b, _)| (n.clone(), b.type_of()))
            .collect();

        if self.check {
            let mut errs: Vec<String> = crate::check::check_with_host(&prog, &exposed)
                .into_iter()
                .map(|e| e.msg)
                .collect();
            errs.extend(
                crate::types::check_types_with_host(&prog, &exposed).into_iter().map(|e| e.msg),
            );
            if !errs.is_empty() {
                return Outcome::Static(errs);
            }
        }

        // **木を辿る実装が参照実装である**（S-5）。VM は差し替えで選ぶ
        let out = if self.use_vm {
            let p = match crate::vm::compile(&prog) {
                Ok(p) => p,
                Err(e) => return Outcome::Static(vec![e.msg]),
            };
            // VM はホストの名前をまだ受け取れない
            if !exposed.is_empty() {
                return Outcome::Static(vec![
                    "VM はホストの名前をまだ受け取れない".into()
                ]);
            }
            crate::vm::run_program(&p)
                .map_err(|e| crate::interp::RuntimeError { msg: e.msg, span: e.span })
        } else {
            let mut it = Interp::new();
            for (name, b, live) in &self.bindings {
                if *live {
                    it.expose(name, b.read());
                }
            }
            let r = it.run(&prog);
            // 走り終わってから書き戻す
            for (name, b, live) in self.bindings.iter_mut() {
                if *live {
                    if let Some(v) = it.host_value(name) {
                        b.write(&v);
                    }
                }
            }
            r
        };

        match out {
            Ok(Eval::Value(v)) => Outcome::Value(v),
            Ok(Eval::Paradox(sp)) => {
                let (line, col) = crate::span::line_col(src, sp.start);
                Outcome::Paradox { line, col }
            }
            Ok(Eval::Akasha) => Outcome::Empty,
            Ok(Eval::Escape(x)) => {
                let (line, col) = crate::span::line_col(src, x.span.start);
                Outcome::Runtime { msg: "フレームを越える脱出".into(), line, col }
            }
            Err(e) => {
                let (line, col) = crate::span::line_col(src, e.span.start);
                Outcome::Runtime { msg: e.msg, line, col }
            }
        }
    }
}

impl Default for Host {
    fn default() -> Self {
        Self::new()
    }
}
