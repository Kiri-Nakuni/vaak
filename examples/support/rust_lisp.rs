#![forbid(unsafe_code)]

//! `examples/vaak/06-LISP.vaak` と同じ小さな式言語の Safe Rust 実装。
//!
//! `naive` は所有する構文木と `let` ごとの環境複製を使う。
//! `tuned` は入力を借用して名前を intern し、arena と push/pop 環境を使う。

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub offset: usize,
    pub message: String,
}

impl Error {
    fn new(offset: usize, message: impl Into<String>) -> Self {
        Self {
            offset,
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "byte {}: {}", self.offset, self.message)
    }
}

impl std::error::Error for Error {}

/// Vaak 版の主例と同じ入力。
#[allow(dead_code)]
pub const MAIN_PROGRAM: &str =
    "(let x 6 (do (let x 40 (+ x 2)) (if (= (* x 7) 42) (+ x 36) (/ 1 0))))";

/// 比較用の、`depth` 個の字句束縛を入れ子にした入力を作る。
///
/// `depth == 0` のときだけ定数 42、それ以外では最も内側の値を返す。
pub fn nested_let_source(depth: usize) -> (String, i64) {
    if depth == 0 {
        return ("42".to_owned(), 42);
    }

    let mut source = String::with_capacity(depth.saturating_mul(24));
    for index in 0..depth {
        use std::fmt::Write as _;
        write!(source, "(let x{index} {index} ").expect("String への書き込みは失敗しない");
    }
    use std::fmt::Write as _;
    write!(source, "x{}", depth - 1).expect("String への書き込みは失敗しない");
    for _ in 0..depth {
        source.push(')');
    }
    (source, (depth - 1) as i64)
}

fn parse_integer(word: &str) -> Option<i64> {
    let bytes = word.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let (negative, digits) = if bytes[0] == b'-' {
        if bytes.len() == 1 {
            return None;
        }
        (true, &bytes[1..])
    } else {
        (false, bytes)
    };
    if !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }

    let value = digits.iter().fold(0_i64, |value, digit| {
        value
            .wrapping_mul(10)
            .wrapping_add(i64::from(*digit - b'0'))
    });
    Some(if negative {
        0_i64.wrapping_sub(value)
    } else {
        value
    })
}

fn divide_euclidean(left: i64, right: i64, offset: usize) -> Result<i64, Error> {
    if right == 0 {
        return Err(Error::new(offset, "0 では割れない"));
    }

    // Vaak は i64 値を i128 上でユークリッド除算してから i64 へ戻す。
    // この形なら i64::MIN / -1 も panic せず、同じ折り返しになる。
    let left = i128::from(left);
    let right = i128::from(right);
    let mut quotient = left / right;
    let remainder = left % right;
    if remainder < 0 {
        if right > 0 {
            quotient -= 1;
        } else {
            quotient += 1;
        }
    }
    Ok(quotient as i64)
}

pub mod naive {
    use super::{divide_euclidean, parse_integer, Error};

    #[derive(Clone, Debug)]
    enum Token {
        Open(usize),
        Close(usize),
        Atom(String, usize),
    }

    impl Token {
        fn offset(&self) -> usize {
            match self {
                Self::Open(offset) | Self::Close(offset) | Self::Atom(_, offset) => *offset,
            }
        }
    }

    #[derive(Clone, Debug)]
    struct Expr {
        offset: usize,
        kind: ExprKind,
    }

    #[derive(Clone, Debug)]
    enum ExprKind {
        Integer(i64),
        Name(String),
        Add(Vec<Expr>),
        Multiply(Vec<Expr>),
        Subtract(Vec<Expr>),
        Divide(Vec<Expr>),
        Equal(Box<Expr>, Box<Expr>),
        Less(Box<Expr>, Box<Expr>),
        If(Box<Expr>, Box<Expr>, Box<Expr>),
        Let(String, Box<Expr>, Box<Expr>),
        Do(Vec<Expr>),
    }

    #[derive(Clone, Debug)]
    pub struct Program {
        root: Expr,
    }

    impl Program {
        pub fn eval(&self) -> Result<i64, Error> {
            eval_expr(&self.root, &[])
        }
    }

    pub fn parse(source: &str) -> Result<Program, Error> {
        let tokens = tokenize(source);
        if tokens.is_empty() {
            return Err(Error::new(0, "式がない"));
        }
        let (root, next) = parse_expr(&tokens, 0)?;
        if next != tokens.len() {
            return Err(Error::new(
                tokens[next].offset(),
                "式の後ろに余分な入力がある",
            ));
        }
        Ok(Program { root })
    }

    fn tokenize(source: &str) -> Vec<Token> {
        let bytes = source.as_bytes();
        let mut tokens = Vec::with_capacity(source.len() / 3);
        let mut cursor = 0;
        while cursor < bytes.len() {
            match bytes[cursor] {
                byte if byte.is_ascii_whitespace() => cursor += 1,
                b'(' => {
                    tokens.push(Token::Open(cursor));
                    cursor += 1;
                }
                b')' => {
                    tokens.push(Token::Close(cursor));
                    cursor += 1;
                }
                _ => {
                    let start = cursor;
                    while cursor < bytes.len()
                        && !bytes[cursor].is_ascii_whitespace()
                        && bytes[cursor] != b'('
                        && bytes[cursor] != b')'
                    {
                        cursor += 1;
                    }
                    tokens.push(Token::Atom(source[start..cursor].to_owned(), start));
                }
            }
        }
        tokens
    }

    fn parse_expr(tokens: &[Token], position: usize) -> Result<(Expr, usize), Error> {
        let token = tokens.get(position).ok_or_else(|| {
            Error::new(
                tokens.last().map_or(0, Token::offset),
                "式の途中で入力が終わった",
            )
        })?;
        match token {
            Token::Close(offset) => Err(Error::new(*offset, "対応する `(` のない `)`")),
            Token::Atom(word, offset) => {
                let kind = parse_integer(word)
                    .map(ExprKind::Integer)
                    .unwrap_or_else(|| ExprKind::Name(word.clone()));
                Ok((
                    Expr {
                        offset: *offset,
                        kind,
                    },
                    position + 1,
                ))
            }
            Token::Open(offset) => parse_list(tokens, position + 1, *offset),
        }
    }

    fn parse_list(
        tokens: &[Token],
        mut position: usize,
        offset: usize,
    ) -> Result<(Expr, usize), Error> {
        let Some(Token::Atom(operator, operator_offset)) = tokens.get(position) else {
            return Err(Error::new(offset, "`(` の直後には演算子が要る"));
        };
        position += 1;

        let kind = match operator.as_str() {
            "+" => {
                let (arguments, next) = parse_many(tokens, position, offset)?;
                position = next;
                ExprKind::Add(arguments)
            }
            "*" => {
                let (arguments, next) = parse_many(tokens, position, offset)?;
                position = next;
                ExprKind::Multiply(arguments)
            }
            "-" | "/" => {
                let (arguments, next) = parse_many(tokens, position, offset)?;
                if arguments.is_empty() {
                    return Err(Error::new(*operator_offset, "`-` と `/` には引数が要る"));
                }
                position = next;
                if operator == "-" {
                    ExprKind::Subtract(arguments)
                } else {
                    ExprKind::Divide(arguments)
                }
            }
            "=" | "<" => {
                let (left, next) = parse_expr(tokens, position)?;
                let (right, next) = parse_expr(tokens, next)?;
                position = expect_close(tokens, next, offset)?;
                if operator == "=" {
                    ExprKind::Equal(Box::new(left), Box::new(right))
                } else {
                    ExprKind::Less(Box::new(left), Box::new(right))
                }
            }
            "if" => {
                let (condition, next) = parse_expr(tokens, position)?;
                let (yes, next) = parse_expr(tokens, next)?;
                let (no, next) = parse_expr(tokens, next)?;
                position = expect_close(tokens, next, offset)?;
                ExprKind::If(Box::new(condition), Box::new(yes), Box::new(no))
            }
            "let" => {
                let Some(Token::Atom(name, _)) = tokens.get(position) else {
                    return Err(Error::new(offset, "`let` の束縛名がない"));
                };
                let name = name.clone();
                let (bound, next) = parse_expr(tokens, position + 1)?;
                let (body, next) = parse_expr(tokens, next)?;
                position = expect_close(tokens, next, offset)?;
                ExprKind::Let(name, Box::new(bound), Box::new(body))
            }
            "do" => {
                let (expressions, next) = parse_many(tokens, position, offset)?;
                position = next;
                ExprKind::Do(expressions)
            }
            _ => {
                return Err(Error::new(
                    *operator_offset,
                    format!("未知の演算子 `{operator}`"),
                ))
            }
        };

        Ok((Expr { offset, kind }, position))
    }

    fn parse_many(
        tokens: &[Token],
        mut position: usize,
        opening_offset: usize,
    ) -> Result<(Vec<Expr>, usize), Error> {
        let mut expressions = Vec::new();
        loop {
            match tokens.get(position) {
                Some(Token::Close(_)) => return Ok((expressions, position + 1)),
                Some(_) => {
                    let (expression, next) = parse_expr(tokens, position)?;
                    expressions.push(expression);
                    position = next;
                }
                None => return Err(Error::new(opening_offset, "`)` がない")),
            }
        }
    }

    fn expect_close(
        tokens: &[Token],
        position: usize,
        opening_offset: usize,
    ) -> Result<usize, Error> {
        match tokens.get(position) {
            Some(Token::Close(_)) => Ok(position + 1),
            Some(token) => Err(Error::new(token.offset(), "引数が多すぎる")),
            None => Err(Error::new(opening_offset, "`)` がない")),
        }
    }

    fn eval_expr(expression: &Expr, environment: &[(String, i64)]) -> Result<i64, Error> {
        match &expression.kind {
            ExprKind::Integer(value) => Ok(*value),
            ExprKind::Name(name) => environment
                .iter()
                .rev()
                .find_map(|(candidate, value)| (candidate == name).then_some(*value))
                .ok_or_else(|| Error::new(expression.offset, format!("未知の名前 `{name}`"))),
            ExprKind::Add(arguments) => arguments.iter().try_fold(0_i64, |total, argument| {
                Ok(total.wrapping_add(eval_expr(argument, environment)?))
            }),
            ExprKind::Multiply(arguments) => arguments.iter().try_fold(1_i64, |total, argument| {
                Ok(total.wrapping_mul(eval_expr(argument, environment)?))
            }),
            ExprKind::Subtract(arguments) => {
                let (first, rest) = arguments.split_first().expect("parser が一引数以上にする");
                let first = eval_expr(first, environment)?;
                if rest.is_empty() {
                    return Ok(0_i64.wrapping_sub(first));
                }
                rest.iter().try_fold(first, |total, argument| {
                    Ok(total.wrapping_sub(eval_expr(argument, environment)?))
                })
            }
            ExprKind::Divide(arguments) => {
                let (first, rest) = arguments.split_first().expect("parser が一引数以上にする");
                let first = eval_expr(first, environment)?;
                rest.iter().try_fold(first, |total, argument| {
                    divide_euclidean(total, eval_expr(argument, environment)?, expression.offset)
                })
            }
            ExprKind::Equal(left, right) => Ok(i64::from(
                eval_expr(left, environment)? == eval_expr(right, environment)?,
            )),
            ExprKind::Less(left, right) => Ok(i64::from(
                eval_expr(left, environment)? < eval_expr(right, environment)?,
            )),
            ExprKind::If(condition, yes, no) => {
                if eval_expr(condition, environment)? != 0 {
                    eval_expr(yes, environment)
                } else {
                    eval_expr(no, environment)
                }
            }
            ExprKind::Let(name, bound, body) => {
                let value = eval_expr(bound, environment)?;
                let mut inner = environment.to_vec();
                inner.push((name.clone(), value));
                eval_expr(body, &inner)
            }
            ExprKind::Do(expressions) => {
                let mut value = 0;
                for expression in expressions {
                    value = eval_expr(expression, environment)?;
                }
                Ok(value)
            }
        }
    }
}

pub mod tuned {
    use std::collections::HashMap;

    use super::{divide_euclidean, parse_integer, Error};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct NodeId(u32);

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Symbol(u32);

    #[derive(Clone, Copy, Debug)]
    struct ChildList {
        first: Option<u32>,
        last: Option<u32>,
        len: u32,
        max_bindings: usize,
    }

    impl ChildList {
        const fn new() -> Self {
            Self {
                first: None,
                last: None,
                len: 0,
                max_bindings: 0,
            }
        }
    }

    #[derive(Clone, Copy, Debug)]
    struct ChildLink {
        node: NodeId,
        next: Option<u32>,
    }

    #[derive(Clone, Copy, Debug)]
    struct Node {
        offset: usize,
        max_bindings: usize,
        kind: NodeKind,
    }

    #[derive(Clone, Copy, Debug)]
    enum NodeKind {
        Integer(i64),
        Name(Symbol),
        Add(ChildList),
        Multiply(ChildList),
        Subtract(ChildList),
        Divide(ChildList),
        Equal(NodeId, NodeId),
        Less(NodeId, NodeId),
        If(NodeId, NodeId, NodeId),
        Let(Symbol, NodeId, NodeId),
        Do(ChildList),
    }

    #[derive(Clone, Copy, Debug)]
    enum TokenKind<'source> {
        Open,
        Close,
        Atom(&'source str),
    }

    #[derive(Clone, Copy, Debug)]
    struct Token<'source> {
        offset: usize,
        kind: TokenKind<'source>,
    }

    struct Lexer<'source> {
        source: &'source str,
        cursor: usize,
    }

    impl<'source> Lexer<'source> {
        fn new(source: &'source str) -> Self {
            Self { source, cursor: 0 }
        }

        fn next(&mut self) -> Option<Token<'source>> {
            let bytes = self.source.as_bytes();
            while self.cursor < bytes.len() && bytes[self.cursor].is_ascii_whitespace() {
                self.cursor += 1;
            }
            if self.cursor == bytes.len() {
                return None;
            }

            let offset = self.cursor;
            match bytes[self.cursor] {
                b'(' => {
                    self.cursor += 1;
                    Some(Token {
                        offset,
                        kind: TokenKind::Open,
                    })
                }
                b')' => {
                    self.cursor += 1;
                    Some(Token {
                        offset,
                        kind: TokenKind::Close,
                    })
                }
                _ => {
                    while self.cursor < bytes.len()
                        && !bytes[self.cursor].is_ascii_whitespace()
                        && bytes[self.cursor] != b'('
                        && bytes[self.cursor] != b')'
                    {
                        self.cursor += 1;
                    }
                    Some(Token {
                        offset,
                        kind: TokenKind::Atom(&self.source[offset..self.cursor]),
                    })
                }
            }
        }
    }

    struct Parser<'source> {
        lexer: Lexer<'source>,
        lookahead: Option<Option<Token<'source>>>,
        nodes: Vec<Node>,
        links: Vec<ChildLink>,
        symbols: Vec<&'source str>,
        symbol_ids: HashMap<&'source str, Symbol>,
    }

    impl<'source> Parser<'source> {
        fn new(source: &'source str) -> Self {
            let estimated_nodes = (source.len() / 8).max(1);
            Self {
                lexer: Lexer::new(source),
                lookahead: None,
                nodes: Vec::with_capacity(estimated_nodes),
                links: Vec::with_capacity(estimated_nodes),
                symbols: Vec::with_capacity(estimated_nodes / 4),
                symbol_ids: HashMap::with_capacity(estimated_nodes / 4),
            }
        }

        fn peek(&mut self) -> Option<Token<'source>> {
            if self.lookahead.is_none() {
                self.lookahead = Some(self.lexer.next());
            }
            self.lookahead.expect("直前に埋めた")
        }

        fn take(&mut self) -> Option<Token<'source>> {
            self.lookahead.take().unwrap_or_else(|| self.lexer.next())
        }

        fn intern(&mut self, name: &'source str) -> Result<Symbol, Error> {
            if let Some(symbol) = self.symbol_ids.get(name) {
                return Ok(*symbol);
            }
            let raw =
                u32::try_from(self.symbols.len()).map_err(|_| Error::new(0, "名前が多すぎる"))?;
            let symbol = Symbol(raw);
            self.symbols.push(name);
            self.symbol_ids.insert(name, symbol);
            Ok(symbol)
        }

        fn push_node(
            &mut self,
            offset: usize,
            kind: NodeKind,
            max_bindings: usize,
        ) -> Result<NodeId, Error> {
            let raw = u32::try_from(self.nodes.len())
                .map_err(|_| Error::new(offset, "構文木が大きすぎる"))?;
            self.nodes.push(Node {
                offset,
                max_bindings,
                kind,
            });
            Ok(NodeId(raw))
        }

        fn append_child(
            &mut self,
            list: &mut ChildList,
            node: NodeId,
            offset: usize,
        ) -> Result<(), Error> {
            let raw = u32::try_from(self.links.len())
                .map_err(|_| Error::new(offset, "子ノードが多すぎる"))?;
            self.links.push(ChildLink { node, next: None });
            if let Some(last) = list.last {
                self.links[last as usize].next = Some(raw);
            } else {
                list.first = Some(raw);
            }
            list.last = Some(raw);
            list.len += 1;
            list.max_bindings = list
                .max_bindings
                .max(self.nodes[node.0 as usize].max_bindings);
            Ok(())
        }

        fn parse_expression(&mut self) -> Result<NodeId, Error> {
            let token = self
                .take()
                .ok_or_else(|| Error::new(self.lexer.cursor, "式の途中で入力が終わった"))?;
            match token.kind {
                TokenKind::Close => Err(Error::new(token.offset, "対応する `(` のない `)`")),
                TokenKind::Atom(word) => {
                    if let Some(value) = parse_integer(word) {
                        self.push_node(token.offset, NodeKind::Integer(value), 0)
                    } else {
                        let symbol = self.intern(word)?;
                        self.push_node(token.offset, NodeKind::Name(symbol), 0)
                    }
                }
                TokenKind::Open => self.parse_list(token.offset),
            }
        }

        fn parse_list(&mut self, opening_offset: usize) -> Result<NodeId, Error> {
            let operator = self
                .take()
                .ok_or_else(|| Error::new(opening_offset, "`(` の直後には演算子が要る"))?;
            let TokenKind::Atom(operator_name) = operator.kind else {
                return Err(Error::new(operator.offset, "`(` の直後には演算子が要る"));
            };

            match operator_name {
                "+" => {
                    let children = self.parse_many(opening_offset)?;
                    self.push_node(
                        opening_offset,
                        NodeKind::Add(children),
                        children.max_bindings,
                    )
                }
                "*" => {
                    let children = self.parse_many(opening_offset)?;
                    self.push_node(
                        opening_offset,
                        NodeKind::Multiply(children),
                        children.max_bindings,
                    )
                }
                "-" | "/" => {
                    let children = self.parse_many(opening_offset)?;
                    if children.len == 0 {
                        return Err(Error::new(operator.offset, "`-` と `/` には引数が要る"));
                    }
                    let kind = if operator_name == "-" {
                        NodeKind::Subtract(children)
                    } else {
                        NodeKind::Divide(children)
                    };
                    self.push_node(opening_offset, kind, children.max_bindings)
                }
                "=" | "<" => {
                    let left = self.parse_expression()?;
                    let right = self.parse_expression()?;
                    self.expect_close(opening_offset)?;
                    let max_bindings = self.nodes[left.0 as usize]
                        .max_bindings
                        .max(self.nodes[right.0 as usize].max_bindings);
                    let kind = if operator_name == "=" {
                        NodeKind::Equal(left, right)
                    } else {
                        NodeKind::Less(left, right)
                    };
                    self.push_node(opening_offset, kind, max_bindings)
                }
                "if" => {
                    let condition = self.parse_expression()?;
                    let yes = self.parse_expression()?;
                    let no = self.parse_expression()?;
                    self.expect_close(opening_offset)?;
                    let max_bindings = [condition, yes, no]
                        .into_iter()
                        .map(|node| self.nodes[node.0 as usize].max_bindings)
                        .max()
                        .unwrap_or(0);
                    self.push_node(
                        opening_offset,
                        NodeKind::If(condition, yes, no),
                        max_bindings,
                    )
                }
                "let" => {
                    let name = self
                        .take()
                        .ok_or_else(|| Error::new(opening_offset, "`let` の束縛名がない"))?;
                    let TokenKind::Atom(name) = name.kind else {
                        return Err(Error::new(name.offset, "`let` の束縛名がない"));
                    };
                    let name = self.intern(name)?;
                    let bound = self.parse_expression()?;
                    let body = self.parse_expression()?;
                    self.expect_close(opening_offset)?;
                    let max_bindings = self.nodes[bound.0 as usize]
                        .max_bindings
                        .max(self.nodes[body.0 as usize].max_bindings.saturating_add(1));
                    self.push_node(
                        opening_offset,
                        NodeKind::Let(name, bound, body),
                        max_bindings,
                    )
                }
                "do" => {
                    let children = self.parse_many(opening_offset)?;
                    self.push_node(
                        opening_offset,
                        NodeKind::Do(children),
                        children.max_bindings,
                    )
                }
                _ => Err(Error::new(
                    operator.offset,
                    format!("未知の演算子 `{operator_name}`"),
                )),
            }
        }

        fn parse_many(&mut self, opening_offset: usize) -> Result<ChildList, Error> {
            let mut children = ChildList::new();
            loop {
                match self.peek() {
                    Some(Token {
                        kind: TokenKind::Close,
                        ..
                    }) => {
                        self.take();
                        return Ok(children);
                    }
                    Some(_) => {
                        let child = self.parse_expression()?;
                        self.append_child(&mut children, child, opening_offset)?;
                    }
                    None => return Err(Error::new(opening_offset, "`)` がない")),
                }
            }
        }

        fn expect_close(&mut self, opening_offset: usize) -> Result<(), Error> {
            match self.take() {
                Some(Token {
                    kind: TokenKind::Close,
                    ..
                }) => Ok(()),
                Some(token) => Err(Error::new(token.offset, "引数が多すぎる")),
                None => Err(Error::new(opening_offset, "`)` がない")),
            }
        }
    }

    #[derive(Clone, Debug)]
    pub struct Program<'source> {
        nodes: Vec<Node>,
        links: Vec<ChildLink>,
        symbols: Vec<&'source str>,
        root: NodeId,
        max_bindings: usize,
    }

    impl Program<'_> {
        pub fn eval(&self) -> Result<i64, Error> {
            let mut evaluator = Evaluator::new(self);
            evaluator.eval(self)
        }

        pub fn evaluator(&self) -> Evaluator {
            Evaluator::new(self)
        }

        fn eval_node(
            &self,
            node: NodeId,
            environment: &mut Vec<(Symbol, i64)>,
        ) -> Result<i64, Error> {
            let node = self.nodes[node.0 as usize];
            match node.kind {
                NodeKind::Integer(value) => Ok(value),
                NodeKind::Name(symbol) => environment
                    .iter()
                    .rev()
                    .find_map(|(candidate, value)| (*candidate == symbol).then_some(*value))
                    .ok_or_else(|| {
                        Error::new(
                            node.offset,
                            format!("未知の名前 `{}`", self.symbols[symbol.0 as usize]),
                        )
                    }),
                NodeKind::Add(children) => {
                    let mut total = 0_i64;
                    self.for_each_child(children, |child| {
                        total = total.wrapping_add(self.eval_node(child, environment)?);
                        Ok(())
                    })?;
                    Ok(total)
                }
                NodeKind::Multiply(children) => {
                    let mut total = 1_i64;
                    self.for_each_child(children, |child| {
                        total = total.wrapping_mul(self.eval_node(child, environment)?);
                        Ok(())
                    })?;
                    Ok(total)
                }
                NodeKind::Subtract(children) => {
                    let mut cursor = children.first.expect("parser が一引数以上にする");
                    let first = self.eval_node(self.links[cursor as usize].node, environment)?;
                    let mut total = first;
                    let mut count = 1;
                    while let Some(next) = self.links[cursor as usize].next {
                        cursor = next;
                        total = total.wrapping_sub(
                            self.eval_node(self.links[cursor as usize].node, environment)?,
                        );
                        count += 1;
                    }
                    Ok(if count == 1 {
                        0_i64.wrapping_sub(total)
                    } else {
                        total
                    })
                }
                NodeKind::Divide(children) => {
                    let mut cursor = children.first.expect("parser が一引数以上にする");
                    let mut total =
                        self.eval_node(self.links[cursor as usize].node, environment)?;
                    while let Some(next) = self.links[cursor as usize].next {
                        cursor = next;
                        total = divide_euclidean(
                            total,
                            self.eval_node(self.links[cursor as usize].node, environment)?,
                            node.offset,
                        )?;
                    }
                    Ok(total)
                }
                NodeKind::Equal(left, right) => Ok(i64::from(
                    self.eval_node(left, environment)? == self.eval_node(right, environment)?,
                )),
                NodeKind::Less(left, right) => Ok(i64::from(
                    self.eval_node(left, environment)? < self.eval_node(right, environment)?,
                )),
                NodeKind::If(condition, yes, no) => {
                    if self.eval_node(condition, environment)? != 0 {
                        self.eval_node(yes, environment)
                    } else {
                        self.eval_node(no, environment)
                    }
                }
                NodeKind::Let(name, bound, body) => {
                    let value = self.eval_node(bound, environment)?;
                    environment.push((name, value));
                    let result = self.eval_node(body, environment);
                    environment.pop();
                    result
                }
                NodeKind::Do(children) => {
                    let mut value = 0;
                    self.for_each_child(children, |child| {
                        value = self.eval_node(child, environment)?;
                        Ok(())
                    })?;
                    Ok(value)
                }
            }
        }

        fn for_each_child(
            &self,
            children: ChildList,
            mut action: impl FnMut(NodeId) -> Result<(), Error>,
        ) -> Result<(), Error> {
            let mut link = children.first;
            while let Some(index) = link {
                let child = self.links[index as usize];
                action(child.node)?;
                link = child.next;
            }
            Ok(())
        }
    }

    pub struct Evaluator {
        environment: Vec<(Symbol, i64)>,
    }

    impl Evaluator {
        pub fn new(program: &Program<'_>) -> Self {
            Self {
                environment: Vec::with_capacity(program.max_bindings),
            }
        }

        pub fn eval(&mut self, program: &Program<'_>) -> Result<i64, Error> {
            self.environment.clear();
            program.eval_node(program.root, &mut self.environment)
        }
    }

    pub fn parse(source: &str) -> Result<Program<'_>, Error> {
        let mut parser = Parser::new(source);
        if parser.peek().is_none() {
            return Err(Error::new(0, "式がない"));
        }
        let root = parser.parse_expression()?;
        if let Some(token) = parser.take() {
            return Err(Error::new(token.offset, "式の後ろに余分な入力がある"));
        }
        let max_bindings = parser.nodes[root.0 as usize].max_bindings;
        Ok(Program {
            nodes: parser.nodes,
            links: parser.links,
            symbols: parser.symbols,
            root,
            max_bindings,
        })
    }
}
