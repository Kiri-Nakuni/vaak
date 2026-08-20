//! ホスト界面（S-4）。
//!
//! **ホストが値の実体を持ち、この言語はそれを操作する。**
//! スクリプトは登録された名前に `&=` で張れる。
//!
//! # 契約
//!
//! 1. **走っている間、ホストは登録したセルを移動も解放もしない。**
//!    破れば未定義である——言語には守る手段が無い
//! 2. ホストは**実行の前後でだけ**セルを差し替えられる
//! 3. 剥がしたいセルは `invalidate` する。**剥がされたセルへのアクセスは paradox**（C-46）
//! 4. ホストは**最上位の外界面**を受け取る（C-31）。値・paradox・脱出のいずれか

use crate::interp::{Eval, Interp};
use crate::value::Value;

/// ホストが渡す値。**実体はホストのもの**であり、ここでは写しを持つ。
///
/// 「値は自己完結している」（C-48）ので、写しを渡して書き戻せば足りる。
#[derive(Clone, Debug)]
pub enum HostCell {
    I64(i64),
    F64(f64),
    U8Array(Vec<u8>),
    I64Array(Vec<i64>),
    Str(String),
}

impl HostCell {
    /// この名前が持つ型。検査器に渡す。
    pub fn type_of(&self) -> crate::ast::ValueType {
        use crate::ast::ValueType as V;
        match self {
            HostCell::I64(_) => V::I64,
            HostCell::F64(_) => V::F64,
            HostCell::U8Array(_) => V::Array(Box::new(V::U8)),
            HostCell::I64Array(_) => V::Array(Box::new(V::I64)),
            HostCell::Str(_) => V::Str,
        }
    }

    fn to_value(&self) -> Value {
        use crate::ast::ValueType;
        match self {
            HostCell::I64(v) => Value::I64(*v),
            HostCell::F64(v) => Value::F64(*v),
            HostCell::U8Array(b) => Value::Array {
                elem: ValueType::U8,
                items: b.iter().map(|x| Value::U8(*x)).collect(),
            },
            HostCell::I64Array(a) => Value::Array {
                elem: ValueType::I64,
                items: a.iter().map(|x| Value::I64(*x)).collect(),
            },
            HostCell::Str(s) => Value::Str(s.as_bytes().to_vec()),
        }
    }

    fn from_value(&mut self, v: &Value) {
        match (self, v) {
            (HostCell::I64(dst), Value::I64(x)) => *dst = *x,
            (HostCell::F64(dst), Value::F64(x)) => *dst = *x,
            (HostCell::U8Array(dst), Value::Array { items, .. }) => {
                *dst = items.iter().filter_map(|x| x.as_int()).map(|x| x as u8).collect()
            }
            (HostCell::I64Array(dst), Value::Array { items, .. }) => {
                *dst = items.iter().filter_map(|x| x.as_int()).map(|x| x as i64).collect()
            }
            (HostCell::Str(dst), Value::Str(b)) => {
                *dst = String::from_utf8_lossy(b).into_owned()
            }
            _ => {}
        }
    }
}

/// 実行の結果。**最上位の外界面は言語の意味論ではない**（C-31）。
#[derive(Clone, Debug)]
pub enum Outcome {
    Value(Value),
    /// 消費されなかった paradox。**発生点を受け取る**（C-46）。
    Paradox { line: usize, col: usize },
    /// 何も残らなかった。
    Empty,
    /// 静的エラー。**走らせる前に分かる。**
    Static(Vec<String>),
    /// 実行時エラー。
    Runtime { msg: String, line: usize, col: usize },
}

/// ホストの側。
pub struct Host {
    cells: Vec<(String, HostCell, bool)>,
    /// 検査を通してから走らせるか。**検査を通したプログラムを受け取る前提**（C-31）。
    pub check: bool,
}

impl Host {
    pub fn new() -> Self {
        Self { cells: Vec::new(), check: true }
    }

    /// セルを名前で見せる。スクリプトは `&=` で張れる。
    pub fn expose(&mut self, name: &str, cell: HostCell) {
        match self.cells.iter_mut().find(|(n, _, _)| n == name) {
            Some(slot) => {
                slot.1 = cell;
                slot.2 = true;
            }
            None => self.cells.push((name.to_string(), cell, true)),
        }
    }

    /// セルを剥がす。**以後のアクセスは paradox になる**（契約 3）。
    pub fn invalidate(&mut self, name: &str) {
        if let Some(slot) = self.cells.iter_mut().find(|(n, _, _)| n == name) {
            slot.2 = false;
        }
    }

    pub fn get(&self, name: &str) -> Option<&HostCell> {
        self.cells.iter().find(|(n, _, live)| n == name && *live).map(|(_, c, _)| c)
    }

    /// 走らせる。**実行の前後でだけ**セルが差し替わる（契約 2）。
    pub fn run(&mut self, src: &str) -> Outcome {
        let prog = match crate::parser::parse(src) {
            Ok(p) => p,
            Err(e) => return Outcome::Static(vec![e.msg]),
        };
        let exposed: Vec<(String, crate::ast::ValueType)> = self
            .cells
            .iter()
            .filter(|(_, _, live)| *live)
            .map(|(n, c, _)| (n.clone(), c.type_of()))
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

        let mut it = Interp::new();
        for (name, cell, live) in &self.cells {
            if *live {
                it.expose(name, cell.to_value());
            }
        }
        let out = it.run(&prog);
        // 走り終わってから書き戻す
        for (name, cell, live) in self.cells.iter_mut() {
            if *live {
                if let Some(v) = it.host_value(name) {
                    cell.from_value(&v);
                }
            }
        }
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
