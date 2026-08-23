//! Pratt 解析。**束縛力表が一次仕様であり、BNF はそれに従属する**（C-61）。
//!
//! 解析器は `min_bp` のほかに **`has_value`** を持つ（C-85 (4)）。
//! `;` は左辺の**領域**を要求し、`+` `-` は左辺の**値**を要求する——
//! 空の領域から `;` は paradox を得るが、`-` は何も得ない。

use crate::ast::*;
use crate::lexer::{lex, Tok, Token};
use crate::span::Span;

// ---- 束縛力（docs/vaak/16-形式構文.md の表） ----
const BP_POSTFIX: u8 = 90;
const BP_PREFIX: u8 = 80;
const BP_ESCAPE: u8 = 8;

/// 中置演算子の (左, 右)。
fn infix_bp(t: &Tok) -> Option<(u8, u8)> {
    Some(match t {
        Tok::Star | Tok::Slash | Tok::Mod => (70, 71),
        Tok::Coalesce => (60, 31), // 非対称。左は強く、右は緩く（C-45）
        Tok::Plus | Tok::Minus => (50, 51),
        Tok::Shl | Tok::Shr => (45, 46),
        Tok::Amp => (42, 43),
        Tok::Caret => (40, 41),
        Tok::Pipe => (38, 39),
        Tok::Lt | Tok::Le | Tok::Gt | Tok::Ge | Tok::EqEq | Tok::Ne => (30, 31),
        Tok::AndAnd => (25, 26),
        Tok::OrOr => (20, 21),
        Tok::Feed => (15, 16),
        Tok::Assign
        | Tok::AliasBind
        | Tok::PlusEq
        | Tok::MinusEq
        | Tok::StarEq
        | Tok::SlashEq
        | Tok::ModEq
        | Tok::ShlEq
        | Tok::ShrEq
        | Tok::CaretEq
        | Tok::PipeEq => (7, 6),
        _ => return None,
    })
}

fn bin_op(t: &Tok) -> Option<BinOp> {
    Some(match t {
        Tok::Star => BinOp::Mul,
        Tok::Slash => BinOp::Div,
        Tok::Mod => BinOp::Mod,
        Tok::Coalesce => BinOp::Coalesce,
        Tok::Plus => BinOp::Add,
        Tok::Minus => BinOp::Sub,
        Tok::Shl => BinOp::Shl,
        Tok::Shr => BinOp::Shr,
        Tok::Amp => BinOp::BitAnd,
        Tok::Caret => BinOp::BitXor,
        Tok::Pipe => BinOp::BitOr,
        Tok::Lt => BinOp::Lt,
        Tok::Le => BinOp::Le,
        Tok::Gt => BinOp::Gt,
        Tok::Ge => BinOp::Ge,
        Tok::EqEq => BinOp::Eq,
        Tok::Ne => BinOp::Ne,
        Tok::AndAnd => BinOp::And,
        Tok::OrOr => BinOp::Or,
        Tok::Feed => BinOp::Feed,
        _ => return None,
    })
}

fn assign_op(t: &Tok) -> Option<AssignOp> {
    Some(match t {
        Tok::Assign => AssignOp::Set,
        Tok::AliasBind => AssignOp::Alias,
        Tok::PlusEq => AssignOp::Add,
        Tok::MinusEq => AssignOp::Sub,
        Tok::StarEq => AssignOp::Mul,
        Tok::SlashEq => AssignOp::Div,
        Tok::ModEq => AssignOp::Mod,
        Tok::ShlEq => AssignOp::Shl,
        Tok::ShrEq => AssignOp::Shr,
        Tok::CaretEq => AssignOp::BitXor,
        Tok::PipeEq => AssignOp::BitOr,
        _ => return None,
    })
}

#[derive(Clone, PartialEq, Debug)]
pub struct ParseError {
    pub msg: String,
    pub span: Span,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.msg)
    }
}

pub fn parse(src: &str) -> Result<Program, ParseError> {
    let toks = lex(src).map_err(|e| ParseError { msg: e.msg, span: e.span })?;
    Parser::new(toks).program()
}

pub fn parse_expr(src: &str) -> Result<Expr, ParseError> {
    let mut p = Parser::new(lex(src).map_err(|e| ParseError { msg: e.msg, span: e.span })?);
    let e = p.expr(0, false)?;
    p.expect(Tok::Eof)?;
    Ok(e)
}

struct Parser {
    toks: Vec<Token>,
    pos: usize,
    next_id: NodeId,
}

impl Parser {
    fn new(toks: Vec<Token>) -> Self {
        Self { toks, pos: 0, next_id: 0 }
    }

    // ---- 基本操作 ----

    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn peek_at(&self, n: usize) -> &Tok {
        let i = (self.pos + n).min(self.toks.len() - 1);
        &self.toks[i].tok
    }

    fn span(&self) -> Span {
        self.toks[self.pos].span
    }

    fn prev_span(&self) -> Span {
        self.toks[self.pos.saturating_sub(1)].span
    }

    fn bump(&mut self) -> Tok {
        let t = self.toks[self.pos].tok.clone();
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, t: Tok) -> bool {
        if *self.peek() == t {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, t: Tok) -> Result<(), ParseError> {
        if *self.peek() == t {
            self.bump();
            Ok(())
        } else {
            Err(self.err(format!("{t:?} を期待したが {:?} が来た", self.peek())))
        }
    }

    fn err(&self, msg: impl Into<String>) -> ParseError {
        ParseError { msg: msg.into(), span: self.span() }
    }

    fn ident(&mut self) -> Result<String, ParseError> {
        match self.bump() {
            Tok::Ident(s) => Ok(s),
            other => Err(ParseError {
                msg: format!("名前を期待したが {other:?} が来た"),
                span: self.prev_span(),
            }),
        }
    }

    fn node(&mut self, kind: ExprKind, span: Span) -> Expr {
        let id = self.next_id;
        self.next_id += 1;
        Expr { kind, span, id }
    }

    // ---- program ----

    fn program(mut self) -> Result<Program, ParseError> {
        let mut body = Vec::new();
        while *self.peek() != Tok::Eof {
            body.push(self.expr(0, false)?);
        }
        Ok(Program { body })
    }

    /// 領域の中身。閉じ記号まで `E@0` を繰り返す。
    fn region_body(&mut self, close: Tok) -> Result<Vec<Expr>, ParseError> {
        let mut out = Vec::new();
        while *self.peek() != close && *self.peek() != Tok::Eof {
            out.push(self.expr(0, false)?);
        }
        self.expect(close)?;
        Ok(out)
    }

    // ---- 式 ----

    /// `parse(min_bp)`。`no_coalesce` は**この呼び出しの最上位でだけ** `??` を消費させない
    /// （`switch` の腕。束縛力では書けないので旗を立てる）。
    fn expr(&mut self, min_bp: u8, no_coalesce: bool) -> Result<Expr, ParseError> {
        let (mut lhs, mut has_value) = self.prefix()?;

        loop {
            let t = self.peek().clone();

            // `;` は左辺の**領域**を要求する。値が無くても働く。
            if t == Tok::Semi {
                if 5 < min_bp {
                    break;
                }
                self.bump();
                let span = lhs.span.to(self.prev_span());
                lhs = self.node(ExprKind::Discard(Some(Box::new(lhs))), span);
                has_value = false;
                continue;
            }

            // 後置：呼び出し・添字・欄
            if has_value && BP_POSTFIX >= min_bp {
                match t {
                    Tok::LParen => {
                        self.bump();
                        let mut args = Vec::new();
                        if *self.peek() != Tok::RParen {
                            loop {
                                args.push(self.expr(0, false)?);
                                if !self.eat(Tok::Comma) {
                                    break;
                                }
                            }
                        }
                        self.expect(Tok::RParen)?;
                        let span = lhs.span.to(self.prev_span());
                        lhs = self.node(ExprKind::Call { callee: Box::new(lhs), args }, span);
                        continue;
                    }
                    Tok::LBracket => {
                        self.bump();
                        let index = self.expr(0, false)?;
                        self.expect(Tok::RBracket)?;
                        let span = lhs.span.to(self.prev_span());
                        lhs = self.node(
                            ExprKind::Index { base: Box::new(lhs), index: Box::new(index) },
                            span,
                        );
                        continue;
                    }
                    Tok::Dot => {
                        self.bump();
                        let name = self.ident()?;
                        let span = lhs.span.to(self.prev_span());
                        lhs = self.node(ExprKind::Field { base: Box::new(lhs), name }, span);
                        continue;
                    }
                    // `E -> T` — **領域に型を付ける**（C-30）。
                    // 後置なので、`200 -> u8 + 100` は `(200 -> u8) + 100`
                    Tok::Arrow => {
                        self.bump();
                        let ty = self.ty()?;
                        let span = lhs.span.to(self.prev_span());
                        lhs = self.node(
                            ExprKind::Ascribe { expr: Box::new(lhs), ty },
                            span,
                        );
                        continue;
                    }
                    _ => {}
                }
            }

            // 中置：左辺の**値**を要求する
            let Some((lbp, rbp)) = infix_bp(&t) else { break };
            if lbp < min_bp || !has_value {
                break;
            }
            if no_coalesce && t == Tok::Coalesce {
                break; // 腕の値は最上位の `??` を消費しない（C-39）
            }
            self.bump();
            let rhs = self.expr(rbp, false)?;
            let span = lhs.span.to(rhs.span);
            lhs = if let Some(op) = assign_op(&t) {
                self.node(
                    ExprKind::Assign { op, lhs: Box::new(lhs), rhs: Box::new(rhs) },
                    span,
                )
            } else {
                let op = bin_op(&t).expect("中置演算子のはず");
                self.node(ExprKind::Binary { op, lhs: Box::new(lhs), rhs: Box::new(rhs) }, span)
            };
        }

        Ok(lhs)
    }

    /// 前置または一次式。返り値の `bool` は「左辺に値があるか」。
    fn prefix(&mut self) -> Result<(Expr, bool), ParseError> {
        let start = self.span();
        let t = self.peek().clone();

        // `;` が一次式の位置に来たら、左辺は空の領域（C-80）
        if t == Tok::Semi {
            self.bump();
            let e = self.node(ExprKind::Discard(None), start.to(self.prev_span()));
            return Ok((e, false));
        }

        // 脱出は一次式ではない。**前置演算子である**（C-61）
        if matches!(t, Tok::Break | Tok::Continue | Tok::FlowName(_)) {
            let esc = self.escape()?;
            let span = esc.span;
            let e = self.node(ExprKind::Escape(Box::new(esc)), span);
            return Ok((e, true));
        }

        let e = match t {
            Tok::Minus | Tok::Plus | Tok::Bang => {
                self.bump();
                let op = match t {
                    Tok::Minus => UnOp::Neg,
                    Tok::Plus => UnOp::Pos,
                    _ => UnOp::Not,
                };
                let rhs = self.expr(BP_PREFIX, false)?;
                let span = start.to(rhs.span);
                self.node(ExprKind::Unary { op, rhs: Box::new(rhs) }, span)
            }
            Tok::Int(s) => {
                self.bump();
                self.node(ExprKind::Int(s), start)
            }
            Tok::Float(s) => {
                self.bump();
                self.node(ExprKind::Float(s), start)
            }
            Tok::Str(s) => {
                self.bump();
                self.node(ExprKind::Str(s), start)
            }
            // **型は `u1` で確定している。** 置かれた場所を見ない（C-97）
            Tok::True => {
                self.bump();
                self.node(ExprKind::Bool(true), start)
            }
            Tok::False => {
                self.bump();
                self.node(ExprKind::Bool(false), start)
            }
            Tok::Ident(s) => {
                self.bump();
                self.node(ExprKind::Name(s), start)
            }
            Tok::LParen => self.paren_or_map()?,
            Tok::LBrace => {
                self.bump();
                let body = self.region_body(Tok::RBrace)?;
                self.node(ExprKind::Block(body), start.to(self.prev_span()))
            }
            Tok::LBracket => {
                self.bump();
                let mut items = Vec::new();
                if *self.peek() != Tok::RBracket {
                    loop {
                        items.push(self.expr(0, false)?);
                        if !self.eat(Tok::Comma) {
                            break;
                        }
                    }
                }
                self.expect(Tok::RBracket)?;
                self.node(ExprKind::ArrayLit(items), start.to(self.prev_span()))
            }
            Tok::Var | Tok::Let | Tok::Const => self.decl()?,
            Tok::Fn => self.fn_decl()?,
            Tok::Flow => self.flow_decl()?,
            Tok::Struct => self.struct_decl()?,
            Tok::Wrap => self.wrap_decl()?,
            Tok::New => self.construct()?,
            Tok::If => self.if_expr()?,
            Tok::Loop => {
                self.bump();
                let body = self.block()?;
                self.node(ExprKind::Loop(Box::new(body)), start.to(self.prev_span()))
            }
            Tok::While => {
                self.bump();
                let cond = self.paren_expr()?;
                let body = self.block()?;
                self.node(
                    ExprKind::While { cond: Box::new(cond), body: Box::new(body) },
                    start.to(self.prev_span()),
                )
            }
            Tok::Nfor => self.nfor()?,
            Tok::Switch => self.switch()?,
            other => {
                return Err(ParseError {
                    msg: format!("式が要る位置に {other:?} が来た"),
                    span: start,
                })
            }
        };
        Ok((e, true))
    }

    // ---- 括弧：領域か写像リテラルか ----

    /// `(` の中の**最上位**に `=>` が現れるかで決まる。任意長の先読み。
    ///
    /// ただし `switch` の腕の `=>` も最上位に現れる。**`case` が消費する分は数えない。**
    fn paren_is_map(&self) -> bool {
        let mut i = self.pos + 1;
        let mut depth = 0usize;
        let mut pending_case = 0usize;
        while i < self.toks.len() {
            match &self.toks[i].tok {
                Tok::LParen | Tok::LBracket | Tok::LBrace => depth += 1,
                Tok::RParen | Tok::RBracket | Tok::RBrace => {
                    if depth == 0 {
                        return false;
                    }
                    depth -= 1;
                }
                Tok::Case if depth == 0 => pending_case += 1,
                Tok::FatArrow if depth == 0 => {
                    if pending_case == 0 {
                        return true;
                    }
                    pending_case -= 1;
                }
                Tok::Eof => return false,
                _ => {}
            }
            i += 1;
        }
        false
    }

    fn paren_or_map(&mut self) -> Result<Expr, ParseError> {
        let start = self.span();
        if self.paren_is_map() {
            self.bump(); // (
            let mut pairs = Vec::new();
            loop {
                let k = self.expr(0, false)?;
                self.expect(Tok::FatArrow)?;
                let v = self.expr(0, false)?;
                pairs.push((k, v));
                if !self.eat(Tok::Comma) {
                    break;
                }
            }
            self.expect(Tok::RParen)?;
            Ok(self.node(ExprKind::MapLit(pairs), start.to(self.prev_span())))
        } else {
            self.bump(); // (
            let body = self.region_body(Tok::RParen)?;
            Ok(self.node(ExprKind::Paren(body), start.to(self.prev_span())))
        }
    }

    fn paren_expr(&mut self) -> Result<Expr, ParseError> {
        if *self.peek() != Tok::LParen {
            return Err(self.err("`(` が要る"));
        }
        self.paren_or_map()
    }

    fn block(&mut self) -> Result<Expr, ParseError> {
        let start = self.span();
        self.expect(Tok::LBrace)?;
        let body = self.region_body(Tok::RBrace)?;
        Ok(self.node(ExprKind::Block(body), start.to(self.prev_span())))
    }

    // ---- 宣言 ----

    fn decl(&mut self) -> Result<Expr, ParseError> {
        let start = self.span();
        let kind = match self.bump() {
            Tok::Var => BindKind::Var,
            Tok::Let => BindKind::Let,
            _ => BindKind::Const,
        };
        let mut bindings = Vec::new();
        loop {
            let bstart = self.span();
            let name = self.ident()?;
            let mut ty = None;
            if self.eat(Tok::Colon) {
                ty = Some(self.ty()?);
            }
            let is_alias = ty.as_ref().map(|t: &Type| t.is_alias).unwrap_or(false);
            let init = if self.eat(Tok::Assign) {
                BindInit::Value(self.expr(6, false)?) // `:=` の右束縛力（表）
            } else if self.eat(Tok::AliasBind) {
                // 対象は名前だけ（C-53）。経路が続くなら、そう言う
                let target = self.ident()?;
                if matches!(self.peek(), Tok::LBracket | Tok::Dot) {
                    return Err(self.err("`&=` の対象は名前だけ。経路には張れない"));
                }
                BindInit::AliasOf(target)
            } else {
                return Err(self.err("`:=` か `&=` が要る"));
            };
            bindings.push(Binding {
                name,
                ty,
                is_alias,
                init,
                span: bstart.to(self.prev_span()),
            });
            if !self.eat(Tok::Comma) {
                break;
            }
        }
        Ok(self.node(ExprKind::Decl(Decl { kind, bindings }), start.to(self.prev_span())))
    }

    fn fn_decl(&mut self) -> Result<Expr, ParseError> {
        let start = self.span();
        self.expect(Tok::Fn)?;
        let first = self.ident()?;
        // `fn T.m` なら型の名前空間に入る（S-1）
        let (owner, name) = if self.eat(Tok::Dot) {
            (Some(first), self.ident()?)
        } else {
            (None, first)
        };
        self.expect(Tok::LParen)?;
        let mut params = Vec::new();
        if *self.peek() != Tok::RParen {
            loop {
                let pstart = self.span();
                let kind = match self.peek() {
                    Tok::Var => {
                        self.bump();
                        BindKind::Var
                    }
                    Tok::Let => {
                        self.bump();
                        BindKind::Let
                    }
                    Tok::Const => {
                        self.bump();
                        BindKind::Const
                    }
                    _ => BindKind::Let, // 既定は let（C-40）
                };
                let pname = self.ident()?;
                // **`self` にだけ型注釈を書かない**（型が `fn T.m` から決まる。S-1）
                let ty = if pname == "self" && owner.is_some() && params.is_empty() {
                    let t = owner.clone().unwrap();
                    Type {
                        value: ValueType::Named(t),
                        is_alias: true,
                        span: pstart.to(self.prev_span()),
                    }
                } else {
                    self.expect(Tok::Colon)?; // 注釈は省けない
                    self.ty()?
                };
                params.push(Param { kind, name: pname, ty, span: pstart.to(self.prev_span()) });
                if !self.eat(Tok::Comma) {
                    break;
                }
            }
        }
        self.expect(Tok::RParen)?;
        let body = self.block()?;
        // `->` は**本体の領域**に付く（C-30）。だから本体の直後に来る
        let ret = if self.eat(Tok::Arrow) { Some(self.ty()?) } else { None };
        let span = start.to(self.prev_span());
        Ok(self.node(
            ExprKind::FnDecl(FnDecl { owner, name, params, body: Box::new(body), ret, span }),
            span,
        ))
    }

    fn flow_decl(&mut self) -> Result<Expr, ParseError> {
        let start = self.span();
        self.expect(Tok::Flow)?;
        let name = match self.bump() {
            Tok::FlowName(s) => s,
            other => {
                return Err(ParseError {
                    msg: format!("`$` で始まる名前が要るが {other:?} が来た"),
                    span: self.prev_span(),
                })
            }
        };
        // `=` は定義であって束縛演算子ではない（C-15）
        self.expect(Tok::Define)?;
        let body = self.escape()?;
        let span = start.to(self.prev_span());
        Ok(self.node(ExprKind::FlowDecl(FlowDecl { name, body: Box::new(body), span }), span))
    }

    /// `wrap 名前 = 型;`（S-2）。`=` は定義であって束縛演算子ではない
    fn wrap_decl(&mut self) -> Result<Expr, ParseError> {
        let start = self.span();
        self.expect(Tok::Wrap)?;
        let name = self.ident()?;
        self.expect(Tok::Define)?;
        let base = self.ty()?;
        let span = start.to(self.prev_span());
        Ok(self.node(ExprKind::WrapDecl(WrapDecl { name, base, span }), span))
    }

    fn struct_decl(&mut self) -> Result<Expr, ParseError> {
        let start = self.span();
        self.expect(Tok::Struct)?;
        let name = self.ident()?;
        self.expect(Tok::LBrace)?;
        let mut fields = Vec::new();
        while *self.peek() != Tok::RBrace && *self.peek() != Tok::Eof {
            let fstart = self.span();
            let kind = match self.bump() {
                Tok::Var => BindKind::Var,
                Tok::Let => BindKind::Let,
                Tok::Const => BindKind::Const,
                other => {
                    return Err(ParseError {
                        msg: format!("欄には束縛種が要るが {other:?} が来た"),
                        span: self.prev_span(),
                    })
                }
            };
            let fname = self.ident()?;
            self.expect(Tok::Colon)?;
            let ty = self.ty()?; // `alias` は書けない（欄は値の場所）
            let default = if self.eat(Tok::Assign) { Some(self.expr(6, false)?) } else { None };
            self.expect(Tok::Semi)?;
            fields.push(Field { kind, name: fname, ty, default, span: fstart.to(self.prev_span()) });
        }
        self.expect(Tok::RBrace)?;
        let span = start.to(self.prev_span());
        Ok(self.node(ExprKind::StructDecl(StructDecl { name, fields, span }), span))
    }

    fn construct(&mut self) -> Result<Expr, ParseError> {
        let start = self.span();
        self.expect(Tok::New)?;
        let ty = self.ty()?;
        self.expect(Tok::LParen)?;
        // 欄を名前で与えるか、位置で与えるか。`IDENT` の次が `:=` かの二トークン先読み
        let named = matches!(self.peek(), Tok::Ident(_)) && *self.peek_at(1) == Tok::Assign;
        let args = if *self.peek() == Tok::RParen {
            CtorArgs::Positional(Vec::new())
        } else if named {
            let mut out = Vec::new();
            loop {
                let f = self.ident()?;
                self.expect(Tok::Assign)?;
                out.push((f, self.expr(0, false)?));
                if !self.eat(Tok::Comma) {
                    break;
                }
            }
            CtorArgs::Named(out)
        } else {
            let mut out = Vec::new();
            loop {
                out.push(self.expr(0, false)?);
                if !self.eat(Tok::Comma) {
                    break;
                }
            }
            CtorArgs::Positional(out)
        };
        self.expect(Tok::RParen)?;
        Ok(self.node(ExprKind::Construct { ty, args }, start.to(self.prev_span())))
    }

    // ---- 制御 ----

    fn if_expr(&mut self) -> Result<Expr, ParseError> {
        let start = self.span();
        self.expect(Tok::If)?;
        let mut arms = Vec::new();
        let cond = self.paren_expr()?;
        let then = self.expr(0, false)?; // 分岐は E@0。`;` を吸う（C-68）
        arms.push((cond, then));
        while self.eat(Tok::Elif) {
            let c = self.paren_expr()?;
            let b = self.expr(0, false)?;
            arms.push((c, b));
        }
        let els = if self.eat(Tok::Else) { Some(Box::new(self.expr(0, false)?)) } else { None };
        self.expect(Tok::Fi)?;
        Ok(self.node(ExprKind::If(If { arms, els }), start.to(self.prev_span())))
    }

    fn nfor(&mut self) -> Result<Expr, ParseError> {
        let start = self.span();
        self.expect(Tok::Nfor)?;
        self.expect(Tok::LParen)?;
        let name = self.ident()?;
        self.expect(Tok::Comma)?;
        let s = self.expr(0, false)?;
        self.expect(Tok::Comma)?;
        let c = self.expr(0, false)?;
        self.expect(Tok::RParen)?;
        let body = self.block()?;
        Ok(self.node(
            ExprKind::NFor {
                name,
                start: Box::new(s),
                count: Box::new(c),
                body: Box::new(body),
            },
            start.to(self.prev_span()),
        ))
    }

    fn switch(&mut self) -> Result<Expr, ParseError> {
        let start = self.span();
        self.expect(Tok::Switch)?;
        let subject = self.paren_expr()?;
        let mut arms = Vec::new();
        // `arm+` は区切りも終端も持たない唯一の繰り返し。**貪欲に読む**
        while *self.peek() == Tok::Case {
            let astart = self.span();
            self.bump();
            let pattern = self.expr(0, false)?;
            self.expect(Tok::FatArrow)?;
            // 腕は `E@6`。`;` を吸わず、最上位の `??` も消費しない（C-82）
            let value = self.expr(6, true)?;
            arms.push(Arm { pattern, value, span: astart.to(self.prev_span()) });
        }
        if arms.is_empty() {
            return Err(self.err("`switch` には腕が要る"));
        }
        Ok(self.node(
            ExprKind::Switch { subject: Box::new(subject), arms },
            start.to(self.prev_span()),
        ))
    }

    // ---- 脱出 ----

    /// 次のトークンが式を始められるか（`FIRST` 集合。一トークンの先読み）。
    fn starts_expr(&self) -> bool {
        matches!(
            self.peek(),
            Tok::Int(_)
                | Tok::Float(_)
                | Tok::Str(_)
                | Tok::True
                | Tok::False
                | Tok::Ident(_)
                | Tok::FlowName(_)
                | Tok::LParen
                | Tok::LBrace
                | Tok::LBracket
                | Tok::Minus
                | Tok::Plus
                | Tok::Bang
                | Tok::Var
                | Tok::Let
                | Tok::Const
                | Tok::Fn
                | Tok::Flow
                | Tok::Struct
                | Tok::New
                | Tok::If
                | Tok::Loop
                | Tok::While
                | Tok::Nfor
                | Tok::Switch
                | Tok::Break
                | Tok::Continue
        )
    }

    fn escape(&mut self) -> Result<Escape, ParseError> {
        let start = self.span();
        match self.peek().clone() {
            Tok::Break => {
                self.bump();
                if self.eat(Tok::Outward) {
                    // `outward` は**脱出**を要求する（C-70 (3)）
                    let inner = self.escape()?;
                    return Ok(Escape {
                        kind: EscapeKind::Break { outward: true },
                        operand: Some(Operand::Escape(Box::new(inner))),
                        span: start.to(self.prev_span()),
                    });
                }
                let operand = self.operand()?;
                Ok(Escape {
                    kind: EscapeKind::Break { outward: false },
                    operand,
                    span: start.to(self.prev_span()),
                })
            }
            Tok::Continue => {
                self.bump();
                // `continue` は**値を取れない**。脱出か虚無だけ（C-71）
                let operand = if matches!(self.peek(), Tok::Break | Tok::Continue | Tok::FlowName(_))
                {
                    Some(Operand::Escape(Box::new(self.escape()?)))
                } else if self.starts_expr() {
                    // 文法では表せないが、書き手の意図は明らかなので、そう言う
                    return Err(self.err("`continue` は値を取れない。脱出か、何も書かないか"));
                } else {
                    None
                };
                Ok(Escape { kind: EscapeKind::Continue, operand, span: start.to(self.prev_span()) })
            }
            Tok::FlowName(name) => {
                self.bump();
                let mut args = Vec::new();
                if *self.peek() == Tok::LParen {
                    self.bump();
                    if *self.peek() != Tok::RParen {
                        loop {
                            // 作用素式は値の引数位置に置けないので分けている
                            if matches!(
                                self.peek(),
                                Tok::Break | Tok::Continue | Tok::FlowName(_)
                            ) {
                                args.push(FlowArg::Escape(self.escape()?));
                            } else {
                                args.push(FlowArg::Value(self.expr(0, false)?));
                            }
                            if !self.eat(Tok::Comma) {
                                break;
                            }
                        }
                    }
                    self.expect(Tok::RParen)?;
                }
                let operand = self.operand()?;
                Ok(Escape {
                    kind: EscapeKind::Flow { name, args },
                    operand,
                    span: start.to(self.prev_span()),
                })
            }
            other => Err(ParseError {
                msg: format!("脱出を期待したが {other:?} が来た"),
                span: start,
            }),
        }
    }

    /// 被演算子＝`escape` または `parse(8)` で読んだ式。省略できる。
    fn operand(&mut self) -> Result<Option<Operand>, ParseError> {
        if matches!(self.peek(), Tok::Break | Tok::Continue | Tok::FlowName(_)) {
            return Ok(Some(Operand::Escape(Box::new(self.escape()?))));
        }
        if self.starts_expr() {
            return Ok(Some(Operand::Value(self.expr(BP_ESCAPE, false)?)));
        }
        Ok(None)
    }

    // ---- 型（逆ポーランド） ----

    fn ty(&mut self) -> Result<Type, ParseError> {
        let start = self.span();
        let mut stack: Vec<ValueType> = Vec::new();
        loop {
            match self.peek().clone() {
                Tok::Ident(name) => {
                    self.bump();
                    stack.push(base_type(&name));
                }
                Tok::Array => {
                    self.bump();
                    let Some(inner) = stack.pop() else {
                        return Err(self.err("`array` は 1 つ取るが、積まれていない"));
                    };
                    stack.push(ValueType::Array(Box::new(inner)));
                }
                Tok::Map => {
                    self.bump();
                    // 鍵、値の順に積まれている
                    let Some(val) = stack.pop() else {
                        return Err(self.err("`map` は 2 つ取るが、足りない"));
                    };
                    let Some(key) = stack.pop() else {
                        return Err(self.err("`map` は 2 つ取るが、足りない"));
                    };
                    stack.push(ValueType::Map(Box::new(key), Box::new(val)));
                }
                Tok::Hash => {
                    self.bump();
                    // 鍵、値の順に積まれている（`map` と同じ）
                    let Some(val) = stack.pop() else {
                        return Err(self.err("`hash` は 2 つ取るが、足りない"));
                    };
                    let Some(key) = stack.pop() else {
                        return Err(self.err("`hash` は 2 つ取るが、足りない"));
                    };
                    stack.push(ValueType::Hash(Box::new(key), Box::new(val)));
                }
                _ => break,
            }
        }
        // `alias` は修飾子ではない。**最も外側にのみ**（C-48）
        let is_alias = self.eat(Tok::Alias);
        if stack.len() != 1 {
            return Err(ParseError {
                msg: format!("型は最後にちょうど一つ残らねばならない（{} 個残った）", stack.len()),
                span: start.to(self.prev_span()),
            });
        }
        Ok(Type { value: stack.pop().unwrap(), is_alias, span: start.to(self.prev_span()) })
    }
}

fn base_type(name: &str) -> ValueType {
    match name {
        "u1" => ValueType::U1,
        // **`bool` は `u1` の別綴りである。** 包み型ではない——構文糖であり、
        // 型としては同じものになる。`\show` しても `u1` と出る
        "bool" => ValueType::U1,
        "u8" => ValueType::U8,
        "u16" => ValueType::U16,
        "u32" => ValueType::U32,
        "i32" => ValueType::I32,
        "i64" => ValueType::I64,
        "f32" => ValueType::F32,
        "f64" => ValueType::F64,
        // **STEEL 方言の基底型**（S-20）
        "f80" => ValueType::F80,
        "str" => ValueType::Str,
        other => ValueType::Named(other.to_string()),
    }
}
