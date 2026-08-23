//! ホスト界面（S-4）。
//!
//! **ホストが値の実体を持ち、この言語はそれを操作する。**
//! スクリプトは登録された名前を通してホストの状態に触れる。
//!
//! # 差し替え可能にしてある
//!
//! ホストが見せるものは [`HostBinding`] という**トレイト**であり、
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
//! 4. ホストは**最上位の外界面**を受け取る（C-31）。値・paradox・脱出のいずれか。
//!    **その値が有効かどうかは、ホストが解釈する**——
//!    TeX なら `str` の外界面はその場に文字列トークンとして展開されるべきである

use crate::ast::{HostSig, ValueType};
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

    /// 要素だけを問う（S-15）。**答えられないなら `None`。**
    ///
    /// 集合体を見せるホストで、台本が**定数の添字しか使っていない**とき、
    /// 丸ごと写さずに済む。rtex の `\count` なら 256 個ではなく触った分だけになる。
    ///
    /// 既定は「答えられない」。**答えないホストは何もしなくてよい**——
    /// そのときは今までどおり丸ごと読む。
    fn read_at(&self, _i: usize) -> Option<Value> {
        None
    }

    /// 要素だけを書く（S-15）。**書けたなら `true`。**
    ///
    /// `read_at` と対で使う。片方しか答えられないなら、両方使われない。
    fn write_at(&mut self, _i: usize, _v: &Value) -> bool {
        false
    }

    /// 長さだけを問う（S-15）。**答えられないなら `None`。**
    ///
    /// 要素だけを渡すときも、**長さは合っていなければならない**——
    /// 台本が `.len()` を見るかもしれないし、範囲の外は paradox でなければならない。
    fn len(&self) -> Option<usize> {
        None
    }

    /// 空か。`len` が答えないなら答えられない。
    fn is_empty(&self) -> Option<bool> {
        self.len().map(|n| n == 0)
    }
}

/// ホストが**呼べる名前**として見せるもの（S-11）。
///
/// # なぜコールバックではないのか
///
/// **閉包が Vaak では表現できない。**
///
/// | 捕まえ方 | 何に反するか |
/// |---|---|
/// | 参照で | **C-48**——値は自己完結している。別名は値の中に入らない |
/// | 写しで | **C-33**——値は深く複製される。呼ぶたびに環境を全部写す |
///
/// だから「関数値を渡す」道は最初から閉じている。
/// 代わりに**ホストが答える名前**を置く——
/// スクリプトから見れば、ただの呼び出しである。
///
/// # 型はホストが宣言する
///
/// [`sig`](HostFn::sig) が引数と返り値の型を答えるので、
/// **検査器は無改造で働く。**
pub trait HostFn {
    /// 引数と返り値の型。**走る前に確定していなければならない。**
    fn sig(&self) -> HostSig;

    /// 呼ばれる。**返り値が `None` なら領域に値を置かない**（paradox）。
    fn call(&mut self, args: &[Value]) -> Option<Value>;
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

/// ホストへの問い合わせに答える側（S-11）。
///
/// **番号は登録順である。** 組み立ての時点で決まっているので、
/// 呼び出しのたびに名前を引き直さない。
struct Answer {
    fns: std::rc::Rc<std::cell::RefCell<Vec<(String, Box<dyn HostFn>)>>>,
}

impl crate::value::HostFns for Answer {
    fn call(&mut self, index: u16, args: &[Value]) -> Option<Value> {
        let mut fns = self.fns.borrow_mut();
        fns.get_mut(index as usize)?.1.call(args)
    }
}

/// ホストの側。**名前と見せるものの対応を持つ。**
pub struct Host {
    bindings: Vec<(String, Box<dyn HostBinding>, bool)>,
    /// 呼べる名前（S-11）。
    ///
    /// **持ち主はここのままである。** 走らせる間だけ `Rc` を貸す——
    /// 借りを `Interp` の欄に持てないので、共有にした
    fns: std::rc::Rc<std::cell::RefCell<Vec<(String, Box<dyn HostFn>)>>>,
    /// 検査を通してから走らせるか。**検査を通したプログラムを受け取る前提**（C-31）。
    pub check: bool,
    /// バイトコード VM で走らせるか。既定は木を辿る参照実装。
    pub use_vm: bool,
}

impl Host {
    pub fn new() -> Self {
        Self { bindings: Vec::new(), fns: Default::default(), check: true, use_vm: false }
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

    /// **呼べる名前**を見せる（S-11）。**同じ名前なら差し替える。**
    pub fn expose_fn(&mut self, name: &str, f: Box<dyn HostFn>) {
        let mut fns = self.fns.borrow_mut();
        match fns.iter_mut().find(|(n, _)| n == name) {
            Some(slot) => slot.1 = f,
            None => fns.push((name.to_string(), f)),
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
        let mut exposed: Vec<(String, crate::ast::HostItem)> = self
            .bindings
            .iter()
            .filter(|(_, _, live)| *live)
            .map(|(n, b, _)| (n.clone(), crate::ast::HostItem::Value(b.type_of())))
            .collect();
        exposed.extend(
            self.fns
                .borrow()
                .iter()
                .map(|(n, f)| (n.clone(), crate::ast::HostItem::Fn(f.sig()))),
        );

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
            let p = match crate::vm::compile_with_host(&prog, &exposed) {
                Ok(p) => p,
                Err(e) => return Outcome::Static(vec![e.msg]),
            };
            // **使わない名前は読まない。**
            //
            // rtex の `\count` のように束縛が何百もあるホストでは、
            // 「全部読む」がそのまま起動費になる。組み立て済みの命令列を見れば、
            // **どれが要るかは走らせる前に分かる**
            let reads = p.host_reads();
            let writes = p.host_writes();
            // **触った添字だけを問える束縛はどれか**（S-15）。
            // 定数の添字しか使っていなくて、ホストが要素で答えられるなら、
            // 丸ごと写さない——rtex の `\count` なら 256 個ではなく触った分だけになる
            let mut partial: Vec<Option<Vec<i128>>> = Vec::new();
            let values: Vec<Value> = self
                .bindings
                .iter()
                .filter(|(_, _, live)| *live)
                .enumerate()
                .map(|(i, (_, b, _))| {
                    // **書き戻すかもしれないなら読む。** 比べる相手が要るからである。
                    // 一度も触れていない名前だけを飛ばす
                    let need = reads.get(i).copied().unwrap_or(true)
                        || writes.get(i).copied().unwrap_or(true);
                    if !need {
                        partial.push(None);
                        // 触れないなら、型に合う空の値を置く。**書き戻しもしない**
                        return empty_of(&b.type_of());
                    }
                    if let Some(idx) = p.host_touched(i) {
                        if let Some(v) = element_view(b.as_ref(), &idx) {
                            partial.push(Some(idx));
                            return v;
                        }
                    }
                    partial.push(None);
                    b.read()
                })
                .collect();
            let mut answer = Answer { fns: self.fns.clone() };
            let before = values.clone();
            let (result, after) =
                crate::vm::run_program_with_fns_writeback(&p, values, &mut answer);
            let mut it = after.into_iter();
            let mut was = before.into_iter();
            let mut k = 0usize;
            for (_, b, live) in self.bindings.iter_mut() {
                if *live {
                    let old = was.next();
                    let touched = writes.get(k).copied().unwrap_or(true);
                    k += 1;
                    let part = partial.get(k - 1).cloned().flatten();
                    if let Some(v) = it.next() {
                        if !touched {
                            // **触れていないなら比べもしない**
                        } else if let Some(idx) = part {
                            // **触った要素だけ書き戻す。** 丸ごと書けば、
                            // 渡していない要素を零で潰してしまう
                            write_elements(b.as_mut(), &idx, &v, old.as_ref());
                        } else if old.as_ref() != Some(&v) {
                            // **同じなら書かない**（`HostBinding::write` の契約）。
                            // ホストによっては書き戻しが高い——rtex なら save stack が動く
                            b.write(&v);
                        }
                    }
                }
            }
            match result {
                Ok(ev) => Ok(ev),
                Err(e) => Err(crate::interp::RuntimeError { msg: e.msg, span: e.span }),
            }
        } else {
            let mut it = Interp::new();
            let mut before: Vec<Option<Value>> = Vec::with_capacity(self.bindings.len());
            for (name, b, live) in &self.bindings {
                if *live {
                    let v = b.read();
                    before.push(Some(v.clone()));
                    it.expose(name, v);
                } else {
                    before.push(None);
                }
            }
            // **呼べる名前を登録する**（S-11）。番号は登録順
            for (i, (name, _)) in self.fns.borrow().iter().enumerate() {
                it.expose_fn(name, i as u16);
            }
            let answer = Box::new(Answer { fns: self.fns.clone() });
            let r = it.run_with(&prog, answer);
            // 走り終わってから書き戻す。**同じなら書かない**（契約）
            for ((name, b, live), old) in self.bindings.iter_mut().zip(before) {
                if *live {
                    if let Some(v) = it.host_value(name) {
                        if old.as_ref() != Some(&v) {
                            b.write(&v);
                        }
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

/// 読まない束縛の場所へ置く値。**型に合う空**であればよい。
///
/// 書き戻さないと決めた枠なので、中身は観測されない。
fn empty_of(t: &ValueType) -> Value {
    use ValueType::*;
    match t {
        U1 => Value::U1(false),
        U8 => Value::U8(0),
        U16 => Value::U16(0),
        U32 => Value::U32(0),
        I32 => Value::I32(0),
        F32 => Value::F32(0.0),
        F64 => Value::F64(0.0),
        Str => Value::str(Vec::new()),
        Array(e) => Value::array((**e).clone(), Vec::new()),
        Map(k, v) => Value::map((**k).clone(), (**v).clone(), Default::default()),
        Hash(k, v) => {
            Value::Hash(Box::new(crate::value::HashVal::new((**k).clone(), (**v).clone())))
        }
        _ => Value::I64(0),
    }
}

/// 触った添字だけを埋めた集合体を作る（S-15）。
///
/// **長さは本物でなければならない**——台本が `.len()` を見るかもしれないし、
/// 枠の外は paradox でなければならない。
///
/// ホストが要素で答えられないなら `None`。**そのときは丸ごと読む。**
fn element_view(b: &dyn HostBinding, idx: &[i128]) -> Option<Value> {
    let ValueType::Array(el) = b.type_of() else { return None };
    let n = b.len()?;
    let mut items = vec![zero_of(&el); n];
    for &i in idx {
        // 枠の外の添字は paradox になるので、埋めなくてよい
        let Ok(u) = usize::try_from(i) else { continue };
        if u >= n {
            continue;
        }
        items[u] = b.read_at(u)?;
    }
    Some(Value::array((*el).clone(), items))
}

/// 触った添字だけ書き戻す。
fn write_elements(b: &mut dyn HostBinding, idx: &[i128], now: &Value, was: Option<&Value>) {
    let Value::Array(a) = now else { return };
    let old = match was {
        Some(Value::Array(o)) => Some(o),
        _ => None,
    };
    for &i in idx {
        let Ok(u) = usize::try_from(i) else { continue };
        let Some(v) = a.items.get(u) else { continue };
        // **同じなら書かない**（契約）
        if let Some(o) = old {
            if o.items.get(u) == Some(v) {
                continue;
            }
        }
        b.write_at(u, v);
    }
}

/// 型に合う零。**触らない要素の場所を埋める**ためだけに使う。
fn zero_of(t: &ValueType) -> Value {
    empty_of(t)
}
