//! Vaak — ホストに埋め込む小さなインタプリタ言語。
//!
//! 設計の決定は `docs/vaak/decisions.md`（C-1 〜 C-90）にある。
//! 構文は `docs/vaak/16-形式構文.md`。束縛力表が一次仕様であり、BNF は従属する。

pub mod ast;
pub mod lexer;
pub mod check;
pub mod host;
pub mod interp;
pub mod parser;
pub mod span;
pub mod types;
pub mod value;
pub mod vm;
