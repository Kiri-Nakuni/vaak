//! バイトコードのコンパイラと仮想機械。
//!
//! **木を辿る実装（`interp.rs`）が参照実装である。** ここは速い方の実装であり、
//! **同じテストに掛けて差分を取る**ことでしか正しさを確かめられない。
//!
//! 意味論はすべて `interp.rs` と共有する——値・標準ライブラリ・算術は同じ関数を呼ぶ。
//! **違うのは「どう辿るか」だけ。** 意味論を二度実装しない（C-61 の教訓）。

use crate::ast::*;
use crate::interp::{arith_pub, read_method_pub, try_coerce_pub, write_method_pub, EKind};
use crate::span::Span;
use crate::value::{Arena, CellId, Value};
use std::collections::{BTreeMap, HashMap};

// ================= 命令 =================

#[derive(Clone, Copy, Debug)]
pub enum Op {
    /// 定数を積む。
    Const(u32),
    /// paradox を積む。**値ではないが、領域の外界面として積む。**
    Paradox(Span),
    /// 名前の指すセルを読む。
    Load(u16),
    /// 関数引数として名前の指すセル自体を積む。
    /// **別名は値ではなく束縛の形態**なので、`Load` とは分ける（C-48）。
    Ref(u16),
    /// 名前の指すセルに書く（上を消費）。
    Store(u16),
    /// コンパイラが既に型を揃えた値をそのまま書く。値引数の入口だけで使う。
    StoreExact(u16),
    /// 名前を別の名前のセルへ向ける。**値は動かさない**（C-20）。
    Alias(u16, u16),
    /// その名前が指すセルを、現在の字句的な段／フレームの間だけ凍らせる。
    Freeze(u16),
    /// ホストが見せている**呼べる名前**を呼ぶ（S-11）。番号と引数の数。
    HostCall(u16, u16, Span),
    /// 新しいセルを作って上を入れる。
    Declare(u16),
    /// 上を捨てる。`;` の作用。
    Pop,
    /// 上が paradox なら捨てて次へ、そうでなければ飛ぶ。`??` の左。
    JumpIfValue(u32),
    Jump(u32),
    /// 上が偽（`u1` の 0）なら飛ぶ。上は消費する。
    JumpIfFalse(u32),
    /// 上が paradox なら飛ぶ。上は残す。
    JumpIfParadox(u32),
    /// 上二つを畳む。
    Bin(BinOp, Span),
    Un(UnOp, Span),
    /// 上が `u1` でなければ実行時エラー。`if` の条件。
    NeedU1(Span),
    /// 上が値でなければ実行時エラー（消費されなかった paradox）。
    NeedValue(Span),
    /// 呼び出し。
    Call(u32, u16, Span),
    /// メンバ関数。名前は定数表。
    Method(u32, u16, Span),
    /// 名前のセルにある集合体を、その場で変更する組み込みメンバ関数。
    /// レシーバを深く複製して `Store` し直さない。
    MutMethod(u16, u32, u16, Span),
    /// 集合体。
    MakeArray(u16),
    /// `new u8 array(x)`。`x` が `str` なら包みを剥がし、整数なら零で埋める。
    /// C-78 の構築は引数の型で通常の配列構築と見分ける。
    MakeU8ArrayOne,
    MakeMap(u16),
    MakeStruct(u32, u16),
    Index(Span),
    Field(u32, Span),
    /// 名前への添字を**一命令で**読む。**`[]` はアクセスであり複製しない**（C-20）——
    /// 集合体をスタックへ写さず、セルから要素だけを取る。
    LoadIndex(u16, Span),
    /// 名前の欄を一命令で読む。同上。
    LoadField(u16, u32, Span),
    /// 名前の長さを一命令で。集合体を写さない。
    LoadLen(u16, Span),
    /// 名前への添字に**一命令で書く**。**`[]` はアクセスであり複製しない**（C-20）——
    /// 集合体をスタックへ写して書き戻す、ということをしない。
    StoreIndex(u16, Span),
    /// 経路への書き込み。深さは添字・欄の列。
    SetIndex(Span),
    SetField(u32, Span),
    /// 脱出。段数・`outward` のビット列・積み荷の有無。
    Break { stages: u32, outward: u64, payload: bool, span: Span },
    Continue { deferred: Option<u32>, span: Span },
    /// 段数が実行時に決まる脱出（`$repeat`）。上に回数がある。
    BreakDyn { payload: bool, span: Span },
    /// 型注釈に合わせる。paradox はそのまま通す。
    Coerce(u32),
    /// フレームの深さを積む。`getdepth()`。
    Depth,
    Ret,
    /// 領域の始まり。**その領域の底を控える。**
    ///
    /// 領域は脱出段ではない（C-20）ので、段の底では代用できない——
    /// `1 + (2)` の `(2)` は被演算子位置の領域であり、
    /// **そこには既に左辺が積まれている。**
    RegionBegin,
    /// 領域の終わり。空なら paradox を積む。
    EndRegion(Span),

    // ---- ループ。**本体はその構文の段**であって、二重にはならない（C-64）----
    /// 反復回数を数え始める。
    LoopBegin,
    /// 本体を一周した。**脱出はここで受ける。** 続けるなら戻る。
    LoopBody(u32, Span),
    /// ループの終わり。値は反復回数、または脱出が置いた値。
    LoopEnd(Span),
    /// `nfor` の始まり。上に開始値と回数がある。
    NForBegin(u16, Span),
    /// `nfor` の次の周へ。
    NForNext(u32, Span),
    /// 上を複製する。`switch` の照合に使う。
    Dup,
    /// **裸のブロックは脱出段を作る**（C-64）。ループ・関数の本体は作らない
    BlockBegin,
    BlockEnd(Span),
}

/// 一つの関数（または最上位）。
#[derive(Clone, Debug, Default)]
pub struct Chunk {
    pub ops: Vec<Op>,
    pub consts: Vec<Value>,
    pub names: Vec<String>,
    pub types: Vec<Type>,
    /// このフレームが使う名前の数。
    pub nslots: u16,
    pub params: Vec<(u16, bool)>,
    /// メンバ関数のとき、第一引数が `var self` か（S-1）。
    pub self_is_var: bool,
    /// **ホストが見せている名前**の枠（S-4）。最上位の塊にだけ入る。
    /// 走らせる前に値を入れ、走り終わってから読み出す。
    pub host_slots: Vec<u16>,
    pub span: Span,
}

/// 全体。
#[derive(Clone, Debug, Default)]
pub struct Program2 {
    pub chunks: Vec<Chunk>,
    /// 関数の名前 → 番号。
    pub fn_index: HashMap<String, u32>,
    pub structs: HashMap<String, StructDecl>,
    pub wraps: HashMap<String, ValueType>,
    pub top: u32,
    /// ホストが見せている**呼べる名前**（S-11）。番号で引く
    pub host_fns: Vec<(String, HostSig)>,
}

// ================= コンパイラ =================

#[derive(Clone, Debug)]
pub struct CompileError {
    pub msg: String,
    pub span: Span,
}

pub fn compile(prog: &Program) -> Result<Program2, CompileError> {
    compile_with_host(prog, &[])
}

/// ホストが見せている名前を添えて組む（S-4）。
///
/// **名前は最上位の枠として先に取る。** 走らせる前に値を入れれば、
/// スクリプトからは最初から見えている状態で始まる。
pub fn compile_with_host(
    prog: &Program,
    host: &[(String, HostItem)],
) -> Result<Program2, CompileError> {
    let mut c = Compiler {
        out: Program2::default(),
        chunk: Chunk::default(),
        scopes: vec![HashMap::new()],
        frame_base: 0,
        flows: HashMap::new(),
        expanding_flows: Default::default(),
        var_self: Default::default(),
        fn_aliases: Default::default(),
        fn_param_types: Default::default(),
        want: None,
        host_fns: HashMap::new(),
    };
    // **呼べる名前を先に登録する。** 関数と同じくスコープ全体で見える（C-36 / S-11）
    for (n, item) in host {
        if let HostItem::Fn(sig) = item {
            let i = c.out.host_fns.len() as u16;
            c.out.host_fns.push((n.clone(), sig.clone()));
            c.host_fns.insert(n.clone(), i);
        }
    }
    // **この方言に無い型は断る**（S-20）
    if let Some((name, span)) = crate::interp::find_dialect_type(&prog.body) {
        return Err(CompileError {
            msg: format!("`{name}` はこの方言には無い型（STEEL 方言の型である）"),
            span,
        });
    }
    c.collect(&prog.body);
    // 無名標準ライブラリ（C-15）
    let prelude = crate::parser::parse("flow $return = $repeat(break, getdepth());").unwrap();
    c.collect(&prelude.body);

    // 先に関数を全部登録する。**宣言はスコープ全体で見える**（C-36）
    let fns = collect_fns(&prog.body);
    for (i, f) in fns.iter().enumerate() {
        let key = fn_key(f);
        c.out.fn_index.insert(key.clone(), i as u32 + 1);
        c.fn_aliases.insert(key.clone(), f.params.iter().map(|p| p.ty.is_alias).collect());
        c.fn_param_types
            .insert(key, f.params.iter().map(|p| p.ty.value.clone()).collect());
        if f.owner.is_some()
            && f.params.first().map(|p| p.kind == BindKind::Var).unwrap_or(false)
        {
            c.var_self.insert(f.name.clone());
        }
    }
    c.out.chunks.push(Chunk::default()); // 0 番は最上位の予約
    for f in &fns {
        let ch = c.function(f)?;
        c.out.chunks.push(ch);
    }
    // 最上位。**領域でありスコープでありフレームである**
    c.chunk = Chunk::default();
    // ホストの名前を先に枠へ。**スクリプトからは最初から見えている**
    for (n, item) in host {
        if matches!(item, HostItem::Value(_)) {
            let slot = c.slot(n);
            c.chunk.host_slots.push(slot);
        }
    }
    c.region(&prog.body, Span::NONE)?;
    c.emit(Op::Ret);
    c.out.chunks[0] = std::mem::take(&mut c.chunk);
    c.out.top = 0;
    Ok(c.out)
}

fn collect_fns(body: &[Expr]) -> Vec<FnDecl> {
    let mut out = Vec::new();
    fn walk(e: &Expr, out: &mut Vec<FnDecl>) {
        let mut e = e;
        while let ExprKind::Discard(Some(i)) = &e.kind {
            e = i;
        }
        match &e.kind {
            ExprKind::FnDecl(f) => {
                out.push(f.clone());
                if let ExprKind::Block(items) = &f.body.kind {
                    for x in items {
                        walk(x, out);
                    }
                }
            }
            ExprKind::Block(items) | ExprKind::Paren(items) => {
                for x in items {
                    walk(x, out);
                }
            }
            ExprKind::Loop(b) => walk(b, out),
            ExprKind::While { body, .. } | ExprKind::NFor { body, .. } => walk(body, out),
            _ => {}
        }
    }
    for e in body {
        walk(e, &mut out);
    }
    out
}

struct Compiler {
    out: Program2,
    chunk: Chunk,
    /// 名前 → （枠、**注釈が言う型**）。型は文脈をリテラルへ届けるために持つ（C-100）
    scopes: Vec<HashMap<String, (u16, Option<ValueType>)>>,
    frame_base: usize,
    flows: HashMap<String, FlowDecl>,
    /// 検査を省いた低水準 API でも、不正な `flow` を有限の誤りにする。
    expanding_flows: std::collections::HashSet<String>,
    /// `var self` を取るメンバ関数の名前（S-1）。**破壊するので書き戻す。**
    var_self: std::collections::HashSet<String>,
    /// 利用者定義関数の引数が `alias` か。前方呼び出しも含めて先に集める。
    fn_aliases: HashMap<String, Vec<bool>>,
    /// 引数の型。**文脈をリテラルへ届けるために持つ**（C-100）
    fn_param_types: HashMap<String, Vec<ValueType>>,
    /// **文脈が求めている型**（C-100）。リテラルがこれを受け取る。
    ///
    /// `expr` の先頭で必ず取り上げられるので**一段しか届かない。**
    /// 通したい枝が置き直す。置き忘れは「今までどおり `i64`」に落ちるだけである。
    want: Option<ValueType>,
    /// ホストが見せている**呼べる名前** → 番号（S-11）
    host_fns: HashMap<String, u16>,
}

impl Compiler {
    fn err<T>(&self, msg: impl Into<String>, span: Span) -> Result<T, CompileError> {
        Err(CompileError { msg: msg.into(), span })
    }

    fn emit(&mut self, op: Op) -> usize {
        self.chunk.ops.push(op);
        self.chunk.ops.len() - 1
    }

    fn here(&self) -> u32 {
        self.chunk.ops.len() as u32
    }

    fn patch(&mut self, at: usize) {
        let target = self.here();
        match &mut self.chunk.ops[at] {
            Op::Jump(t)
            | Op::JumpIfFalse(t)
            | Op::JumpIfParadox(t)
            | Op::JumpIfValue(t) => *t = target,
            _ => unreachable!(),
        }
    }

    fn konst(&mut self, v: Value) -> u32 {
        self.chunk.consts.push(v);
        self.chunk.consts.len() as u32 - 1
    }

    fn name_idx(&mut self, n: &str) -> u32 {
        if let Some(i) = self.chunk.names.iter().position(|x| x == n) {
            return i as u32;
        }
        self.chunk.names.push(n.to_string());
        self.chunk.names.len() as u32 - 1
    }

    fn type_idx(&mut self, t: &Type) -> u32 {
        self.chunk.types.push(t.clone());
        self.chunk.types.len() as u32 - 1
    }

    fn collect(&mut self, body: &[Expr]) {
        for e in body {
            let mut e = e;
            while let ExprKind::Discard(Some(i)) = &e.kind {
                e = i;
            }
            match &e.kind {
                ExprKind::StructDecl(s) => {
                    self.out.structs.insert(s.name.clone(), s.clone());
                }
                ExprKind::WrapDecl(w) => {
                    self.out.wraps.insert(w.name.clone(), w.base.value.clone());
                }
                ExprKind::FlowDecl(f) => {
                    self.flows.insert(f.name.clone(), f.clone());
                }
                _ => {}
            }
        }
    }

    fn slot(&mut self, n: &str) -> u16 {
        if let Some(s) = self.lookup(n) {
            return s;
        }
        let s = self.chunk.nslots;
        self.chunk.nslots += 1;
        self.scopes.last_mut().unwrap().insert(n.to_string(), (s, None));
        s
    }

    /// 名前に注釈の型を覚えさせる。**枠は既にある。**
    fn note_type(&mut self, n: &str, ty: Option<ValueType>) {
        for s in self.scopes[self.frame_base..].iter_mut().rev() {
            if let Some(e) = s.get_mut(n) {
                e.1 = ty;
                return;
            }
        }
    }

    fn declared_type(&self, n: &str) -> Option<ValueType> {
        for s in self.scopes[self.frame_base..].iter().rev() {
            if let Some(e) = s.get(n) {
                return e.1.clone();
            }
        }
        None
    }

    fn lookup(&self, n: &str) -> Option<u16> {
        for s in self.scopes[self.frame_base..].iter().rev() {
            if let Some(i) = s.get(n) {
                return Some(i.0);
            }
        }
        None
    }

    fn function(&mut self, f: &FnDecl) -> Result<Chunk, CompileError> {
        let saved_chunk = std::mem::take(&mut self.chunk);
        let saved_base = self.frame_base;
        self.scopes.push(HashMap::new());
        self.frame_base = self.scopes.len() - 1;
        self.chunk.span = f.span;

        for p in &f.params {
            let s = self.slot(&p.name);
            self.chunk.params.push((s, p.ty.is_alias));
            // `const` の別名引数は呼び出し元のセル自体を凍らせる。
            // 凍結はフレームの出口で一括して解く（C-35 / C-37）。
            if p.ty.is_alias && p.kind == BindKind::Const {
                self.emit(Op::Freeze(s));
            }
            // 値引数のリテラルは仮引数の型を受け取る（C-25）。
            // 呼び出し側で既定型の値にしても、ここでセルの幅へ揃える。
            if !p.ty.is_alias {
                self.emit(Op::Load(s));
                let ti = self.type_idx(&p.ty);
                self.emit(Op::Coerce(ti));
                self.emit(Op::StoreExact(s));
            }
        }
        self.chunk.self_is_var = f.owner.is_some()
            && f.params.first().map(|p| p.kind == BindKind::Var).unwrap_or(false);
        if let ExprKind::Block(items) = &f.body.kind {
            self.collect(items);
            // **返り値の型が本体の中まで届く**（C-100）
            let want = f.ret.as_ref().map(|t| t.value.clone());
            self.region_wanting(items, want, f.body.span)?;
        }
        if let Some(ret) = &f.ret {
            let ti = self.type_idx(ret);
            self.emit(Op::Coerce(ti));
        }
        self.emit(Op::Ret);

        self.frame_base = saved_base;
        self.scopes.pop();
        let ch = std::mem::replace(&mut self.chunk, saved_chunk);
        Ok(ch)
    }

    /// 領域。**値を一つ残す。** 何も残らなければ paradox を積む。
    fn region(&mut self, body: &[Expr], span: Span) -> Result<(), CompileError> {
        self.region_wanting(body, None, span)
    }

    /// **領域の値は一つだけ**（C-14）。どれがそれかは分からないので全部に置く。
    /// 受け取らない枝は先頭で捨てる（C-100）
    fn region_wanting(
        &mut self,
        body: &[Expr],
        want: Option<ValueType>,
        span: Span,
    ) -> Result<(), CompileError> {
        for e in body {
            self.want = want.clone();
            self.expr(e)?;
        }
        self.emit(Op::EndRegion(span));
        Ok(())
    }
}

impl Compiler {
    fn expr(&mut self, e: &Expr) -> Result<(), CompileError> {
        let want = self.want.take();
        match &e.kind {
            // **リテラルは置かれた場所の型を受け取る**（C-21）。
            // 演算の途中でも同じである（C-100）
            ExprKind::Int(s) => {
                let v = crate::interp::parse_int_pub(s, e.span)
                    .map_err(|x| CompileError { msg: x.msg, span: x.span })?;
                let v = crate::interp::coerce_lit_pub(v, &want);
                let k = self.konst(v);
                self.emit(Op::Const(k));
            }
            ExprKind::Float(s) => {
                let v = crate::interp::parse_float_pub(s, e.span)
                    .map_err(|x| CompileError { msg: x.msg, span: x.span })?;
                let v = crate::interp::coerce_lit_pub(v, &want);
                let k = self.konst(v);
                self.emit(Op::Const(k));
            }
            ExprKind::Str(s) => {
                let k = self.konst(Value::str(s.as_bytes().to_vec()));
                self.emit(Op::Const(k));
            }
            &ExprKind::Bool(b) => {
                let k = self.konst(Value::U1(b));
                self.emit(Op::Const(k));
            }
            ExprKind::Ascribe { expr, ty } => {
                // **注釈は式の中まで届く**（C-100）
                self.want = Some(ty.value.clone());
                self.expr(expr)?;
                self.emit(Op::NeedValue(e.span));
                let k = self.type_idx(ty);
                self.emit(Op::Coerce(k));
            }
            ExprKind::Name(n) => {
                let Some(s) = self.lookup(n) else {
                    return self.err(format!("知らない名前 `{n}`"), e.span);
                };
                self.emit(Op::Load(s));
            }

            // `( )` は領域を作る。スコープでも脱出段でもない
            ExprKind::Paren(b) => {
                // **被演算子位置の領域は、自分の底を持たねばならない。**
                // 段の底では足りない——左辺が既に積まれていることがある
                self.emit(Op::RegionBegin);
                self.region_wanting(b, want, e.span)?;
            }

            // 裸のブロックは領域・スコープ・**脱出段**の三つ（C-64）
            ExprKind::Block(b) => {
                self.emit(Op::BlockBegin);
                self.scopes.push(HashMap::new());
                self.collect(b);
                self.region_wanting(b, want, e.span)?;
                self.scopes.pop();
                self.emit(Op::BlockEnd(e.span));
            }

            ExprKind::Discard(inner) => {
                match inner {
                    Some(x) => {
                        self.expr(x)?;
                        self.emit(Op::Pop);
                    }
                    // 左辺が空の領域でも `;` は働く（C-80）
                    None => {}
                }
            }

            ExprKind::Unary { op, rhs } => {
                self.expr(rhs)?;
                self.emit(Op::NeedValue(rhs.span));
                self.emit(Op::Un(*op, e.span));
            }

            ExprKind::Binary { op, lhs, rhs } => self.binary(*op, lhs, rhs, want, e.span)?,

            ExprKind::Field { base, name } => {
                let n = self.name_idx(name);
                // 名前の欄なら一命令で。**複製しない**（C-20）
                if let ExprKind::Name(v) = &base.kind {
                    if let Some(slot) = self.lookup(v) {
                        self.emit(Op::LoadField(slot, n, e.span));
                        return Ok(());
                    }
                }
                self.expr(base)?;
                self.emit(Op::NeedValue(base.span));
                self.emit(Op::Field(n, e.span));
            }
            ExprKind::Index { base, index } => {
                // 名前への添字なら一命令で。**複製しない**（C-20）
                if let ExprKind::Name(v) = &base.kind {
                    if let Some(slot) = self.lookup(v) {
                        self.expr(index)?;
                        self.emit(Op::NeedValue(index.span));
                        self.emit(Op::LoadIndex(slot, e.span));
                        return Ok(());
                    }
                }
                self.expr(base)?;
                self.emit(Op::NeedValue(base.span));
                self.expr(index)?;
                self.emit(Op::NeedValue(index.span));
                self.emit(Op::Index(e.span));
            }

            ExprKind::Call { callee, args } => self.call(callee, args, e.span)?,

            ExprKind::ArrayLit(items) => {
                // **注釈が言う要素の型が届く**（C-100）
                let el = match &want {
                    Some(ValueType::Array(x)) => Some((**x).clone()),
                    Some(ValueType::Str) => Some(ValueType::U8),
                    _ => None,
                };
                for it in items {
                    self.want = el.clone();
                    self.expr(it)?;
                    self.emit(Op::NeedValue(it.span));
                }
                self.emit(Op::MakeArray(items.len() as u16));
            }
            ExprKind::MapLit(pairs) => {
                for (k, v) in pairs {
                    self.expr(k)?;
                    self.emit(Op::NeedValue(k.span));
                    self.expr(v)?;
                    self.emit(Op::NeedValue(v.span));
                }
                self.emit(Op::MakeMap(pairs.len() as u16));
            }
            ExprKind::Construct { ty, args } => self.construct(ty, args, e.span)?,

            ExprKind::Decl(d) => self.decl(d, e.span)?,
            ExprKind::Assign { op, lhs, rhs } => self.assign(*op, lhs, rhs, e.span)?,

            // 宣言は値を置かない。外界面は paradox
            ExprKind::FnDecl(_)
            | ExprKind::StructDecl(_)
            | ExprKind::FlowDecl(_)
            | ExprKind::WrapDecl(_) => {
                self.emit(Op::Paradox(e.span));
            }

            ExprKind::If(i) => self.if_expr(i, want, e.span)?,
            ExprKind::Loop(b) => self.loop_expr(None, b, e.span)?,
            ExprKind::While { cond, body } => self.while_expr(cond, body, e.span)?,
            ExprKind::NFor { name, start, count, body } => {
                self.nfor(name, start, count, body, e.span)?
            }
            ExprKind::Switch { subject, arms } => self.switch(subject, arms, e.span)?,

            ExprKind::Escape(esc) => self.escape(esc, e.span)?,
        }
        Ok(())
    }

    fn binary(
        &mut self,
        op: BinOp,
        lhs: &Expr,
        rhs: &Expr,
        want: Option<ValueType>,
        span: Span,
    ) -> Result<(), CompileError> {
        // `??` は paradox の除去子。左が値ならそれ、paradox なら右
        if op == BinOp::Coalesce {
            // **どちらも同じ場所に置かれる**（C-100）
            self.want = want.clone();
            self.expr(lhs)?;
            let j = self.emit(Op::JumpIfValue(0));
            self.emit(Op::Pop);
            self.want = want;
            self.expr(rhs)?;
            self.patch(j);
            return Ok(());
        }
        // `&&` / `||` は短絡する
        if op == BinOp::And || op == BinOp::Or {
            self.expr(lhs)?;
            self.emit(Op::NeedValue(lhs.span));
            let k = self.konst(Value::U1(op == BinOp::Or));
            self.emit(Op::Const(k));
            self.emit(Op::Bin(BinOp::Eq, span));
            let j = self.emit(Op::JumpIfFalse(0));
            let k2 = self.konst(Value::U1(op == BinOp::Or));
            self.emit(Op::Const(k2));
            let done = self.emit(Op::Jump(0));
            self.patch(j);
            self.expr(rhs)?;
            self.emit(Op::NeedValue(rhs.span));
            let z = self.konst(Value::U1(false));
            self.emit(Op::Const(z));
            self.emit(Op::Bin(BinOp::Ne, span));
            self.patch(done);
            return Ok(());
        }
        // `|>` は構文の水準の糖衣
        if op == BinOp::Feed {
            let ExprKind::Call { callee, args } = &rhs.kind else {
                return self.err("`|>` の右辺は呼び出しでなければならない", span);
            };
            let mut all = vec![lhs.clone()];
            all.extend(args.iter().cloned());
            return self.call(callee, &all, span);
        }
        // **比較は結果が `u1`。** 外の型は左右へ届かない（型検査器と同じ扱い）
        let cmp = matches!(
            op,
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Eq | BinOp::Ne
        );
        // **右は左に揃う**（C-21：暗黙変換は無い）。
        // 左が名前なら注釈の型が分かるので、それを使う
        let left_ty = self.static_type(lhs);
        let for_lhs = if cmp { left_ty.clone() } else { want.or(left_ty.clone()) };
        self.want = for_lhs.clone();
        self.expr(lhs)?;
        self.emit(Op::NeedValue(lhs.span));
        self.want = if matches!(op, BinOp::Shl | BinOp::Shr) {
            Some(ValueType::I64)
        } else {
            left_ty.or(for_lhs)
        };
        self.expr(rhs)?;
        self.emit(Op::NeedValue(rhs.span));
        self.emit(Op::Bin(op, span));
        Ok(())
    }

    /// 組み立て時に型が分かる式か。**名前と注釈だけ。**
    fn static_type(&self, e: &Expr) -> Option<ValueType> {
        match &e.kind {
            ExprKind::Name(n) => self.declared_type(n),
            ExprKind::Ascribe { ty, .. } => Some(ty.value.clone()),
            ExprKind::Paren(v) if v.len() == 1 => self.static_type(&v[0]),
            _ => None,
        }
    }

    fn decl(&mut self, d: &Decl, span: Span) -> Result<(), CompileError> {
        for b in &d.bindings {
            match &b.init {
                BindInit::Value(e) => {
                    // **注釈は式の中まで届く**（C-100）
                    self.want = b.ty.as_ref().map(|t| t.value.clone());
                    self.expr(e)?;
                    self.emit(Op::NeedValue(e.span));
                    if let Some(t) = &b.ty {
                        let ti = self.type_idx(t);
                        self.emit(Op::Coerce(ti));
                    }
                    let s = self.slot(&b.name);
                    self.note_type(&b.name, b.ty.as_ref().map(|t| t.value.clone()));
                    self.emit(Op::Declare(s));
                }
                BindInit::AliasOf(t) => {
                    let Some(src) = self.lookup(t) else {
                        return self.err(format!("知らない名前 `{t}`"), b.span);
                    };
                    let dst = self.slot(&b.name);
                    if d.kind == BindKind::Const {
                        // `const` は経路ではなくセルの性質。
                        // 元の名前からの書き込みも同じ間は禁じる（C-35）。
                        self.emit(Op::Freeze(src));
                    }
                    self.emit(Op::Alias(dst, src));
                }
            }
        }
        self.emit(Op::Paradox(span));
        Ok(())
    }

    fn assign(
        &mut self,
        op: AssignOp,
        lhs: &Expr,
        rhs: &Expr,
        span: Span,
    ) -> Result<(), CompileError> {
        if op == AssignOp::Alias {
            let (ExprKind::Name(n), ExprKind::Name(t)) = (&lhs.kind, &rhs.kind) else {
                return self.err("`&=` は名前どうしでなければならない", span);
            };
            let (Some(dst), Some(src)) = (self.lookup(n), self.lookup(t)) else {
                return self.err("知らない名前", span);
            };
            self.emit(Op::Alias(dst, src));
            self.emit(Op::Paradox(span));
            return Ok(());
        }
        // 左辺の場所を先に解決してから右辺を評価する（C-79 (6)）
        let bop = match op {
            AssignOp::Set => None,
            AssignOp::Add => Some(BinOp::Add),
            AssignOp::Sub => Some(BinOp::Sub),
            AssignOp::Mul => Some(BinOp::Mul),
            AssignOp::Div => Some(BinOp::Div),
            AssignOp::Mod => Some(BinOp::Mod),
            AssignOp::Shl => Some(BinOp::Shl),
            AssignOp::Shr => Some(BinOp::Shr),
            AssignOp::BitXor => Some(BinOp::BitXor),
            AssignOp::BitOr => Some(BinOp::BitOr),
            AssignOp::Alias => unreachable!(),
        };
        match &lhs.kind {
            ExprKind::Name(n) => {
                let Some(s) = self.lookup(n) else {
                    return self.err(format!("知らない名前 `{n}`"), lhs.span);
                };
                // **置き場の型が右辺の中まで届く**（C-100）
                let ty = self.declared_type(n);
                if let Some(b) = bop {
                    self.emit(Op::Load(s));
                    self.emit(Op::NeedValue(lhs.span));
                    self.want = ty;
                    self.expr(rhs)?;
                    self.emit(Op::NeedValue(rhs.span));
                    self.emit(Op::Bin(b, span));
                } else {
                    self.want = ty;
                    self.expr(rhs)?;
                    self.emit(Op::NeedValue(rhs.span));
                }
                self.emit(Op::NeedValue(span));
                self.emit(Op::Store(s));
            }
            // 名前への添字なら一命令で書く。**複製しない**（C-20）
            ExprKind::Index { base, index } if matches!(base.kind, ExprKind::Name(_)) => {
                let ExprKind::Name(n) = &base.kind else { unreachable!() };
                let Some(slot) = self.lookup(n) else {
                    return self.err(format!("知らない名前 `{n}`"), base.span);
                };
                if let Some(b) = bop {
                    self.expr(index)?;
                    self.emit(Op::NeedValue(index.span));
                    self.emit(Op::LoadIndex(slot, span));
                    self.expr(rhs)?;
                    self.emit(Op::NeedValue(rhs.span));
                    self.emit(Op::Bin(b, span));
                    self.emit(Op::NeedValue(span));
                } else {
                    self.expr(rhs)?;
                    self.emit(Op::NeedValue(rhs.span));
                }
                self.expr(index)?;
                self.emit(Op::NeedValue(index.span));
                self.emit(Op::StoreIndex(slot, span));
            }
            ExprKind::Index { base, index } => {
                self.expr(base)?;
                self.emit(Op::NeedValue(base.span));
                self.expr(index)?;
                self.emit(Op::NeedValue(index.span));
                // SetIndex は [値, 場, 添字] の順に積まれているものとして畳む
                if let Some(b) = bop {
                    self.emit(Op::Index(span));
                    self.expr(rhs)?;
                    self.emit(Op::NeedValue(rhs.span));
                    self.emit(Op::Bin(b, span));
                    self.emit(Op::NeedValue(span));
                } else {
                    self.stack_drop2();
                    self.expr(rhs)?;
                    self.emit(Op::NeedValue(rhs.span));
                }
                self.expr(base)?;
                self.emit(Op::NeedValue(base.span));
                self.expr(index)?;
                self.emit(Op::NeedValue(index.span));
                self.emit(Op::SetIndex(span));
                self.store_back(base)?;
            }
            ExprKind::Field { base, name } => {
                let n = self.name_idx(name);
                self.expr(base)?;
                self.emit(Op::NeedValue(base.span));
                if let Some(b) = bop {
                    self.emit(Op::Field(n, span));
                    self.expr(rhs)?;
                    self.emit(Op::NeedValue(rhs.span));
                    self.emit(Op::Bin(b, span));
                    self.emit(Op::NeedValue(span));
                } else {
                    self.emit(Op::Pop);
                    self.expr(rhs)?;
                    self.emit(Op::NeedValue(rhs.span));
                }
                self.expr(base)?;
                self.emit(Op::NeedValue(base.span));
                self.emit(Op::SetField(n, span));
                self.store_back(base)?;
            }
            _ => return self.err("代入の左辺は経路でなければならない", lhs.span),
        }
        self.emit(Op::Paradox(span));
        Ok(())
    }

    /// 経路の根へ書き戻す。値は自己完結しているので、根まで戻せば足りる。
    /// `a[i] := v` で、場所を積む前に不要な二つを捨てる。
    fn stack_drop2(&mut self) {
        self.emit(Op::Pop);
        self.emit(Op::Pop);
    }

    fn store_back(&mut self, base: &Expr) -> Result<(), CompileError> {
        match &base.kind {
            ExprKind::Name(n) => {
                let Some(s) = self.lookup(n) else {
                    return self.err(format!("知らない名前 `{n}`"), base.span);
                };
                self.emit(Op::Store(s));
                Ok(())
            }
            ExprKind::Index { base: b2, index } => {
                self.expr(b2)?;
                self.emit(Op::NeedValue(b2.span));
                self.expr(index)?;
                self.emit(Op::NeedValue(index.span));
                self.emit(Op::SetIndex(base.span));
                self.store_back(b2)
            }
            ExprKind::Field { base: b2, name } => {
                let n = self.name_idx(name);
                self.expr(b2)?;
                self.emit(Op::NeedValue(b2.span));
                self.emit(Op::SetField(n, base.span));
                self.store_back(b2)
            }
            _ => self.err("代入の左辺は経路でなければならない", base.span),
        }
    }
}

impl Compiler {
    fn call(&mut self, callee: &Expr, args: &[Expr], span: Span) -> Result<(), CompileError> {
        // メンバ関数。レシーバは経路でよい（C-64）
        if let ExprKind::Field { base, name } = &callee.kind {
            // `x.len()` は長さだけ要る。**集合体を写さない**
            if name == "len" && args.is_empty() {
                if let ExprKind::Name(v) = &base.kind {
                    if let Some(slot) = self.lookup(v) {
                        self.emit(Op::LoadLen(slot, span));
                        return Ok(());
                    }
                }
            }
            // 組み込みの破壊的操作を名前へ掛けるなら、セルを直接変更できる。
            // 引数はこれまでどおり左から右に評価し、変更はその後に行う。
            if is_destructive(name) {
                if let ExprKind::Name(v) = &base.kind {
                    if let Some(slot) = self.lookup(v) {
                        for a in args {
                            self.expr(a)?;
                            self.emit(Op::NeedValue(a.span));
                        }
                        let n = self.name_idx(name);
                        self.emit(Op::MutMethod(slot, n, args.len() as u16, span));
                        return Ok(());
                    }
                }
            }
            self.expr(base)?;
            self.emit(Op::NeedValue(base.span));
            for a in args {
                // 利用者定義メソッドは、実行時にレシーバの型から選ばれる。
                // そのためここでは追加引数が値か alias かをまだ決められない。
                // 名前ならセルと、この時点の値の写しを両方積み、選んだ宣言に
                // 従って Op::Method が片方を使う。写しを今取ることで、後続引数が
                // 同じセルを書き換えても左から右の評価順を保つ（C-79）。
                if let ExprKind::Name(v) = &a.kind {
                    let Some(slot) = self.lookup(v) else {
                        return self.err(format!("知らない名前 `{v}`"), a.span);
                    };
                    self.emit(Op::Ref(slot));
                }
                self.expr(a)?;
                self.emit(Op::NeedValue(a.span));
            }
            let n = self.name_idx(name);
            self.emit(Op::Method(n, args.len() as u16, span));
            // 破壊的なら書き戻す。上には結果が、その下にレシーバが残る
            if is_destructive(name) || self.user_destructive(name) {
                self.store_back(base)?;
            }
            return Ok(());
        }
        let ExprKind::Name(name) = &callee.kind else {
            return self.err("呼び出せるのは名前だけ（第一級関数は無い）", callee.span);
        };
        if name == "getdepth" {
            self.emit(Op::Depth);
            return Ok(());
        }
        // **ホストが答える名前**（S-11）。
        // 値と違って、**呼べる名前はスコープ全体で見える**（C-36 と同じ扱い）
        if let Some(&hi) = self.host_fns.get(name) {
            for a in args {
                self.expr(a)?;
                self.emit(Op::NeedValue(a.span));
            }
            self.emit(Op::HostCall(hi, args.len() as u16, span));
            return Ok(());
        }
        let Some(idx) = self.out.fn_index.get(name).copied() else {
            return self.err(format!("知らない関数 `{name}`"), callee.span);
        };
        let aliases = self.fn_aliases.get(name).cloned().unwrap_or_default();
        let ptys = self.fn_param_types.get(name).cloned().unwrap_or_default();
        for (i, a) in args.iter().enumerate() {
            // **引数の型が式の中まで届く**（C-100）
            self.want = ptys.get(i).cloned();
            self.argument(a, aliases.get(i).copied().unwrap_or(false))?;
        }
        self.emit(Op::Call(idx, args.len() as u16, span));
        Ok(())
    }

    /// 別名引数は名前のセル、値引数はその場で複製した値を積む。
    fn argument(&mut self, arg: &Expr, is_alias: bool) -> Result<(), CompileError> {
        let want = self.want.take();
        if is_alias {
            let ExprKind::Name(name) = &arg.kind else {
                return self.err("`alias` 引数に渡せるのは名前だけ", arg.span);
            };
            let Some(slot) = self.lookup(name) else {
                return self.err(format!("知らない名前 `{name}`"), arg.span);
            };
            self.emit(Op::Ref(slot));
        } else {
            self.want = want;
            self.expr(arg)?;
            self.emit(Op::NeedValue(arg.span));
        }
        Ok(())
    }

    fn construct(&mut self, ty: &Type, args: &CtorArgs, span: Span) -> Result<(), CompileError> {
        match (&ty.value, args) {
            (ValueType::Named(n), CtorArgs::Named(given)) => {
                let Some(s) = self.out.structs.get(n).cloned() else {
                    return self.err(format!("知らない型 `{n}`"), span);
                };
                for f in &s.fields {
                    // **欄の型が式の中まで届く**（C-100）
                    let fty = Some(f.ty.value.clone());
                    match given.iter().find(|(g, _)| g == &f.name) {
                        Some((_, e)) => {
                            self.want = fty;
                            self.expr(e)?
                        }
                        None => match &f.default {
                            Some(d) => {
                                self.want = fty;
                                self.expr(d)?
                            }
                            None => return self.err(format!("欄 `{}` に値が無い", f.name), span),
                        },
                    }
                    self.emit(Op::NeedValue(span));
                    let ti = self.type_idx(&f.ty);
                    self.emit(Op::Coerce(ti));
                }
                let ni = self.name_idx(n);
                self.emit(Op::MakeStruct(ni, s.fields.len() as u16));
            }
            (ValueType::Named(n), CtorArgs::Positional(a))
                if a.is_empty() && self.out.structs.contains_key(n) =>
            {
                let s = self.out.structs[n].clone();
                for f in &s.fields {
                    // **欄の型が既定の式の中まで届く**（C-100）
                    self.want = Some(f.ty.value.clone());
                    let Some(d) = &f.default else {
                        return self.err(format!("欄 `{}` に値が無い", f.name), span);
                    };
                    self.expr(d)?;
                    self.emit(Op::NeedValue(span));
                    let ti = self.type_idx(&f.ty);
                    self.emit(Op::Coerce(ti));
                }
                let ni = self.name_idx(n);
                self.emit(Op::MakeStruct(ni, s.fields.len() as u16));
            }
            // ラップ型（S-2）
            (ValueType::Named(n), CtorArgs::Positional(a))
                if a.len() == 1 && self.out.wraps.contains_key(n) =>
            {
                self.expr(&a[0])?;
                self.emit(Op::NeedValue(span));
                let ni = self.name_idx(n);
                self.emit(Op::MakeStruct(ni, 1));
            }
            (ValueType::Array(elem), CtorArgs::Positional(a))
                if **elem == ValueType::U8 && a.len() == 1 =>
            {
                self.expr(&a[0])?;
                self.emit(Op::NeedValue(span));
                self.emit(Op::MakeU8ArrayOne);
            }
            (ValueType::Array(elem), CtorArgs::Positional(a)) => {
                if a.is_empty() {
                    let k = self.konst(Value::array((**elem).clone(), vec![]));
                    self.emit(Op::Const(k));
                } else {
                    self.expr(&a[0])?;
                    self.emit(Op::NeedValue(span));
                    if a.len() > 1 {
                        self.expr(&a[1])?;
                    } else {
                        let k = self.konst(Value::I64(0));
                        self.emit(Op::Const(k));
                    }
                    self.emit(Op::NeedValue(span));
                    let ti = self.type_idx(ty);
                    self.emit(Op::Coerce(ti));
                    self.emit(Op::MakeArray(u16::MAX)); // MAX は「回数と値から作る」印
                }
            }
            (ValueType::Map(k, v), CtorArgs::Positional(_)) => {
                let c = self.konst(Value::map((**k).clone(), (**v).clone(), BTreeMap::new()));
                self.emit(Op::Const(c));
            }
            (ValueType::Hash(k, v), CtorArgs::Positional(_)) => {
                let c = self.konst(Value::Hash(Box::new(crate::value::HashVal::new(
                    (**k).clone(),
                    (**v).clone(),
                ))));
                self.emit(Op::Const(c));
            }
            // ラップを剥がす
            (_, CtorArgs::Positional(a)) if a.len() == 1 => {
                self.expr(&a[0])?;
                self.emit(Op::NeedValue(span));
                let ti = self.type_idx(ty);
                self.emit(Op::Coerce(ti));
            }
            _ => return self.err("この型は構築できない", span),
        }
        Ok(())
    }

    fn if_expr(
        &mut self,
        i: &If,
        want: Option<ValueType>,
        span: Span,
    ) -> Result<(), CompileError> {
        let mut ends = Vec::new();
        for (c, b) in &i.arms {
            // **条件は `u1`。** 外の型は届かない
            self.expr(c)?;
            self.emit(Op::NeedU1(c.span));
            let j = self.emit(Op::JumpIfFalse(0));
            // **分岐は領域である**（C-20：被演算子位置なので領域だが、
            // スコープでも脱出段でもない）。したがって
            // **中身が空になれば外界面は paradox**（C-14 規則2）。
            //
            // これを書かないと、`if (c) x; fi` で条件が真のとき
            // **分岐が何も積まない**——偽のときは paradox を積むのに。
            // 合流点で高さが揃わず、後の `;` が下の値を食う（S-16）
            self.emit(Op::RegionBegin);
            self.want = want.clone();
            self.expr(b)?;
            self.emit(Op::EndRegion(b.span));
            ends.push(self.emit(Op::Jump(0)));
            self.patch(j);
        }
        match &i.els {
            Some(b) => {
                self.emit(Op::RegionBegin);
                self.want = want;
                self.expr(b)?;
                self.emit(Op::EndRegion(b.span));
            }
            // `else` の無い `if` で条件が偽 → paradox
            None => {
                self.emit(Op::Paradox(span));
            }
        }
        for j in ends {
            self.patch(j);
        }
        Ok(())
    }

    /// ループ。**本体はその構文の段**であって、二重にはならない（C-64）。
    fn loop_expr(
        &mut self,
        cond: Option<&Expr>,
        body: &Expr,
        span: Span,
    ) -> Result<(), CompileError> {
        let ExprKind::Block(items) = &body.kind else {
            return self.err("ループの本体はブロックでなければならない", body.span);
        };
        // **本体はその構文の段**（C-64）。`while` は条件の前から段に入る
        if cond.is_none() {
            self.emit(Op::LoopBegin);
        }
        let start = self.here();
        let mut exit = Vec::new();
        if let Some(c) = cond {
            self.expr(c)?;
            self.emit(Op::NeedValue(c.span));
            let z = self.konst(Value::I64(0));
            self.emit(Op::Const(z));
            self.emit(Op::Bin(BinOp::Ne, span));
            exit.push(self.emit(Op::JumpIfFalse(0)));
        }
        self.scopes.push(HashMap::new());
        self.collect(items);
        self.region(items, body.span)?;
        self.scopes.pop();
        self.emit(Op::LoopBody(start, span));
        for j in exit {
            self.patch(j);
        }
        self.emit(Op::LoopEnd(span));
        Ok(())
    }

    fn while_expr(&mut self, cond: &Expr, body: &Expr, span: Span) -> Result<(), CompileError> {
        self.emit(Op::LoopBegin);
        self.loop_expr(Some(cond), body, span)
    }

    fn nfor(
        &mut self,
        name: &str,
        start: &Expr,
        count: &Expr,
        body: &Expr,
        span: Span,
    ) -> Result<(), CompileError> {
        let ExprKind::Block(items) = &body.kind else {
            return self.err("ループの本体はブロックでなければならない", body.span);
        };
        self.expr(start)?;
        self.emit(Op::NeedValue(start.span));
        self.expr(count)?;
        self.emit(Op::NeedValue(count.span));
        self.scopes.push(HashMap::new());
        let s = self.slot(name);
        self.emit(Op::NForBegin(s, span));
        let top = self.here();
        self.collect(items);
        self.region(items, body.span)?;
        self.scopes.pop();
        self.emit(Op::NForNext(top, span));
        Ok(())
    }

    fn switch(&mut self, subject: &Expr, arms: &[Arm], span: Span) -> Result<(), CompileError> {
        self.expr(subject)?;
        self.emit(Op::NeedValue(subject.span));
        let mut ends = Vec::new();
        for a in arms {
            self.emit(Op::Dup);
            self.expr(&a.pattern)?;
            self.emit(Op::NeedValue(a.pattern.span));
            self.emit(Op::Bin(BinOp::Eq, a.span));
            let j = self.emit(Op::JumpIfFalse(0));
            self.emit(Op::Pop); // 被照合体を捨てる
            // **腕も領域である**（C-82）。`if` の分岐と同じ理由で正規化する
            self.emit(Op::RegionBegin);
            self.expr(&a.value)?;
            self.emit(Op::EndRegion(a.span));
            ends.push(self.emit(Op::Jump(0)));
            self.patch(j);
        }
        self.emit(Op::Pop);
        // どの腕にも当たらない → paradox
        self.emit(Op::Paradox(span));
        for j in ends {
            self.patch(j);
        }
        Ok(())
    }
}

impl Compiler {
    /// 利用者定義のメンバ関数が `var self` を取るか（S-1）。
    /// **取るなら破壊的なので、呼び出しの後にレシーバを書き戻す。**
    fn user_destructive(&self, name: &str) -> bool {
        self.var_self.contains(name)
    }
}

/// 第一引数が `var self` か（S-1）。**破壊するなら書き戻す。**
fn is_var_self(p: &Program2, idx: u32) -> bool {
    p.chunks.get(idx as usize).map(|c| c.self_is_var).unwrap_or(false)
}

fn is_destructive(name: &str) -> bool {
    matches!(name, "push" | "pop" | "clear" | "insert" | "remove")
}

impl Compiler {
    /// 脱出。**段数は書いた順に数える**（C-70）。`outward` は位置で持つ。
    fn escape(&mut self, esc: &Escape, span: Span) -> Result<(), CompileError> {
        let shape = self.shape(esc)?;
        match shape {
            Shape::Static { stages, outward, kind, payload, deferred } => {
                if let Some(p) = &payload {
                    self.expr(p)?;
                    self.emit(Op::NeedValue(p.span));
                }
                match kind {
                    EKind::Break => {
                        self.emit(Op::Break {
                            stages,
                            outward,
                            payload: payload.is_some(),
                            span,
                        });
                    }
                    EKind::Continue => {
                        // 遅延した被演算子は**再開した本体の先頭**で走る（C-73）
                        let d = match deferred {
                            Some(inner) => {
                                let j = self.emit(Op::Jump(0));
                                let at = self.here();
                                self.escape(&inner, span)?;
                                self.patch(j);
                                Some(at)
                            }
                            None => None,
                        };
                        self.emit(Op::Continue { deferred: d, span });
                    }
                }
            }
            Shape::Dynamic { count, payload } => {
                if let Some(p) = &payload {
                    self.expr(p)?;
                    self.emit(Op::NeedValue(p.span));
                }
                self.expr(&count)?;
                self.emit(Op::NeedValue(count.span));
                self.emit(Op::BreakDyn { payload: payload.is_some(), span });
            }
        }
        Ok(())
    }

    fn shape(&mut self, esc: &Escape) -> Result<Shape, CompileError> {
        Ok(match &esc.kind {
            EscapeKind::Break { outward } => {
                let base = if *outward { 1u64 } else { 0 };
                match &esc.operand {
                    Some(Operand::Escape(inner)) => match self.shape(inner)? {
                        Shape::Static { stages, outward: m, kind, payload, deferred } => {
                            Shape::Static {
                                stages: stages + 1,
                                outward: (m << 1) | base,
                                kind,
                                payload,
                                deferred,
                            }
                        }
                        d => d,
                    },
                    Some(Operand::Value(v)) => Shape::Static {
                        stages: 1,
                        outward: base,
                        kind: EKind::Break,
                        payload: Some(v.clone()),
                        deferred: None,
                    },
                    None => Shape::Static {
                        stages: 1,
                        outward: base,
                        kind: EKind::Break,
                        payload: None,
                        deferred: None,
                    },
                }
            }
            // `continue X` は段数を足さない（C-92）
            EscapeKind::Continue => Shape::Static {
                stages: 1,
                outward: 0,
                kind: EKind::Continue,
                payload: None,
                deferred: match &esc.operand {
                    Some(Operand::Escape(x)) => Some((**x).clone()),
                    _ => None,
                },
            },
            EscapeKind::Flow { name, args } => {
                if name == "$repeat" {
                    let [FlowArg::Escape(_), FlowArg::Value(n)] = &args[..] else {
                        return self.err("`$repeat` は作用素と回数を取る", esc.span);
                    };
                    let payload = match &esc.operand {
                        Some(Operand::Value(v)) => Some(v.clone()),
                        _ => None,
                    };
                    return Ok(Shape::Dynamic { count: n.clone(), payload });
                }
                let Some(d) = self.flows.get(name).cloned() else {
                    return self.err(format!("知らない作用素式 `{name}`"), esc.span);
                };
                if !self.expanding_flows.insert(name.clone()) {
                    return self.err("`flow` の本体に `flow` 名は書けない", esc.span);
                }
                // **本体は使用位置で読み直される**（C-15）
                let result = self.shape(&d.body);
                self.expanding_flows.remove(name);
                let mut sh = result?;
                if let (Shape::Dynamic { payload, .. }, Some(Operand::Value(v))) =
                    (&mut sh, &esc.operand)
                {
                    *payload = Some(v.clone());
                }
                if let (Shape::Static { payload, .. }, Some(Operand::Value(v))) =
                    (&mut sh, &esc.operand)
                {
                    *payload = Some(v.clone());
                }
                sh
            }
        })
    }
}

enum Shape {
    Static {
        stages: u32,
        outward: u64,
        kind: EKind,
        payload: Option<Expr>,
        deferred: Option<Escape>,
    },
    /// 段数が実行時に決まる（`$repeat`）。
    Dynamic { count: Expr, payload: Option<Expr> },
}

// ================= 仮想機械 =================

/// スタックに載るもの。**脱出は載らない**——制御であって値ではない。
#[derive(Clone, Debug)]
enum Slot {
    Value(Value),
    Paradox(Span),
    /// 値ではない。`alias` 引数の束縛にだけ使う（C-48）。
    Cell(CellId),
}

impl Slot {
    fn value(self, span: Span) -> Result<Value, RtErr> {
        match self {
            Slot::Value(v) => Ok(v),
            // **消費されなかった paradox**（C-46）
            Slot::Paradox(sp) => Err(RtErr { msg: "消費されなかった paradox".into(), span: sp }),
            Slot::Cell(_) => Err(RtErr { msg: "セル参照は値ではない".into(), span }),
        }
    }
    fn from_eval(e: crate::interp::Eval, span: Span) -> Result<Slot, RtErr> {
        match e {
            crate::interp::Eval::Value(v) => Ok(Slot::Value(v)),
            crate::interp::Eval::Paradox(sp) => Ok(Slot::Paradox(sp)),
            crate::interp::Eval::Akasha => Ok(Slot::Paradox(span)),
            crate::interp::Eval::Escape(_) => {
                Err(RtErr { msg: "ここで脱出は起きない".into(), span })
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct RtErr {
    pub msg: String,
    pub span: Span,
}

/// 制御の転送。**スタックには載せない。**
#[derive(Clone, Debug)]
struct Esc {
    kind: EKind,
    stages: u32,
    outward: u64,
    payload: Option<Value>,
    deferred: Option<u32>,
    span: Span,
}

/// 脱出段の種類。**`{ }` が段を作るのは、それ自体が式として置かれたときだけ**（C-64）。
/// `loop` / `while` / `nfor` / `fn` の本体は**その構文の段**であって、二重にはならない。
#[derive(Clone, Copy, PartialEq, Debug)]
enum StageKind {
    /// 裸のブロック。**`break` は抜けられるが、`continue` は再開できない**（C-72）。
    Block,
    /// `loop` / `while` の本体。
    Loop,
    /// `nfor` の本体。ループ変数を周回ごとに束縛し直す。
    NFor,
}

/// 実行中の脱出段。**内側が末尾。** 段送りはこの列を末尾から削っていく（C-70）。
struct StageState {
    kind: StageKind,
    /// 反復回数。**正常終了したループの値**になる（C-62）。
    count: i64,
    /// `nfor` の状態：開始値・済んだ回数・総回数・束縛する枠・型の見本。
    nfor: Option<(i128, i128, i128, u16, Value)>,
    /// `continue` が遅延させた被演算子の飛び先（C-73）。
    /// **再開した本体の先頭で走る**ので、次の周回の入口でここへ飛ぶ。
    pending: Option<u32>,
    /// この段に入ったときのスタックの高さ。抜けるときはここまで捨てる。
    base: usize,
    /// この段に入ったときの凍結の個数。
    /// `const` 別名の凍結は字句的に解ける（C-37）。
    frozen_base: usize,
}

struct Frame {
    chunk: u32,
    pc: usize,
    cells: Vec<CellId>,
    stack_base: usize,
    /// このフレームの中の脱出段。**フレーム自身は含まない。**
    stages: Vec<StageState>,
    /// 開いている領域の底。**段とは別に数える**（C-20：三つは別の単位）
    regions: Vec<usize>,
    /// `var self` を取るメンバ関数のとき、レシーバのセル。
    /// **レシーバは複製されない**（C-20）ので、返るときに書き戻す。
    self_cell: Option<CellId>,
    /// 呼び出し元が持っていた凍結の個数。フレームを返るとここまで戻す。
    frozen_base: usize,
}

pub struct Vm<'a> {
    p: &'a Program2,
    /// ホストが答える側（S-11）
    hosts: &'a mut dyn crate::value::HostFns,
    arena: Arena,
    stack: Vec<Slot>,
    frames: Vec<Frame>,
    /// 凍っているセル。同じセルの入れ子の凍結を数えるため列で持つ。
    frozen: Vec<CellId>,
    /// 枠のセル表の使い回し。`Runner` から借りる
    pool: Vec<Vec<CellId>>,
}

/// 実行の結果。木を辿る実装と同じ形（`interp::Eval` に合わせる）。
pub fn run_program(p: &Program2) -> Result<crate::interp::Eval, RtErr> {
    Ok(run_program_with_host(p, Vec::new())?.0)
}

/// 呼べる名前を持つホストで走らせる（S-11）。
pub fn run_program_with_fns(
    p: &Program2,
    host: Vec<Value>,
    hosts: &mut dyn crate::value::HostFns,
) -> Result<(crate::interp::Eval, Vec<Value>), RtErr> {
    Runner::new().run_with(p, host, hosts)
}

/// 実行時エラーでも、そこまでに変わったホストの値を返す。
///
/// C-2 により実行時エラーは状態を巻き戻さない。公開の低水準 API は互換性のため
/// `Result` のまま保ち、ホスト界面だけがこの形を使う。
pub(crate) fn run_program_with_fns_writeback(
    p: &Program2,
    host: Vec<Value>,
    hosts: &mut dyn crate::value::HostFns,
) -> (Result<crate::interp::Eval, RtErr>, Vec<Value>) {
    Runner::new().run_with_writeback(p, host, hosts)
}

/// ホストの値を渡して走らせ、**走り終わった値を返す**（S-4）。
///
/// 返る `Vec<Value>` は渡した順に対応する。**変わったかどうかはホストが見る。**
pub fn run_program_with_host(
    p: &Program2,
    host: Vec<Value>,
) -> Result<(crate::interp::Eval, Vec<Value>), RtErr> {
    Runner::new().run(p, host)
}

/// **一度きりでない実行のための入れ物。**
///
/// ホストが同じ組み立てを何千回も走らせるとき、走るたびに
/// 場（`Arena`）・積み（`stack`）・枠（`frames`）を作り直すのは無駄である——
/// **容量は前回と同じだけ要る。**
///
/// 中身は毎回捨てる。**捨てるのは値であって、場所ではない。**
/// 字句アリーナが領域の出口で一括解放するのと同じ原理を、実行そのものに掛ける。
#[derive(Default)]
pub struct Runner {
    arena: Arena,
    stack: Vec<Slot>,
    frames: Vec<Frame>,
    /// 凍結表の容量も実行ごとに使い回す。
    frozen: Vec<CellId>,
    /// 枠のセル表を使い回す。関数呼び出しのたびに確保しない
    pool: Vec<Vec<CellId>>,
}

impl Runner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn run(
        &mut self,
        p: &Program2,
        host: Vec<Value>,
    ) -> Result<(crate::interp::Eval, Vec<Value>), RtErr> {
        let mut none = crate::value::NoHostFns;
        self.run_with(p, host, &mut none)
    }

    /// **誤りのときは、そこまでの書き換えが落ちる。**
    ///
    /// C-2 は「実行時の誤りより前に行われたホストの書き換えを巻き戻さない」と決めている。
    /// つまり**誤りのときの後の状態にも意味がある**——
    /// 拾いたい埋め込み側は [`run_writeback`](Runner::run_writeback) を使うこと。
    pub fn run_with(
        &mut self,
        p: &Program2,
        host: Vec<Value>,
        hosts: &mut dyn crate::value::HostFns,
    ) -> Result<(crate::interp::Eval, Vec<Value>), RtErr> {
        let (result, after) = self.run_with_writeback(p, host, hosts);
        result.map(|eval| (eval, after))
    }

    /// **誤りでも後の状態を返す**（C-2）。
    ///
    /// `run` は誤りのときに書き換えを落とす。ホストが
    /// 「途中まで変わった」を回収するにはこちらを使う。
    pub fn run_writeback(
        &mut self,
        p: &Program2,
        host: Vec<Value>,
    ) -> (Result<crate::interp::Eval, RtErr>, Vec<Value>) {
        let mut none = crate::value::NoHostFns;
        self.run_with_writeback(p, host, &mut none)
    }

    /// 呼べる名前つき（S-11）で、**誤りでも後の状態を返す**（C-2）。
    pub fn run_with_writeback(
        &mut self,
        p: &Program2,
        host: Vec<Value>,
        hosts: &mut dyn crate::value::HostFns,
    ) -> (Result<crate::interp::Eval, RtErr>, Vec<Value>) {
        let mut vm = Vm {
            p,
            hosts,
            arena: std::mem::take(&mut self.arena),
            stack: std::mem::take(&mut self.stack),
            frames: std::mem::take(&mut self.frames),
            frozen: std::mem::take(&mut self.frozen),
            pool: std::mem::take(&mut self.pool),
        };
        let r = vm.run_top(p, host);
        // **返ってくる道は一つ。** 誤りで抜けても入れ物は戻す
        vm.arena.clear();
        vm.stack.clear();
        vm.frozen.clear();
        for f in vm.frames.drain(..) {
            vm.pool.push(f.cells);
        }
        self.arena = vm.arena;
        self.stack = vm.stack;
        self.frames = vm.frames;
        self.frozen = vm.frozen;
        self.pool = vm.pool;
        r
    }
}

impl Vm<'_> {
    fn run_top(
        &mut self,
        p: &Program2,
        host: Vec<Value>,
    ) -> (Result<crate::interp::Eval, RtErr>, Vec<Value>) {
        // ホストの値をセルに置き、最上位の枠へ結び付ける
        let base = self.arena.mark();
        let n = host.len();
        for v in host {
            self.arena.alloc(Some(v));
        }
        self.push_frame(p.top, Vec::new(), 1);
        // **写さない。** `host_slots` は組み立ての結果であり、走るたびに複製する理由が無い
        let slots = &p.chunks[p.top as usize].host_slots;
        let f = self.frames.last_mut().unwrap();
        for (i, s) in slots.iter().enumerate() {
            if i < n {
                f.cells[*s as usize] = CellId((base + i) as u32);
            }
        }
        let out = self.run();
        // 走り終わってから**取り出す**。写さない——セルはもう要らない
        let after: Vec<Value> = (0..n)
            .map(|i| self.arena.take(CellId((base + i) as u32)).unwrap_or(Value::I64(0)))
            .collect();
        let ev = match out {
            Ok(Slot::Value(v)) => Ok(crate::interp::Eval::Value(v)),
            Ok(Slot::Paradox(sp)) => Ok(crate::interp::Eval::Paradox(sp)),
            Ok(Slot::Cell(_)) => Err(RtErr {
                msg: "セル参照が呼び出しの外へ出た".into(),
                span: Span::NONE,
            }),
            Err(e) => Err(e),
        };
        (ev, after)
    }
}

impl Program2 {
    /// **ホストの名前を実際に使っているか**を、組んだ命令列から見る。
    ///
    /// 使っていない名前に値を作って渡すのは無駄である——
    /// ホストは**これを見て、要るものだけ用意すればよい。**
    ///
    /// 別名で受け直しても（`var c &= count;`）、`Alias` の元として現れるので数える。
    /// **どの添字を見ているか。**
    ///
    /// 「別名として見えている」ことと「実際に見ている」ことは別である。
    /// `count[5] * 2` は `count` が見えているが、**見ているのは 5 番だけ**——
    /// ホストは 256 個を用意する必要が無い。
    ///
    /// - `None` — **全部要る。** 動く添字、長さ以外の丸ごとの用途、別名で受け直し
    /// - `Some(v)` — **その添字だけ要る。** 定数の添字しか使っていない
    ///
    /// 長さ（`LoadLen`）は**値を要求しない**ので、ここには入らない。
    pub fn host_touched(&self, i: usize) -> Option<Vec<i128>> {
        let top = &self.chunks[self.top as usize];
        let slot = *top.host_slots.get(i)?;
        let mut idx = Vec::new();
        for c in &self.chunks {
            for (k, op) in c.ops.iter().enumerate() {
                match op {
                    // 長さだけなら値は要らない
                    Op::LoadLen(x, _) if *x == slot => {}
                    // 定数の添字なら、その一個だけ
                    Op::LoadIndex(x, _) | Op::StoreIndex(x, _) if *x == slot => {
                        match const_index_before(c, k) {
                            Some(n) => idx.push(n),
                            // 動く添字。**全部要る**
                            None => return None,
                        }
                    }
                    // 丸ごと読む・書く・別名にする → 全部要る
                    Op::Load(x) | Op::Store(x) | Op::StoreExact(x) | Op::Declare(x)
                        if *x == slot => return None,
                    Op::LoadField(x, _, _) if *x == slot => return None,
                    Op::Alias(a, b) if *a == slot || *b == slot => return None,
                    _ => {}
                }
            }
        }
        idx.sort_unstable();
        idx.dedup();
        Some(idx)
    }

    pub fn host_used(&self) -> Vec<bool> {
        let r = self.host_reads();
        let w = self.host_writes();
        r.iter().zip(w).map(|(a, b)| *a || b).collect()
    }

    /// この束縛を**読む必要があるか**。
    ///
    /// 使わない名前を読まなくてよくなる——rtex の `\count` のように
    /// **束縛が何百もあるホスト**では、これが起動費そのものである。
    ///
    /// **迷ったら読む側に倒す。** 読み過ぎは遅いだけだが、読み落としは間違いである。
    pub fn host_reads(&self) -> Vec<bool> {
        self.host_slot_flags(|op, s| match op {
            // 値そのものが要る
            Op::Load(x) | Op::LoadIndex(x, _) | Op::LoadLen(x, _) => *x == s,
            Op::LoadField(x, _, _) => *x == s,
            // **枡へ書くには、まず集合体が要る**
            Op::StoreIndex(x, _) => *x == s,
            // 破壊的メソッドも、いまの中身の上で働く
            Op::MutMethod(x, _, _, _) => *x == s,
            // **別名は読みにも書きにもなりうる**（C-53）。倒す先は読む側
            Op::Alias(a, b) => *a == s || *b == s,
            Op::Ref(x) | Op::Freeze(x) | Op::Declare(x) => *x == s,
            _ => false,
        })
    }

    /// この束縛を**書き戻す必要があるか**。
    ///
    /// 契約は「同じなら書かない」だが、**そもそも触れていないなら比べる必要も無い。**
    pub fn host_writes(&self) -> Vec<bool> {
        self.host_slot_flags(|op, s| match op {
            Op::Store(x) | Op::StoreExact(x) | Op::StoreIndex(x, _) => *x == s,
            Op::MutMethod(x, _, _, _) => *x == s,
            // 別名で受け直した先から書かれうる
            Op::Alias(a, b) => *a == s || *b == s,
            Op::Ref(x) | Op::Declare(x) => *x == s,
            _ => false,
        })
    }

    fn host_slot_flags(&self, f: impl Fn(&Op, u16) -> bool) -> Vec<bool> {
        let top = &self.chunks[self.top as usize];
        let mut used = vec![false; top.host_slots.len()];
        for (i, slot) in top.host_slots.iter().enumerate() {
            let s = *slot;
            used[i] = self.chunks.iter().any(|c| c.ops.iter().any(|op| f(op, s)));
        }
        used
    }
}

impl<'a> Vm<'a> {
    fn push_frame(&mut self, chunk: u32, args: Vec<(CellId, bool)>, depth: u32) {
        let ch = &self.p.chunks[chunk as usize];
        let mut cells = self.pool.pop().unwrap_or_default();
        cells.clear();
        cells.resize(ch.nslots as usize, CellId(u32::MAX));
        for (i, (cell, _)) in args.iter().enumerate() {
            if let Some((slot, _)) = ch.params.get(i) {
                cells[*slot as usize] = *cell;
            }
        }
        let _ = depth;
        self.frames.push(Frame {
            chunk,
            pc: 0,
            cells,
            stack_base: self.stack.len(),
            stages: Vec::new(),
            regions: Vec::new(),
            self_cell: None,
            frozen_base: self.frozen.len(),
        });
    }

    /// フレームからの段数（C-23）。`getdepth()` が返す。
    ///
    /// **フレーム自身が一段目**なので、中の段の数に 1 を足す。
    fn depth(&self) -> i64 {
        self.frames.last().map(|f| f.stages.len() as i64 + 1).unwrap_or(1)
    }

    fn err<T>(&self, msg: impl Into<String>, span: Span) -> Result<T, RtErr> {
        Err(RtErr { msg: msg.into(), span })
    }

    fn pop(&mut self) -> Slot {
        self.stack.pop().unwrap_or(Slot::Paradox(Span::NONE))
    }

    fn cell(&mut self, slot: u16) -> CellId {
        let f = self.frames.last_mut().unwrap();
        let c = f.cells[slot as usize];
        if c.0 == u32::MAX {
            let n = self.arena.alloc(None);
            self.frames.last_mut().unwrap().cells[slot as usize] = n;
            n
        } else {
            c
        }
    }
}

impl<'a> Vm<'a> {
    fn run(&mut self) -> Result<Slot, RtErr> {
        // **組み立ての結果は動かない。** 借りを一度取れば、毎回引き直さずに済む
        let p = self.p;
        let mut steps: u64 = 0;
        loop {
            steps += 1;
            if steps > 200_000_000 {
                return self.err("実行が長すぎる", Span::NONE);
            }
            // 最上位のフレームが返ったら終わり
            let Some(f) = self.frames.last_mut() else {
                return Ok(self.pop());
            };
            // **命令は写さない。** `Op` は平らな 24 バイトであり、
            // 借りて読めば済む——一命令ごとに写す理由が無い
            let Some(&op) = p.chunks[f.chunk as usize].ops.get(f.pc) else {
                return self.err("命令の終端を越えた", Span::NONE);
            };
            f.pc += 1;

            if let Some(esc) = self.step(op)? {
                // 脱出が起きた。段を送る
                if let Some(out) = self.unwind(esc)? {
                    return Ok(out);
                }
            }
        }
    }

    /// 一命令。脱出が起きたら返す。
    #[inline]
    fn step(&mut self, op: Op) -> Result<Option<Esc>, RtErr> {
        match op {
            Op::Const(k) => {
                let v = self.p.chunks[self.cur()].consts[k as usize].clone();
                self.stack.push(Slot::Value(v));
            }
            Op::Paradox(sp) => self.stack.push(Slot::Paradox(sp)),
            Op::Dup => {
                let t = self.stack.last().cloned().unwrap_or(Slot::Paradox(Span::NONE));
                self.stack.push(t);
            }
            Op::Pop => {
                self.pop();
            }
            Op::Load(s) => {
                let c = self.cell(s);
                match self.arena.get(c) {
                    Some(v) => self.stack.push(Slot::Value(v.clone())),
                    None => return self.err("まだ束縛されていない", Span::NONE),
                }
            }
            Op::Ref(s) => {
                let c = self.cell(s);
                self.stack.push(Slot::Cell(c));
            }
            Op::Store(s) => {
                let v = self.pop().value(Span::NONE)?;
                let c = self.cell(s);
                if self.frozen.contains(&c) {
                    return self.err("凍っているセルには書けない（`const` の別名がある）", Span::NONE);
                }
                let target = self
                    .arena
                    .get(c)
                    .map(Value::type_of)
                    .unwrap_or_else(|| v.type_of());
                let Some(v) = crate::interp::try_coerce_to(v, &target) else {
                    return self.err("浮動小数を狭めると非有限になる", Span::NONE);
                };
                self.arena.set(c, v);
            }
            Op::StoreExact(s) => {
                let v = self.pop().value(Span::NONE)?;
                let c = self.cell(s);
                self.arena.set(c, v);
            }
            Op::Declare(s) => {
                let v = self.pop().value(Span::NONE)?;
                let c = self.arena.alloc(Some(v));
                self.frames.last_mut().unwrap().cells[s as usize] = c;
            }
            // **名前が別のセルを指すようにする。値は動かさない**（C-20）
            Op::Alias(dst, src) => {
                let c = self.cell(src);
                self.frames.last_mut().unwrap().cells[dst as usize] = c;
            }
            Op::Freeze(src) => {
                let c = self.cell(src);
                self.frozen.push(c);
            }
            // **ホストが答える**（S-11）。
            // 返り値が無ければ領域に値を置かない——paradox になる
            Op::HostCall(hi, argc, sp) => {
                let mut args = Vec::with_capacity(argc as usize);
                for _ in 0..argc {
                    args.push(self.pop().value(sp)?);
                }
                args.reverse();
                match self.hosts.call(hi, &args) {
                    Some(v) => self.stack.push(Slot::Value(v)),
                    None => self.stack.push(Slot::Paradox(sp)),
                }
            }
            Op::Coerce(t) => {
                let ty = self.p.chunks[self.cur()].types[t as usize].clone();
                match self.pop() {
                    Slot::Value(v) => match try_coerce_pub(v, Some(&ty)) {
                        Some(v) => self.stack.push(Slot::Value(v)),
                        None => self.stack.push(Slot::Paradox(ty.span)),
                    },
                    Slot::Paradox(sp) => self.stack.push(Slot::Paradox(sp)),
                    Slot::Cell(_) => return self.err("セル参照は値ではない", ty.span),
                }
            }
            Op::NeedValue(sp) => {
                let t = self.pop();
                self.stack.push(Slot::Value(t.value(sp)?));
            }
            Op::NeedU1(sp) => {
                let v = self.pop().value(sp)?;
                match v {
                    Value::U1(_) => self.stack.push(Slot::Value(v)),
                    _ => return self.err("`if` の条件は `u1` でなければならない", sp),
                }
            }
            Op::Jump(t) => self.frames.last_mut().unwrap().pc = t as usize,
            Op::JumpIfFalse(t) => {
                let v = self.pop().value(Span::NONE)?;
                if !v.is_nonzero() {
                    self.frames.last_mut().unwrap().pc = t as usize;
                }
            }
            Op::JumpIfParadox(t) => {
                if matches!(self.stack.last(), Some(Slot::Paradox(_))) {
                    self.frames.last_mut().unwrap().pc = t as usize;
                }
            }
            // `??`：左が値ならそれで決まり、paradox なら右へ
            Op::JumpIfValue(t) => {
                if matches!(self.stack.last(), Some(Slot::Value(_))) {
                    self.frames.last_mut().unwrap().pc = t as usize;
                }
            }
            Op::Un(o, sp) => {
                let v = self.pop().value(sp)?;
                let r = crate::interp::unary_pub(o, v, sp)
                    .map_err(|e| RtErr { msg: e.msg, span: e.span })?;
                self.stack.push(Slot::Value(r));
            }
            Op::Bin(o, sp) => {
                let b = self.pop().value(sp)?;
                let a = self.pop().value(sp)?;
                let r = arith_pub(o, a, b, sp).map_err(|e| RtErr { msg: e.msg, span: e.span })?;
                self.stack.push(Slot::from_eval(r, sp)?);
            }
            Op::MakeArray(n) => {
                if n == u16::MAX {
                    // 回数と値から作る
                    let fill = self.pop().value(Span::NONE)?;
                    let cnt = self.pop().value(Span::NONE)?.as_int().unwrap_or(0).max(0);
                    self.stack.push(Slot::Value(Value::array(fill.type_of(), vec![fill; cnt as usize])));
                } else {
                    let mut items = Vec::with_capacity(n as usize);
                    for _ in 0..n {
                        items.push(self.pop().value(Span::NONE)?);
                    }
                    items.reverse();
                    let elem = items.first().map(|v| v.type_of()).unwrap_or(ValueType::I64);
                    self.stack.push(Slot::Value(Value::array(elem, items)));
                }
            }
            Op::MakeU8ArrayOne => {
                let source = self.pop().value(Span::NONE)?;
                let array = crate::interp::make_u8_array_one(source).ok_or_else(|| RtErr {
                    msg: "`u8 array` は `str` または `i64` の長さから作る".into(),
                    span: Span::NONE,
                })?;
                self.stack.push(Slot::Value(array));
            }
            Op::MakeMap(n) => {
                let mut entries = BTreeMap::new();
                let mut kt = ValueType::I64;
                let mut vt = ValueType::I64;
                let mut pairs = Vec::new();
                for _ in 0..n {
                    let v = self.pop().value(Span::NONE)?;
                    let k = self.pop().value(Span::NONE)?;
                    pairs.push((k, v));
                }
                for (k, v) in pairs.into_iter().rev() {
                    kt = k.type_of();
                    vt = v.type_of();
                    let Some(key) = k.as_key() else {
                        return self.err("写像の鍵にできない値", Span::NONE);
                    };
                    entries.insert(key, v);
                }
                self.stack.push(Slot::Value(Value::map(kt, vt, entries)));
            }
            Op::MakeStruct(ni, n) => {
                let name = self.p.chunks[self.cur()].names[ni as usize].clone();
                let mut vals = Vec::with_capacity(n as usize);
                for _ in 0..n {
                    vals.push(self.pop().value(Span::NONE)?);
                }
                vals.reverse();
                let fields: Vec<(String, Value)> = match self.p.structs.get(&name) {
                    Some(s) => s.fields.iter().map(|f| f.name.clone()).zip(vals).collect(),
                    // ラップ型（S-2）：欄の名前は空
                    None => vec![(String::new(), vals.into_iter().next().unwrap())],
                };
                self.stack.push(Slot::Value(Value::strukt(name, fields)));
            }
            Op::Field(ni, sp) => {
                let name = self.p.chunks[self.cur()].names[ni as usize].clone();
                let b = self.pop().value(sp)?;
                match crate::interp::get_field(&b, &name) {
                    Some(v) => self.stack.push(Slot::Value(v)),
                    None => return self.err(format!("欄 `{name}` が無い"), sp),
                }
            }
            Op::StoreIndex(slot, sp) => {
                let i = self.pop().value(sp)?;
                let v = self.pop().value(sp)?;
                let c = self.cell(slot);
                if self.frozen.contains(&c) {
                    return self.err("凍っているセルには書けない（`const` の別名がある）", sp);
                }
                let Some(coll) = self.arena.get_mut(c) else {
                    return self.err("まだ束縛されていない", sp);
                };
                if !crate::interp::set_index(coll, &i, v) {
                    return self.err("経路がたどれない", sp);
                }
            }
            Op::LoadIndex(slot, sp) => {
                let i = self.pop().value(sp)?;
                let c = self.cell(slot);
                let Some(coll) = self.arena.get(c) else {
                    return self.err("まだ束縛されていない", sp);
                };
                match crate::interp::get_index(coll, &i) {
                    Some(v) => self.stack.push(Slot::Value(v)),
                    None => self.stack.push(Slot::Paradox(sp)),
                }
            }
            Op::LoadField(slot, ni, sp) => {
                let name = self.p.chunks[self.cur()].names[ni as usize].clone();
                let c = self.cell(slot);
                let Some(v) = self.arena.get(c) else {
                    return self.err("まだ束縛されていない", sp);
                };
                match crate::interp::get_field(v, &name) {
                    Some(v) => self.stack.push(Slot::Value(v)),
                    None => return self.err(format!("欄 `{name}` が無い"), sp),
                }
            }
            Op::LoadLen(slot, sp) => {
                let c = self.cell(slot);
                let Some(v) = self.arena.get(c) else {
                    return self.err("まだ束縛されていない", sp);
                };
                let n = match v {
                    Value::Array(ar) => ar.items.len(),
                    Value::Str(s) => s.len(),
                    Value::Map(mp) => mp.entries.len(),
                    Value::Hash(h) => h.len(),
                    _ => return self.err("`len` は集合体にしか使えない", sp),
                };
                self.stack.push(Slot::Value(Value::I64(n as i64)));
            }
            Op::Index(sp) => {
                let i = self.pop().value(sp)?;
                let b = self.pop().value(sp)?;
                match crate::interp::get_index(&b, &i) {
                    // 配列の範囲外・写像の欠損キーは paradox
                    Some(v) => self.stack.push(Slot::Value(v)),
                    None => self.stack.push(Slot::Paradox(sp)),
                }
            }
            Op::SetField(ni, sp) => {
                let name = self.p.chunks[self.cur()].names[ni as usize].clone();
                let mut base = self.pop().value(sp)?;
                let v = self.pop().value(sp)?;
                if !crate::interp::set_field(&mut base, &name, v) {
                    return self.err(format!("欄 `{name}` が無い"), sp);
                }
                self.stack.push(Slot::Value(base));
            }
            Op::SetIndex(sp) => {
                let i = self.pop().value(sp)?;
                let mut base = self.pop().value(sp)?;
                let v = self.pop().value(sp)?;
                if !crate::interp::set_index(&mut base, &i, v) {
                    return self.err("経路がたどれない", sp);
                }
                self.stack.push(Slot::Value(base));
            }
            Op::Depth => {
                let d = self.depth();
                self.stack.push(Slot::Value(Value::I64(d)));
            }
            Op::Method(ni, argc, sp) => {
                let name = self.p.chunks[self.cur()].names[ni as usize].clone();
                // 名前の引数は Compiler::call が [Cell, Value] の二つを積む。
                // Value は左から右に評価した時点の写し、Cell は alias 束縛用である。
                let mut args: Vec<(Value, Option<CellId>)> = Vec::with_capacity(argc as usize);
                for _ in 0..argc {
                    let value = self.pop().value(sp)?;
                    let cell = match self.stack.last() {
                        Some(Slot::Cell(_)) => match self.pop() {
                            Slot::Cell(cell) => Some(cell),
                            _ => unreachable!(),
                        },
                        _ => None,
                    };
                    args.push((value, cell));
                }
                args.reverse();
                let recv = self.pop().value(sp)?;
                // 利用者定義（S-1）を先に探す
                let key = match recv.type_of() {
                    ValueType::Named(t) => format!("{t}.{name}"),
                    _ => String::new(),
                };
                if let Some(idx) = self.p.fn_index.get(&key).copied() {
                    let params = self.p.chunks[idx as usize].params.clone();
                    if params.len() != args.len() + 1 {
                        return self.err(
                            format!("`{name}` は引数を {} 個取る", params.len().saturating_sub(1)),
                            sp,
                        );
                    }
                    let self_cell = self.arena.alloc(Some(recv));
                    let mut cells = vec![(self_cell, true)];
                    let mut aliases = Vec::new();
                    for (i, (value, cell)) in args.into_iter().enumerate() {
                        let is_alias = params[i + 1].1;
                        if is_alias {
                            let Some(cell) = cell else {
                                return self.err("`alias` 引数に渡せるのは名前だけ", sp);
                            };
                            if aliases.contains(&cell) {
                                return self.err("同じセルに別名が二つ届く", sp);
                            }
                            aliases.push(cell);
                            cells.push((cell, true));
                        } else {
                            cells.push((self.arena.alloc(Some(value)), false));
                        }
                    }
                    self.push_frame(idx, cells, 1);
                    // `var self` なら返るときに書き戻す（S-1）
                    if is_var_self(self.p, idx) {
                        self.frames.last_mut().unwrap().self_cell = Some(self_cell);
                    }
                    return Ok(None);
                }
                let args: Vec<Value> = args.into_iter().map(|(value, _)| value).collect();
                if is_destructive(&name) {
                    let mut cur = recv;
                    let out = write_method_pub(&mut cur, &name, &args, sp)
                        .map_err(|e| RtErr { msg: e.msg, span: e.span })?;
                    self.stack.push(Slot::Value(cur));
                    self.stack.push(Slot::from_eval(out, sp)?);
                    // 結果とレシーバを入れ替えて、書き戻しに備える
                    let n = self.stack.len();
                    self.stack.swap(n - 1, n - 2);
                } else {
                    let out = read_method_pub(&recv, &name, &args, sp)
                        .map_err(|e| RtErr { msg: e.msg, span: e.span })?;
                    self.stack.push(Slot::from_eval(out, sp)?);
                }
            }
            Op::MutMethod(slot, ni, argc, sp) => {
                let name = self.p.chunks[self.cur()].names[ni as usize].clone();
                let mut args = Vec::with_capacity(argc as usize);
                for _ in 0..argc {
                    args.push(self.pop().value(sp)?);
                }
                args.reverse();
                let cell = self.cell(slot);
                if self.frozen.contains(&cell) {
                    return self.err("凍っているセルには書けない", sp);
                }
                let Some(cur) = self.arena.get_mut(cell) else {
                    return self.err("まだ束縛されていない", sp);
                };
                let out = write_method_pub(cur, &name, &args, sp)
                    .map_err(|e| RtErr { msg: e.msg, span: e.span })?;
                self.stack.push(Slot::from_eval(out, sp)?);
            }
            Op::Call(idx, argc, sp) => {
                let ch = &self.p.chunks[idx as usize];
                let params = ch.params.clone();
                let mut args = Vec::with_capacity(argc as usize);
                for _ in 0..argc {
                    args.push(self.pop());
                }
                args.reverse();
                let mut cells = Vec::new();
                let mut aliases = Vec::new();
                for (i, a) in args.into_iter().enumerate() {
                    let is_alias = params.get(i).map(|p| p.1).unwrap_or(false);
                    if is_alias {
                        let cell = match a {
                            Slot::Cell(cell) => cell,
                            _ => {
                                return self.err("`alias` 引数に渡せるのは名前だけ", sp)
                            }
                        };
                        // 名前が違っても、実際のセルが同じなら二つの別名である（C-87）。
                        if aliases.contains(&cell) {
                            return self.err("同じセルに別名が二つ届く", sp);
                        }
                        aliases.push(cell);
                        cells.push((cell, true));
                    } else {
                        // 非 alias は深く複製する（値は自己完結しているので clone で足りる）
                        let value = a.value(sp)?;
                        cells.push((self.arena.alloc(Some(value)), false));
                    }
                }
                self.push_frame(idx, cells, 1);
            }
            Op::Ret => {
                let out = self.pop();
                let f = self.frames.pop().unwrap();
                self.stack.truncate(f.stack_base);
                self.frozen.truncate(f.frozen_base);
                self.stack.push(out);
                // `var self` なら、書き換えたレシーバを**結果の上**に置く（S-1）。
                // 呼び出し側は直後に `Store` で書き戻し、結果だけが残る
                if let Some(c) = f.self_cell {
                    if let Some(v) = self.arena.get(c).cloned() {
                        self.stack.push(Slot::Value(v));
                    }
                }
            }
            Op::RegionBegin => {
                let b = self.stack.len();
                self.frames.last_mut().unwrap().regions.push(b);
            }
            Op::EndRegion(sp) => {
                // **一つの領域は値を一つしか持てない。** 空なら外界面は paradox
                let base = self.region_base();
                self.frames.last_mut().unwrap().regions.pop();
                if self.stack.len() > base + 1 {
                    return self.err("一つの領域に値が二つある", sp);
                }
                if self.stack.len() == base {
                    self.stack.push(Slot::Paradox(sp));
                }
            }
            Op::BlockBegin => {
                let base = self.stack.len();
                self.frames.last_mut().unwrap().stages.push(StageState {
                    kind: StageKind::Block,
                    count: 0,
                    nfor: None,
                    pending: None,
                    base,
                    frozen_base: self.frozen.len(),
                });
            }
            // 裸のブロックが正常に終わった。段を畳むだけで、値はそのまま
            Op::BlockEnd(_) => {
                let f = self.frames.last_mut().unwrap();
                if let Some(st) = f.stages.pop() {
                    self.frozen.truncate(st.frozen_base);
                }
            }
            Op::LoopBegin => {
                let base = self.stack.len();
                self.frames.last_mut().unwrap().stages.push(StageState {
                    kind: StageKind::Loop,
                    count: 0,
                    nfor: None,
                    pending: None,
                    base,
                    frozen_base: self.frozen.len(),
                });
            }
            Op::LoopBody(start, sp) => return self.loop_body(start, sp),
            Op::LoopEnd(sp) => {
                let f = self.frames.last_mut().unwrap();
                let st = f.stages.pop().unwrap();
                self.stack.truncate(st.base);
                self.frozen.truncate(st.frozen_base);
                // 0 周なら paradox、そうでなければ反復回数
                if st.count == 0 {
                    self.stack.push(Slot::Paradox(sp));
                } else {
                    self.stack.push(Slot::Value(Value::I64(st.count)));
                }
            }
            Op::NForBegin(slot, sp) => return self.nfor_begin(slot, sp),
            Op::NForNext(top, sp) => return self.nfor_next(top, sp),
            Op::Break { stages, outward, payload, span } => {
                let p = if payload { Some(self.pop().value(span)?) } else { None };
                return Ok(Some(Esc {
                    kind: EKind::Break,
                    stages,
                    outward,
                    payload: p,
                    deferred: None,
                    span,
                }));
            }
            Op::BreakDyn { payload, span } => {
                let n = self.pop().value(span)?.as_int().unwrap_or(0);
                let p = if payload { Some(self.pop().value(span)?) } else { None };
                // `n` が 0 以下なら作用素を一つも重ねない（C-75 の 5）
                if n <= 0 {
                    self.stack.push(match p {
                        Some(v) => Slot::Value(v),
                        None => Slot::Paradox(span),
                    });
                    return Ok(None);
                }
                return Ok(Some(Esc {
                    kind: EKind::Break,
                    stages: n as u32,
                    outward: 0,
                    payload: p,
                    deferred: None,
                    span,
                }));
            }
            Op::Continue { deferred, span } => {
                return Ok(Some(Esc {
                    kind: EKind::Continue,
                    stages: 1,
                    outward: 0,
                    payload: None,
                    deferred,
                    span,
                }))
            }
        }
        Ok(None)
    }

    fn cur(&self) -> usize {
        self.frames.last().unwrap().chunk as usize
    }

    /// いまの領域が積み始めた位置。**一つの領域は値を一つしか持てない**ので、
    /// ここから数えて二つ以上あればエラーになる。
    fn region_base(&self) -> usize {
        let f = self.frames.last().unwrap();
        // **開いている領域があればその底。** 無ければ段の底
        if let Some(&b) = f.regions.last() {
            return b;
        }
        match f.stages.last() {
            Some(l) => l.base,
            None => f.stack_base,
        }
    }
}

impl<'a> Vm<'a> {
    /// ループ本体を一周し終えた。**脱出はここで受ける。**
    fn loop_body(&mut self, start: u32, sp: Span) -> Result<Option<Esc>, RtErr> {
        // 本体は値を残してはいけない（静的検査が保証する）。残りを捨てる
        let (base, frozen_base) = {
            let f = self.frames.last().unwrap();
            f.stages
                .last()
                .map(|l| (l.base, l.frozen_base))
                .unwrap_or((f.stack_base, f.frozen_base))
        };
        self.stack.truncate(base);
        self.frozen.truncate(frozen_base);
        let f = self.frames.last_mut().unwrap();
        let st = f.stages.last_mut().unwrap();
        st.count = st.count.wrapping_add(1);
        // 遅延した被演算子は**再開した本体の先頭**で走る（C-73）
        if let Some(pc) = st.pending.take() {
            f.pc = pc as usize;
        } else {
            f.pc = start as usize;
        }
        let _ = sp;
        Ok(None)
    }

    fn nfor_begin(&mut self, slot: u16, sp: Span) -> Result<Option<Esc>, RtErr> {
        let count = self.pop().value(sp)?;
        let start = self.pop().value(sp)?;
        let n = count.as_int().unwrap_or(0);
        let s0 = start.as_int().unwrap_or(0);
        let base = self.stack.len();
        self.frames.last_mut().unwrap().stages.push(StageState {
            kind: StageKind::NFor,
            count: 0,
            nfor: Some((s0, 0, n, slot, start.clone())),
            pending: None,
            base,
            frozen_base: self.frozen.len(),
        });
        // 回数が 0 以下なら 0 周 → paradox
        if n <= 0 {
            let f = self.frames.last_mut().unwrap();
            f.stages.pop();
            self.stack.push(Slot::Paradox(sp));
            // 本体を飛ばす。`NForNext` まで進める
            let top = f.pc;
            let ops = &self.p.chunks[f.chunk as usize].ops;
            let mut i = top;
            let mut depth = 0i32;
            while i < ops.len() {
                match &ops[i] {
                    Op::NForBegin(..) => depth += 1,
                    Op::NForNext(..) => {
                        if depth == 0 {
                            break;
                        }
                        depth -= 1;
                    }
                    _ => {}
                }
                i += 1;
            }
            self.frames.last_mut().unwrap().pc = i + 1;
            return Ok(None);
        }
        // 最初の周回の束縛
        let cell = self.arena.alloc(Some(crate::interp::wrap_like(&start, s0)));
        self.frames.last_mut().unwrap().cells[slot as usize] = cell;
        Ok(None)
    }

    fn nfor_next(&mut self, top: u32, sp: Span) -> Result<Option<Esc>, RtErr> {
        let (base, frozen_base) = {
            let f = self.frames.last().unwrap();
            f.stages
                .last()
                .map(|l| (l.base, l.frozen_base))
                .unwrap_or((f.stack_base, f.frozen_base))
        };
        self.stack.truncate(base);
        self.frozen.truncate(frozen_base);
        let (s0, k, n, slot, sample, pending) = {
            let f = self.frames.last_mut().unwrap();
            let st = f.stages.last_mut().unwrap();
            st.count = st.count.wrapping_add(1);
            let (s0, k, n, slot, sample) = st.nfor.clone().unwrap();
            let pending = st.pending.take();
            st.nfor = Some((s0, k + 1, n, slot, sample.clone()));
            (s0, k + 1, n, slot, sample, pending)
        };
        if k >= n {
            let f = self.frames.last_mut().unwrap();
            let st = f.stages.pop().unwrap();
            self.stack.truncate(st.base);
            self.frozen.truncate(st.frozen_base);
            self.stack.push(Slot::Value(Value::I64(st.count)));
            return Ok(None);
        }
        // 次の周回の束縛。**`i` は周回ごとに新しい**
        let cell = self.arena.alloc(Some(crate::interp::wrap_like(&sample, s0.wrapping_add(k))));
        let f = self.frames.last_mut().unwrap();
        f.cells[slot as usize] = cell;
        f.pc = match pending {
            Some(pc) => pc as usize,
            None => top as usize,
        };
        let _ = sp;
        Ok(None)
    }

    /// 段を送る。
    ///
    /// **段送りは書いた順（左から右）に一段ずつ起きる**（C-70）。
    /// 一段ごとに、いまいる段が何であるかを見て:
    ///
    /// - **通り抜ける段**（`stages > 1`）なら畳んで次へ。
    ///   フレームなら `outward` が要る。**その段送りに `outward` が掛かっているかは
    ///   ビット列の最下位で見る**（C-70）——「何回越えるか」ではなく「何回目が越えるか」
    /// - **行き先の段**（`stages == 1`）なら、`break` は**終結**させ、
    ///   `continue` は**再開**させる（C-72）。再開できるものが無ければエラー
    ///
    /// 返り値が `Some` なら、最上位まで抜けて実行が終わったということ。
    fn unwind(&mut self, mut esc: Esc) -> Result<Option<Slot>, RtErr> {
        loop {
            // **脱出は領域を閉じてから起きる**（C-43）が、飛び越された領域の底は残る。
            // 積みより上に残った底を捨てる——**領域は開いたまま消えることがある**
            {
                let n = self.stack.len();
                let f = self.frames.last_mut().unwrap();
                f.regions.retain(|b| *b <= n);
            }
            let in_stage = !self.frames.last().unwrap().stages.is_empty();

            if in_stage {
                let kind = self.frames.last().unwrap().stages.last().unwrap().kind;
                if esc.stages > 1 {
                    // 通り抜ける。**フレームでないのに `outward` が掛かっていれば空振り**
                    if esc.outward & 1 != 0 {
                        return self.err("`outward` がフレームを越えていない（空振り）", esc.span);
                    }
                    esc.stages -= 1;
                    esc.outward >>= 1;
                    self.leave_stage_to_end(kind)?;
                    continue;
                }
                return self.land(esc, kind);
            }

            // フレーム。**脱出はここで止まる。越えるには `outward` が要る**（C-34）
            if esc.stages > 1 {
                if esc.outward & 1 == 0 {
                    return self.err("フレームを越える脱出", esc.span);
                }
                esc.stages -= 1;
                esc.outward >>= 1;
            }
            let out = match esc.kind {
                // **上限まで抜けるのは許される**——関数から返る（C-66）
                EKind::Break => match esc.payload.take() {
                    Some(v) => Slot::Value(v),
                    None => Slot::Paradox(esc.span),
                },
                EKind::Continue => return self.err("再開できるものが無い", esc.span),
            };
            let f = self.frames.pop().unwrap();
            self.stack.truncate(f.stack_base);
            self.frozen.truncate(f.frozen_base);
            if self.frames.is_empty() {
                return Ok(Some(out));
            }
            self.stack.push(out);
            if let Some(c) = f.self_cell {
                if let Some(v) = self.arena.get(c).cloned() {
                    self.stack.push(Slot::Value(v));
                }
            }
            // `outward` で越えた先で、まだ段が残っていれば続ける
            if esc.stages > 1 {
                continue;
            }
            return Ok(None);
        }
    }

    /// 通り抜ける段を畳み、**その段の直後の命令へ位置を進める。**
    ///
    /// 畳むだけでは駄目で、命令の位置も合わせないと、
    /// 抜けたはずの段の終わり（`LoopEnd` など）を踏んでしまう。
    fn leave_stage_to_end(&mut self, kind: StageKind) -> Result<(), RtErr> {
        let f = self.frames.last_mut().unwrap();
        let st = f.stages.pop().unwrap();
        self.stack.truncate(st.base);
        self.frozen.truncate(st.frozen_base);
        let f = self.frames.last_mut().unwrap();
        let i = find_stage_end(&self.p.chunks[f.chunk as usize].ops, f.pc, kind);
        f.pc = i + 1;
        Ok(())
    }

    /// 行き先の段。**`break` は終結させ、`continue` は再開させる**（C-72）。
    fn land(&mut self, mut esc: Esc, kind: StageKind) -> Result<Option<Slot>, RtErr> {
        match esc.kind {
            EKind::Break => {
                let f = self.frames.last_mut().unwrap();
                let st = f.stages.pop().unwrap();
                let i = find_stage_end(&self.p.chunks[f.chunk as usize].ops, f.pc, kind);
                self.stack.truncate(st.base);
                self.frozen.truncate(st.frozen_base);
                // **値の無い脱出は何も置かない**ので、外界面は paradox（C-62）
                let v = match esc.payload.take() {
                    Some(v) => Slot::Value(v),
                    None => Slot::Paradox(esc.span),
                };
                self.stack.push(v);
                self.frames.last_mut().unwrap().pc = i + 1;
                Ok(None)
            }
            EKind::Continue => {
                // **裸のブロックは再開できない**——ループではないから（C-72）
                if kind == StageKind::Block {
                    return self.err("再開できるものが無い（抜けた先がループではない）", esc.span);
                }
                let f = self.frames.last_mut().unwrap();
                let st = f.stages.last_mut().unwrap();
                // 遅延した被演算子は**再開した本体の先頭**で走る（C-73）
                st.pending = esc.deferred;
                let base = st.base;
                let frozen_base = st.frozen_base;
                self.stack.truncate(base);
                self.frozen.truncate(frozen_base);
                let f = self.frames.last_mut().unwrap();
                let i = find_stage_end(&self.p.chunks[f.chunk as usize].ops, f.pc, kind);
                // 本体の末尾（`LoopBody` / `NForNext`）へ飛ぶ。そこで周回が進む
                f.pc = i;
                Ok(None)
            }
        }
    }
}

/// いまの位置から、その段の終わりの命令を探す。
///
/// 段は入れ子になるので、**同じ種類の始まりを数えて釣り合いを取る。**
fn find_stage_end(ops: &[Op], from: usize, kind: StageKind) -> usize {
    let mut depth = 0i32;
    let mut i = from;
    while i < ops.len() {
        match &ops[i] {
            Op::BlockBegin | Op::LoopBegin | Op::NForBegin(..) => depth += 1,
            Op::BlockEnd(_) | Op::LoopEnd(_) | Op::NForNext(..) | Op::LoopBody(..) => {
                let matches_kind = match (kind, &ops[i]) {
                    (StageKind::Block, Op::BlockEnd(_)) => true,
                    (StageKind::Loop, Op::LoopEnd(_)) => true,
                    (StageKind::NFor, Op::NForNext(..)) => true,
                    _ => false,
                };
                if depth == 0 && matches_kind {
                    return i;
                }
                // `LoopBody` は段の終わりではないので数えない
                if !matches!(&ops[i], Op::LoopBody(..)) && depth > 0 {
                    depth -= 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    ops.len().saturating_sub(1)
}

/// 走らせる。**木を辿る実装と同じ結果になるはず**——差分を取るのが目的。
pub fn run(src: &str) -> Result<crate::interp::Eval, String> {
    let prog = crate::parser::parse(src).map_err(|e| format!("構文: {}", e.msg))?;
    let p = compile(&prog).map_err(|e| e.msg)?;
    run_program(&p).map_err(|e| e.msg)
}

/// 添字が定数か。`Const(k); NeedValue; LoadIndex(..)` という並びを見る。
fn const_index_before(c: &Chunk, at: usize) -> Option<i128> {
    // 直前は NeedValue、その前が Const のはず
    let prev = at.checked_sub(1)?;
    if !matches!(c.ops[prev], Op::NeedValue(_)) {
        return None;
    }
    let prev2 = prev.checked_sub(1)?;
    let Op::Const(k) = c.ops[prev2] else {
        return None;
    };
    c.consts.get(k as usize)?.as_int()
}

/// 大きさを測る。**遅さの多くは、動かしている塊の大きさである。**
pub fn sizes() -> Vec<(&'static str, usize)> {
    use std::mem::size_of;
    vec![
        ("Value", size_of::<Value>()),
        ("Slot", size_of::<Slot>()),
        ("Op", size_of::<Op>()),
        ("Esc", size_of::<Esc>()),
        ("RtErr", size_of::<RtErr>()),
        ("Result<Option<Esc>,RtErr>", size_of::<Result<Option<Esc>, RtErr>>()),
        ("Eval", size_of::<crate::interp::Eval>()),
        ("ValueType", size_of::<ValueType>()),
    ]
}
