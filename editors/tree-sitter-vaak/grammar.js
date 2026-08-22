/**
 * Vaak のハイライト用 tree-sitter 文法。
 *
 * これは構文解析器の参照実装ではない。式の束縛力、領域、脱出段などの意味は
 * `src/parser.rs` 以降だけが持つ（S-5）。ここではエディタが色を付けるために、
 * `src/lexer.rs` が作る字句の種類だけを分類する。
 */

module.exports = grammar({
  name: 'vaak',

  word: $ => $.identifier,

  externals: $ => [$.line_comment, $.block_comment],

  extras: _ => [/\s/],

  rules: {
    source_file: $ => repeat($._lexeme),

    _lexeme: $ => choice(
      $.keyword,
      $.boolean,
      $.type_name,
      $.flow_name,
      $.string,
      $.float,
      $.integer,
      $.operator,
      $.punctuation,
      $.line_comment,
      $.block_comment,
      $.identifier,
    ),

    // `tests/grammar.rs` が `src/lexer.rs` の表と突き合わせる。
    keyword: $ => choice(
      'var',
      'let',
      'const',
      'fn',
      'flow',
      'struct',
      'wrap',
      'new',
      'if',
      'elif',
      'else',
      'fi',
      'loop',
      'while',
      'nfor',
      'switch',
      'case',
      'break',
      'continue',
      'outward',
      'mod',
      'array',
      'map',
      'hash',
      'alias',
    ),

    boolean: $ => choice(
      'true',
      'false',
    ),

    // 組み込み型は字句上は識別子。`src/parser.rs` の型表と試験で揃える。
    type_name: $ => choice(
      'u1',
      'bool',
      'u8',
      'u16',
      'u32',
      'i32',
      'i64',
      'f32',
      'f64',
      'f80',
      'str',
    ),

    flow_name: _ => token(seq('$', /[_\p{L}][_\p{L}\p{N}]*/u)),

    string: $ => seq(
      '"',
      repeat(choice(
        $.escape_sequence,
        token.immediate(/[^"\\]+/),
      )),
      '"',
    ),

    escape_sequence: _ => token.immediate(choice(
      /\\[nt\\"0]/,
      /\\x[0-9a-fA-F]{2}/,
      /\\u\{[0-9a-fA-F]+\}/,
    )),

    float: _ => token(prec(2, choice(
      /[0-9][0-9_]*\.[0-9][0-9_]*([eE][+-]?[0-9][0-9_]*)?/,
      /[0-9][0-9_]*[eE][+-]?[0-9][0-9_]*/,
    ))),

    integer: _ => token(prec(1, choice(
      /0[xX][0-9a-fA-F_]+/,
      /0[bB][01_]+/,
      /0[oO][0-7_]+/,
      /[0-9][0-9_]*/,
    ))),

    operator: _ => choice(
      '??',
      '<<=',
      '>>=',
      'mod=',
      '->',
      '=>',
      '&&',
      '||',
      '|>',
      ':=',
      '&=',
      '+=',
      '-=',
      '*=',
      '/=',
      '^=',
      '|=',
      '<<',
      '>>',
      '<=',
      '>=',
      '==',
      '!=',
      '+',
      '-',
      '*',
      '/',
      '!',
      '&',
      '^',
      '|',
      '<',
      '>',
      '=',
      '.',
    ),

    punctuation: _ => choice(
      '(',
      ')',
      '{',
      '}',
      '[',
      ']',
      ',',
      ':',
      ';',
    ),

    identifier: _ => /[_\p{L}][_\p{L}\p{N}]*/u,
  },
});
