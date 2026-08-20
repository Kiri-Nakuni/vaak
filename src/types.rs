//! 型検査。**`check.rs` とは別の層である。**
//!
//! `check.rs` が見るのは領域の値の数・段数・権限・別名——**効果と能力**。
//! ここが見るのは**型だけ**。二つを混ぜると、どちらも読めなくなる。
//!
//! **暗黙の数値変換は無い**（C-21）。リテラルだけが、置かれた場所の型を受け取る。

use crate::ast::*;
use crate::check::StaticError;
use crate::span::Span;
use std::collections::HashMap;

pub fn check_types(prog: &Program) -> Vec<StaticError> {
    check_types_with_host(prog, &[])
}

/// ホストが見せている名前と型を添えて検査する（S-4）。
pub fn check_types_with_host(prog: &Program, host: &[(String, ValueType)]) -> Vec<StaticError> {
    let mut t = TypeChecker {
        errs: Vec::new(),
        scopes: vec![HashMap::new()],
        frame_base: 0,
        fns: HashMap::new(),
        structs: HashMap::new(),
        wraps: HashMap::new(),
        stage_want: vec![None],
    };
    for (n, ty) in host {
        t.scopes[0].insert(n.clone(), ty.clone());
    }
    t.collect(&prog.body);
    t.body(&prog.body);
    t.errs
}

struct TypeChecker {
    errs: Vec<StaticError>,
    scopes: Vec<HashMap<String, ValueType>>,
    frame_base: usize,
    fns: HashMap<String, FnDecl>,
    structs: HashMap<String, StructDecl>,
    wraps: HashMap<String, ValueType>,
    /// 段ごとの「そこに置かれる値の型」。**脱出が運ぶ値はこれに合わねばならない。**
    /// 内側が末尾。段送りは書いた順（左から右）に外へ進む（C-70）。
    stage_want: Vec<T>,
}

/// 分かっている型。`None` は「分からない」——**そこでは何も言わない**。
type T = Option<ValueType>;

impl TypeChecker {
    fn err(&mut self, msg: impl Into<String>, span: Span) {
        self.errs.push(StaticError { msg: msg.into(), span });
    }

    fn collect(&mut self, body: &[Expr]) {
        for e in body {
            let mut e = e;
            while let ExprKind::Discard(Some(i)) = &e.kind {
                e = i;
            }
            match &e.kind {
                ExprKind::FnDecl(f) => {
                    self.fns.insert(crate::ast::fn_key(f), f.clone());
                }
                ExprKind::StructDecl(s) => {
                    self.structs.insert(s.name.clone(), s.clone());
                }
                ExprKind::WrapDecl(w) => {
                    self.wraps.insert(w.name.clone(), w.base.value.clone());
                }
                _ => {}
            }
        }
    }

    fn lookup(&self, n: &str) -> T {
        for s in self.scopes[self.frame_base..].iter().rev() {
            if let Some(t) = s.get(n) {
                return Some(t.clone());
            }
        }
        None
    }

    fn body(&mut self, body: &[Expr]) {
        for e in body {
            self.expr(e, None);
        }
    }

    fn scoped(&mut self, f: impl FnOnce(&mut Self)) {
        self.scopes.push(HashMap::new());
        f(self);
        self.scopes.pop();
    }

    /// 型が合うか。**暗黙の変換は無いので、同じでなければならない。**
    /// ラップ型も**別の型である**（S-2）——包むには `new` が要る。
    fn unify(&mut self, want: &ValueType, got: &ValueType, span: Span) {
        if want != got {
            self.err(format!("型が合わない（`{}` が要るのに `{}`）", show(want), show(got)), span);
        }
    }

    fn expect(&mut self, e: &Expr, want: &ValueType) {
        if let Some(got) = self.expr(e, Some(want)) {
            self.unify(want, &got, e.span);
        }
    }

    // ---- 式 ----

    fn expr(&mut self, e: &Expr, want: Option<&ValueType>) -> T {
        match &e.kind {
            // リテラルは**型が決まるまでソースの表現を保持する**。
            // 置かれた場所の型を受け取り、無ければ既定（整数は i64、浮動小数は f64）
            ExprKind::Int(_) => Some(match want {
                Some(t) if is_int(t) => t.clone(),
                _ => ValueType::I64,
            }),
            ExprKind::Float(_) => Some(match want {
                Some(t) if is_float(t) => t.clone(),
                _ => ValueType::F64,
            }),
            ExprKind::Str(_) => Some(ValueType::Str),
            // **`u1` で確定。** 文脈を見ない——真偽は数ではない（C-97）
            ExprKind::Bool(_) => Some(ValueType::U1),

            // **`->` が求める型になる。** リテラルはここで型が決まる（C-30 / C-25）
            ExprKind::Ascribe { expr, ty } => {
                self.expr(expr, Some(&ty.value));
                Some(ty.value.clone())
            }

            ExprKind::Name(n) => self.lookup(n),

            ExprKind::Paren(b) => {
                let mut last = None;
                self.scopes.push(HashMap::new());
                self.collect(b);
                for x in b {
                    let t = self.expr(x, want);
                    if !matches!(x.kind, ExprKind::Discard(_)) {
                        last = t;
                    }
                }
                self.scopes.pop();
                last
            }

            // 裸のブロックは段。**脱出の行き先になる**
            ExprKind::Block(b) => {
                let mut last = None;
                self.scopes.push(HashMap::new());
                self.stage_want.push(want.cloned());
                self.collect(b);
                for x in b {
                    let t = self.expr(x, want);
                    if !matches!(x.kind, ExprKind::Discard(_)) {
                        last = t;
                    }
                }
                self.stage_want.pop();
                self.scopes.pop();
                last
            }

            ExprKind::Discard(inner) => {
                if let Some(x) = inner {
                    self.expr(x, None);
                }
                None
            }

            ExprKind::Unary { op, rhs } => {
                let t = self.expr(rhs, want);
                match op {
                    UnOp::Not => {
                        if let Some(t) = &t {
                            if !is_int(t) {
                                self.err("`!` は整数にしか使えない", e.span);
                            }
                        }
                    }
                    _ => {
                        if let Some(t) = &t {
                            if !is_num(t) {
                                self.err("符号は数値にしか使えない", e.span);
                            }
                        }
                    }
                }
                t
            }

            ExprKind::Binary { op, lhs, rhs } => self.binary(*op, lhs, rhs, want, e.span),

            ExprKind::Field { base, name } => {
                let bt = self.expr(base, None)?;
                let ValueType::Named(sn) = &bt else {
                    self.err(format!("`{}` に欄は無い", show(&bt)), e.span);
                    return None;
                };
                let s = self.structs.get(sn)?;
                match s.fields.iter().find(|f| &f.name == name) {
                    Some(f) => Some(f.ty.value.clone()),
                    None => {
                        self.err(format!("`{sn}` に欄 `{name}` は無い"), e.span);
                        None
                    }
                }
            }

            ExprKind::Index { base, index } => {
                let bt = self.expr(base, None)?;
                match &bt {
                    ValueType::Array(el) => {
                        self.expr(index, Some(&ValueType::I64));
                        Some((**el).clone())
                    }
                    ValueType::Str => {
                        self.expr(index, Some(&ValueType::I64));
                        Some(ValueType::U8)
                    }
                    ValueType::Map(k, v) => {
                        self.expect(index, k);
                        Some((**v).clone())
                    }
                    other => {
                        self.err(format!("`{}` に添字は使えない", show(other)), e.span);
                        None
                    }
                }
            }

            ExprKind::Call { callee, args } => self.call(callee, args, e.span),

            ExprKind::ArrayLit(items) => {
                let el = match want {
                    Some(ValueType::Array(el)) => Some((**el).clone()),
                    _ => None,
                };
                let mut ty = el.clone();
                for it in items {
                    let t = self.expr(it, ty.as_ref());
                    match (&ty, t) {
                        (None, Some(t)) => ty = Some(t),
                        (Some(w), Some(g)) => self.unify(&w.clone(), &g, it.span),
                        _ => {}
                    }
                }
                Some(ValueType::Array(Box::new(ty.unwrap_or(ValueType::I64))))
            }

            ExprKind::MapLit(pairs) => {
                let (mut kt, mut vt) = match want {
                    Some(ValueType::Map(k, v)) => (Some((**k).clone()), Some((**v).clone())),
                    _ => (None, None),
                };
                for (k, v) in pairs {
                    let a = self.expr(k, kt.as_ref());
                    match (&kt, a) {
                        (None, Some(t)) => kt = Some(t),
                        (Some(w), Some(g)) => self.unify(&w.clone(), &g, k.span),
                        _ => {}
                    }
                    let b = self.expr(v, vt.as_ref());
                    match (&vt, b) {
                        (None, Some(t)) => vt = Some(t),
                        (Some(w), Some(g)) => self.unify(&w.clone(), &g, v.span),
                        _ => {}
                    }
                }
                Some(ValueType::Map(
                    Box::new(kt.unwrap_or(ValueType::I64)),
                    Box::new(vt.unwrap_or(ValueType::I64)),
                ))
            }

            ExprKind::Construct { ty, args } => {
                self.construct(ty, args, e.span);
                Some(ty.value.clone())
            }

            ExprKind::Decl(d) => {
                self.decl(d);
                None
            }
            ExprKind::Assign { op, lhs, rhs } => {
                self.assign(*op, lhs, rhs, e.span);
                None
            }

            ExprKind::FnDecl(f) => {
                self.fn_decl(f);
                None
            }
            ExprKind::StructDecl(_) | ExprKind::FlowDecl(_) | ExprKind::WrapDecl(_) => None,

            ExprKind::If(i) => {
                let mut ty: T = want.cloned();
                for (c, b) in &i.arms {
                    // 条件は `u1` 一つの領域。
                    //
                    // **求める型を渡さない。** 渡せば整数リテラルが `u1` を名乗り、
                    // `if (0)` が静的に通ってしまう——そして評価器は落とす（S-13）。
                    //
                    // `u1` は真偽であって数ではない。`if (0)` と書きたいなら
                    // `if (1 == 0)` と書く。**`while` が任意の整数を取るのと分けてある**
                    if let Some(got) = self.expr(c, None) {
                        self.unify(&ValueType::U1, &got, c.span);
                    }
                    let t = self.expr(b, ty.as_ref());
                    match (&ty, t) {
                        (None, Some(t)) => ty = Some(t),
                        (Some(w), Some(g)) => self.unify(&w.clone(), &g, b.span),
                        _ => {}
                    }
                }
                if let Some(b) = &i.els {
                    let t = self.expr(b, ty.as_ref());
                    match (&ty, t) {
                        (None, Some(t)) => ty = Some(t),
                        (Some(w), Some(g)) => self.unify(&w.clone(), &g, b.span),
                        _ => {}
                    }
                }
                ty
            }

            ExprKind::Loop(b) => {
                self.loop_body(b, want);
                // 正常終了したループの値は反復回数。脱出で終わればその値
                want.cloned().or(Some(ValueType::I64))
            }
            ExprKind::While { cond, body } => {
                // 条件は**任意の整数型**
                if let Some(t) = self.expr(cond, None) {
                    if !is_int(&t) {
                        self.err("`while` の条件は整数でなければならない", cond.span);
                    }
                }
                self.loop_body(body, want);
                want.cloned().or(Some(ValueType::I64))
            }
            ExprKind::NFor { name, start, count, body } => {
                let st = self.expr(start, None);
                if let Some(t) = &st {
                    if !is_int(t) {
                        self.err("`nfor` の開始は整数でなければならない", start.span);
                    }
                }
                // 回数の型は `i64`（C-86）
                self.expect(count, &ValueType::I64);
                let it = st.unwrap_or(ValueType::I64);
                let w = want.cloned();
                self.scoped(|s| {
                    s.scopes.last_mut().unwrap().insert(name.clone(), it);
                    s.loop_body(body, w.as_ref());
                });
                want.cloned().or(Some(ValueType::I64))
            }

            ExprKind::Switch { subject, arms } => {
                let st = self.expr(subject, None);
                let mut ty: T = want.cloned();
                for a in arms {
                    // 腕は上から順に照合する。照合値は被照合体と同じ型
                    if let Some(s) = &st {
                        self.expect(&a.pattern, &s.clone());
                    } else {
                        self.expr(&a.pattern, None);
                    }
                    let t = self.expr(&a.value, ty.as_ref());
                    match (&ty, t) {
                        (None, Some(t)) => ty = Some(t),
                        (Some(w), Some(g)) => self.unify(&w.clone(), &g, a.value.span),
                        _ => {}
                    }
                }
                ty
            }

            ExprKind::Escape(esc) => {
                self.escape(esc, want);
                None
            }
        }
    }

    fn binary(
        &mut self,
        op: BinOp,
        lhs: &Expr,
        rhs: &Expr,
        want: Option<&ValueType>,
        span: Span,
    ) -> T {
        use BinOp::*;
        match op {
            // `??` は paradox を落とすだけ。**左右は同じ型でなければならない**
            Coalesce => {
                let a = self.expr(lhs, want);
                let b = self.expr(rhs, a.as_ref().or(want));
                match (a, b) {
                    (Some(a), Some(b)) => {
                        self.unify(&a, &b, rhs.span);
                        Some(a)
                    }
                    (Some(a), None) => Some(a),
                    (None, b) => b,
                }
            }
            // 比較の結果は `u1`
            Lt | Le | Gt | Ge | Eq | Ne => {
                let a = self.expr(lhs, None);
                match &a {
                    Some(t) => self.expect(rhs, &t.clone()),
                    None => {
                        self.expr(rhs, None);
                    }
                }
                Some(ValueType::U1)
            }
            And | Or => {
                for e in [lhs, rhs] {
                    if let Some(t) = self.expr(e, None) {
                        if !is_int(&t) {
                            self.err("`&&` `||` は整数にしか使えない", e.span);
                        }
                    }
                }
                Some(ValueType::U1)
            }
            Feed => {
                self.expr(lhs, None);
                self.expr(rhs, None)
            }
            Shl | Shr => {
                let a = self.expr(lhs, want);
                // 桁数は別の型でよい
                self.expr(rhs, Some(&ValueType::I64));
                a
            }
            _ => {
                let a = self.expr(lhs, want);
                match &a {
                    Some(t) => {
                        if !is_num(t) {
                            self.err("この演算子は数値にしか使えない", lhs.span);
                        }
                        self.expect(rhs, &t.clone());
                    }
                    None => {
                        self.expr(rhs, want);
                    }
                }
                a
            }
        }
    }

    fn decl(&mut self, d: &Decl) {
        for b in &d.bindings {
            let want = b.ty.as_ref().map(|t| t.value.clone());
            let t = match &b.init {
                BindInit::Value(e) => {
                    let got = self.expr(e, want.as_ref());
                    match (&want, got) {
                        (Some(w), Some(g)) => {
                            self.unify(&w.clone(), &g, e.span);
                            want.clone()
                        }
                        (Some(_), None) => want.clone(),
                        (None, g) => g,
                    }
                }
                BindInit::AliasOf(target) => {
                    let got = self.lookup(target);
                    if let (Some(w), Some(g)) = (&want, &got) {
                        self.unify(&w.clone(), &g.clone(), b.span);
                    }
                    want.clone().or(got)
                }
            };
            if let Some(t) = t {
                self.scopes.last_mut().unwrap().insert(b.name.clone(), t);
            }
        }
    }

    fn assign(&mut self, op: AssignOp, lhs: &Expr, rhs: &Expr, span: Span) {
        if op == AssignOp::Alias {
            let (ExprKind::Name(n), ExprKind::Name(t)) = (&lhs.kind, &rhs.kind) else { return };
            if let (Some(a), Some(b)) = (self.lookup(n), self.lookup(t)) {
                self.unify(&a, &b, span);
            }
            return;
        }
        match self.expr(lhs, None) {
            Some(t) => self.expect(rhs, &t),
            None => {
                self.expr(rhs, None);
            }
        }
    }

    fn fn_decl(&mut self, f: &FnDecl) {
        let saved = self.frame_base;
        self.scopes.push(HashMap::new());
        self.frame_base = self.scopes.len() - 1;
        for p in &f.params {
            self.scopes.last_mut().unwrap().insert(p.name.clone(), p.ty.value.clone());
        }
        // `->` は**外界面の型**（C-66）。本体の値がそれに合わねばならない
        let want = f.ret.as_ref().map(|t| t.value.clone());
        if let ExprKind::Block(items) = &f.body.kind {
            self.stage_want.push(want.clone());
            self.collect(items);
            let mut last = None;
            for x in items {
                let t = self.expr(x, want.as_ref());
                if !matches!(x.kind, ExprKind::Discard(_)) {
                    last = t;
                }
            }
            if let (Some(w), Some(g)) = (&want, &last) {
                self.unify(&w.clone(), &g.clone(), f.body.span);
            }
            self.stage_want.pop();
        }
        self.frame_base = saved;
        self.scopes.pop();
    }

    fn call(&mut self, callee: &Expr, args: &[Expr], span: Span) -> T {
        if let ExprKind::Field { base, name } = &callee.kind {
            let bt = self.expr(base, None);
            // 利用者定義のメンバ関数（S-1）を先に探す
            if let Some(ValueType::Named(t)) = &bt {
                if let Some(f) = self.fns.get(&format!("{t}.{name}")).cloned() {
                    for (p, a) in f.params[1..].iter().zip(args) {
                        self.expect(a, &p.ty.value);
                    }
                    return f.ret.map(|r| r.value);
                }
            }
            return match name.as_str() {
                "len" => {
                    for a in args {
                        self.expr(a, None);
                    }
                    Some(ValueType::I64)
                }
                "push" => {
                    let want = match &bt {
                        Some(ValueType::Array(el)) => Some((**el).clone()),
                        Some(ValueType::Str) => Some(ValueType::U8),
                        _ => None,
                    };
                    for a in args {
                        match &want {
                            Some(w) => self.expect(a, &w.clone()),
                            None => {
                                self.expr(a, None);
                            }
                        }
                    }
                    None
                }
                "pop" => match &bt {
                    Some(ValueType::Array(el)) => Some((**el).clone()),
                    Some(ValueType::Str) => Some(ValueType::U8),
                    _ => None,
                },
                "has" | "utf8_valid" => {
                    for a in args {
                        self.expr(a, None);
                    }
                    Some(ValueType::U1)
                }
                "utf8_len" => Some(ValueType::I64),
                "utf8_at" => {
                    for a in args {
                        self.expect(a, &ValueType::I64);
                    }
                    Some(ValueType::I32)
                }
                "keys" => match &bt {
                    Some(ValueType::Map(k, _)) => Some(ValueType::Array(k.clone())),
                    _ => None,
                },
                "remove" => match &bt {
                    Some(ValueType::Array(el)) => {
                        for a in args {
                            self.expect(a, &ValueType::I64);
                        }
                        Some((**el).clone())
                    }
                    Some(ValueType::Map(k, v)) => {
                        for a in args {
                            self.expect(a, &k.clone());
                        }
                        Some((**v).clone())
                    }
                    _ => None,
                },
                _ => {
                    for a in args {
                        self.expr(a, None);
                    }
                    None
                }
            };
        }
        let ExprKind::Name(name) = &callee.kind else { return None };
        if name == "getdepth" {
            return Some(ValueType::I64);
        }
        let Some(f) = self.fns.get(name).cloned() else {
            for a in args {
                self.expr(a, None);
            }
            return None;
        };
        for (p, a) in f.params.iter().zip(args) {
            self.expect(a, &p.ty.value);
        }
        for a in args.iter().skip(f.params.len()) {
            self.expr(a, None);
        }
        let _ = span;
        f.ret.map(|t| t.value)
    }

    fn construct(&mut self, ty: &Type, args: &CtorArgs, span: Span) {
        match (&ty.value, args) {
            (ValueType::Named(n), CtorArgs::Named(given)) => {
                let Some(s) = self.structs.get(n).cloned() else { return };
                for (fname, e) in given {
                    match s.fields.iter().find(|f| &f.name == fname) {
                        Some(f) => self.expect(e, &f.ty.value),
                        None => self.err(format!("`{n}` に欄 `{fname}` は無い"), e.span),
                    }
                }
                for f in &s.fields {
                    if f.default.is_none() && !given.iter().any(|(g, _)| g == &f.name) {
                        self.err(format!("欄 `{}` に値が無い", f.name), span);
                    }
                }
            }
            (ValueType::Array(el), CtorArgs::Positional(a)) => {
                if let Some(n) = a.first() {
                    self.expect(n, &ValueType::I64);
                }
                if let Some(fill) = a.get(1) {
                    self.expect(fill, el);
                }
            }
            (ValueType::Str, CtorArgs::Positional(a)) => {
                for e in a {
                    self.expr(e, Some(&ValueType::Array(Box::new(ValueType::U8))));
                }
            }
            // ラップ型（S-2）。包むのも剥がすのも `new`
            (ValueType::Named(n), CtorArgs::Positional(a)) if self.wraps.contains_key(n) => {
                let base = self.wraps[n].clone();
                for e in a {
                    self.expect(e, &base);
                }
            }
            (ValueType::Named(n), CtorArgs::Positional(a)) if a.is_empty() => {
                let Some(s) = self.structs.get(n).cloned() else { return };
                for f in &s.fields {
                    if f.default.is_none() {
                        self.err(format!("欄 `{}` に値が無い", f.name), span);
                    }
                }
            }
            (_, CtorArgs::Positional(a)) => {
                for e in a {
                    self.expr(e, None);
                }
            }
            (_, CtorArgs::Named(v)) => {
                for (_, e) in v {
                    self.expr(e, None);
                }
            }
        }
    }

    /// 脱出。**運ぶ値は行き先の段に置かれる**ので、その型でなければならない。
    fn escape(&mut self, esc: &Escape, _want: Option<&ValueType>) {
        let stages = escape_stages(esc);
        // 段送りは内側から外へ。行き先の段の型を取る
        let target = stages
            .and_then(|n| {
                let len = self.stage_want.len();
                if (n as usize) <= len {
                    self.stage_want[len - n as usize].clone()
                } else {
                    None
                }
            });
        self.escape_operand(esc, target.as_ref());
    }

    fn escape_operand(&mut self, esc: &Escape, want: Option<&ValueType>) {
        match &esc.operand {
            Some(Operand::Value(v)) => match want {
                Some(w) => self.expect(v, &w.clone()),
                None => {
                    self.expr(v, None);
                }
            },
            Some(Operand::Escape(x)) => self.escape_operand(x, want),
            None => {}
        }
        if let EscapeKind::Flow { args, .. } = &esc.kind {
            for a in args {
                match a {
                    FlowArg::Value(v) => {
                        self.expr(v, None);
                    }
                    FlowArg::Escape(x) => self.escape_operand(x, None),
                }
            }
        }
    }

    fn loop_body(&mut self, body: &Expr, want: Option<&ValueType>) {
        let ExprKind::Block(items) = &body.kind else {
            self.expr(body, None);
            return;
        };
        self.scopes.push(HashMap::new());
        // 本体は**その構文の段**。二重にはならない（C-64）
        self.stage_want.push(want.cloned());
        self.collect(items);
        for x in items {
            self.expr(x, None);
        }
        self.stage_want.pop();
        self.scopes.pop();
    }
}

fn is_int(t: &ValueType) -> bool {
    matches!(
        t,
        ValueType::U1
            | ValueType::U8
            | ValueType::U16
            | ValueType::U32
            | ValueType::I32
            | ValueType::I64
    )
}

fn is_float(t: &ValueType) -> bool {
    matches!(t, ValueType::F32 | ValueType::F64)
}

fn is_num(t: &ValueType) -> bool {
    is_int(t) || is_float(t)
}

pub fn show(t: &ValueType) -> String {
    use ValueType::*;
    match t {
        U1 => "u1".into(),
        U8 => "u8".into(),
        U16 => "u16".into(),
        U32 => "u32".into(),
        I32 => "i32".into(),
        I64 => "i64".into(),
        F32 => "f32".into(),
        F64 => "f64".into(),
        Str => "str".into(),
        Array(i) => format!("{} array", show(i)),
        Map(k, v) => format!("{} {} map", show(k), show(v)),
        Named(n) => n.clone(),
    }
}

/// 脱出の段数。**`break X` は足し、`continue X` は足さない**（C-92）。
/// 静的に決まらなければ `None`。
fn escape_stages(esc: &Escape) -> Option<u32> {
    match &esc.kind {
        EscapeKind::Break { .. } => match &esc.operand {
            Some(Operand::Escape(inner)) => escape_stages(inner).map(|n| n + 1),
            _ => Some(1),
        },
        EscapeKind::Continue => Some(1),
        EscapeKind::Flow { .. } => None,
    }
}
