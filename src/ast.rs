//! AST。
//!
//! **脱出は式ではない**（C-61）。前置演算子であり、専用の型を持つ。
//! **paradox は AST に現れない**——評価の結果であって、書けるものではない。

use crate::span::Span;

pub type NodeId = u32;

#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
    pub id: NodeId,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    // --- 一次式 ---
    Int(String),
    Float(String),
    /// `true` / `false`。**型は `u1` で確定している**——文脈を見ない（C-97）
    Bool(bool),
    Str(String),
    Name(String),

    /// `( E@0* )` — 領域を作る。スコープでも脱出段でもない。
    Paren(Vec<Expr>),
    /// `{ E@0* }` — 領域・スコープ・脱出段の三つを作る（裸のとき）。
    Block(Vec<Expr>),

    /// `new 型 ( 引数 )`
    Construct { ty: Type, args: CtorArgs },
    /// `[ E, … ]`
    ArrayLit(Vec<Expr>),
    /// `( K => V, … )`
    MapLit(Vec<(Expr, Expr)>),

    Decl(Decl),
    FnDecl(FnDecl),
    FlowDecl(FlowDecl),
    StructDecl(StructDecl),
    WrapDecl(WrapDecl),

    If(If),
    Loop(Box<Expr>),
    While { cond: Box<Expr>, body: Box<Expr> },
    NFor { name: String, start: Box<Expr>, count: Box<Expr>, body: Box<Expr> },
    Switch { subject: Box<Expr>, arms: Vec<Arm> },

    // --- 演算子 ---
    /// 前置。`-` `+` `!`
    Unary { op: UnOp, rhs: Box<Expr> },
    /// 中置。
    Binary { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr> },
    /// `;` — 左の領域を潰し、内面を空にする。**左辺は無くてもよい**（C-80）。
    Discard(Option<Box<Expr>>),
    /// `E -> T` — **領域に型を付ける**（C-30）。
    ///
    /// 「`:` は識別子に、`->` は領域に」の後半である。
    /// **リテラルは型が決まるまでソースの表現を保持する**ので、
    /// これがその型を決める道になる——`1 -> u1`、`300 -> u8`。
    Ascribe { expr: Box<Expr>, ty: Type },
    /// `a.m`
    Field { base: Box<Expr>, name: String },
    /// `a[i]`
    Index { base: Box<Expr>, index: Box<Expr> },
    /// `f(a, b)` / `a.m(b)`
    Call { callee: Box<Expr>, args: Vec<Expr> },
    /// `path := E` / `path += E` / `IDENT &= IDENT`
    Assign { op: AssignOp, lhs: Box<Expr>, rhs: Box<Expr> },

    /// 脱出。**式ではないが、式の位置に前置演算子として現れる。**
    Escape(Box<Escape>),
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum UnOp {
    Neg,
    Pos,
    Not,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum BinOp {
    Add, Sub, Mul, Div, Mod,
    Shl, Shr, BitAnd, BitXor, BitOr,
    Lt, Le, Gt, Ge, Eq, Ne,
    And, Or,
    /// `??` — paradox の除去子。**唯一の回復手段**（C-22）。
    Coalesce,
    /// `|>` — 構文の水準の糖衣（C-15）。
    Feed,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum AssignOp {
    /// `:=` セルに値を書く（深く複製する）
    Set,
    /// `&=` 名前が別のセルを指すようにする
    Alias,
    Add, Sub, Mul, Div, Mod, Shl, Shr, BitXor, BitOr,
}

// --- 宣言 ---

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BindKind {
    Var,
    Let,
    Const,
}

#[derive(Clone, Debug)]
pub struct Decl {
    pub kind: BindKind,
    pub bindings: Vec<Binding>,
}

#[derive(Clone, Debug)]
pub struct Binding {
    pub name: String,
    /// `alias` は型注釈の最も外側にのみ（C-48）。`&=` の側にしか現れない。
    pub ty: Option<Type>,
    pub is_alias: bool,
    /// `:=` なら値、`&=` なら指す先の名前。
    pub init: BindInit,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum BindInit {
    /// `:= E@6`
    Value(Expr),
    /// `&= IDENT` — 対象は名前だけ（C-53）
    AliasOf(String),
}

#[derive(Clone, Debug)]
pub struct FnDecl {
    /// `fn T.m` のときの `T`。**型の名前空間に入る**（S-1）。
    pub owner: Option<String>,
    pub name: String,
    pub params: Vec<Param>,
    pub body: Box<Expr>,
    /// `->` は**外界面の型**（C-66）。無ければ外界面は paradox のみ。
    pub ret: Option<Type>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Param {
    pub kind: BindKind,
    pub name: String,
    pub ty: Type,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct FlowDecl {
    /// `$` を含む（C-60）。
    pub name: String,
    /// 本体は**使用位置で読み直される**（C-15）。ここでは構文木を保持するだけ。
    pub body: Box<Escape>,
    pub span: Span,
}

/// `wrap 名前 = 型;`（S-2）。**包むのも剥がすのも `new`。**
#[derive(Clone, Debug)]
pub struct WrapDecl {
    pub name: String,
    pub base: Type,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct StructDecl {
    pub name: String,
    pub fields: Vec<Field>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Field {
    pub kind: BindKind,
    pub name: String,
    pub ty: Type,
    pub default: Option<Expr>,
    pub span: Span,
}

// --- 制御 ---

#[derive(Clone, Debug)]
pub struct If {
    /// `(cond, branch)` の列。最初が `if`、以降が `elif`。
    pub arms: Vec<(Expr, Expr)>,
    pub els: Option<Box<Expr>>,
}

#[derive(Clone, Debug)]
pub struct Arm {
    pub pattern: Expr,
    /// `E@6` — `;` を吸わない（C-82）。
    pub value: Expr,
    pub span: Span,
}

// --- 脱出 ---

#[derive(Clone, Debug)]
pub struct Escape {
    pub kind: EscapeKind,
    /// `break` だけが値を取れる。`continue` と `outward` は脱出しか取らない（C-71）。
    pub operand: Option<Operand>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum EscapeKind {
    /// 段を**終結**させる。
    Break { outward: bool },
    /// 段を**再開**させる。
    Continue,
    /// `$name` または `$name(args)`。使用位置で本体を読み直す。
    Flow { name: String, args: Vec<FlowArg> },
}

#[derive(Clone, Debug)]
pub enum Operand {
    /// `parse(8)` で読んだ式。
    Value(Expr),
    Escape(Box<Escape>),
}

#[derive(Clone, Debug)]
pub enum FlowArg {
    Escape(Escape),
    Value(Expr),
}

// --- 型 ---

/// 逆ポーランドで書かれた型。解決後の形。
#[derive(Clone, PartialEq, Debug)]
pub struct Type {
    pub value: ValueType,
    /// `alias` は修飾子ではなく**束縛の形態**（C-48）。最も外側にのみ。
    pub is_alias: bool,
    pub span: Span,
}

#[derive(Clone, PartialEq, Debug)]
pub enum ValueType {
    U1, U8, U16, U32, I32, I64, F32, F64,
    /// `u8 array` をラップした型（C-77）。
    Str,
    Array(Box<ValueType>),
    Map(Box<ValueType>, Box<ValueType>),
    /// 構造体・ラップ型。名前で参照する。
    Named(String),
}

#[derive(Clone, Debug)]
pub enum CtorArgs {
    /// 構造体：欄は名前で。
    Named(Vec<(String, Expr)>),
    /// 配列・写像：位置で。
    Positional(Vec<Expr>),
}

/// ホストが見せるもの（C-95 / S-11）。
///
/// **値だけでなく、呼べる名前も見せられる。**
///
/// ```text
/// count            値。読んで、書き戻す
/// tex_print("…")   呼べる名前。ホストが答える
/// ```
///
/// 呼べる名前を足したのは、**閉包が Vaak では表現できない**からである（S-11）——
/// コールバックは環境を捕まえるが、参照で捕まえれば C-48（値の中に別名は入らない）に、
/// 写しで捕まえれば C-33（値は深く複製される）に掛かる。
#[derive(Clone, PartialEq, Debug)]
pub enum HostItem {
    Value(ValueType),
    Fn(HostSig),
}

/// 呼べる名前の形。**ホストが宣言する**ので、検査器は無改造で働く。
#[derive(Clone, PartialEq, Debug)]
pub struct HostSig {
    pub params: Vec<ValueType>,
    /// `None` は**値を置かない**——呼び出しは paradox になる。
    pub ret: Option<ValueType>,
}

/// プログラム全体。最上位は領域でありスコープである。
#[derive(Clone, Debug)]
pub struct Program {
    pub body: Vec<Expr>,
}

/// 関数を引く鍵。メンバ関数は**型の名前空間に入る**（S-1）。
pub fn fn_key(f: &FnDecl) -> String {
    match &f.owner {
        Some(t) => format!("{t}.{}", f.name),
        None => f.name.clone(),
    }
}
