/**
 * PraTeX 埋め込みスクリプト言語の tree-sitter 文法。
 *
 * ハイライト用なので、インタプリタ本体（読みながら実行するカーソルマシン）とは
 * 別に「全体を構文解析できる部分集合」を定義している。goto で読み飛ばされる
 * 区間が構文的に壊れていてもよい、という言語の性質は tree-sitter では表現
 * できないので、その区間は ERROR ノードになる——エディタ上での見た目の問題に
 * とどまり、実行には影響しない。
 */

const PREC = {
  or: 1,
  and: 2,
  cmp: 3,
  add: 4,
  mul: 5,
  unary: 6,
  ascription: 7,
  call: 8,
};

module.exports = grammar({
  name: 'reod',

  word: ($) => $.identifier,

  // 改行終端（src/scanner.c）。アトリビュートと返り値型がこれで切れる。
  externals: ($) => [$._line_end, $.block_comment],

  // `return 42;` のような作用素別名の適用は、先頭が識別子なので式文と
  // 見分けが1トークンではつかない。GLR に両方試させる。
  // `return (x);` は「呼び出しの引数列」とも「括弧つき式」とも読めるので
  // GLR に両方試させる。
  conflicts: ($) => [[$.argument_list, $.parenthesized_expression]],

  extras: ($) => [/\s/, $.comment],

  rules: {
    source_file: ($) => repeat($._statement),

    comment: (_) => token(seq('%', /[^\n]*/)),

    // D-18r: `#[global, immediate]` のようにリストで書ける。行末で終端し、
    // **次の行**の評価方法を変更する。同一行に文を続けることはできない。
    attribute: ($) =>
      seq(
        '#',
        '[',
        alias($.identifier, $.attribute_name),
        repeat(seq(',', alias($.identifier, $.attribute_name))),
        ']',
        $._line_end,
      ),

    // ---- 文 ----

    _statement: ($) =>
      choice(
        $.type_definition,
        $.function_definition,
        $.let_binding,
        $.assignment,
        $.if_expression,
        $.loop_expression,
        $.while_statement,
        $.nfor_expression,
        $.ifor_statement,
        $.switch_expression,
        $.break_statement,
        $.continue_statement,
        $.flow_binding,
        $.operator_application,
        $.goto_statement,
        $.label_statement,
        $.comefrom_statement,
        $.block,
        $.block_comment,
        $.expression_statement,
        ';',
      ),

    type_definition: ($) =>
      seq('typ', field('name', $.type_identifier), '{', repeat($.field_declaration), '}', ';'),

    field_declaration: ($) =>
      seq(field('name', $.identifier), ':', field('type', $.type), ';'),

    // D-27: 関数定義は `fn`。`let`（値の束縛）とは束縛種も終端規則も第一級性も違う。
    // D-21r: 改行で終端する。`;` は要らない。
    function_definition: ($) =>
      seq(
        repeat($.attribute),
        'fn',
        field('name', $.identifier),
        field('parameters', $.parameter_list),
        field('body', $.block),
        optional(seq('->', field('return_type', $.type))),
        $._line_end,
        ';',
      ),

    parameter_list: ($) =>
      seq('(', optional(seq($.parameter, repeat(seq(',', $.parameter)), optional(','))), ')'),

    parameter: ($) =>
      seq(field('name', $.identifier), optional(seq(':', field('type', $.type)))),

    let_binding: ($) =>
      seq(
        repeat($.attribute),
        'let',
        field('name', $.identifier),
        field('operator', $._bind_operator),
        field('value', $._expression),
        ';',
      ),

    assignment: ($) =>
      seq(
        repeat($.attribute),
        field('target', $._lvalue),
        field('operator', choice($._bind_operator, '+=', '-=', '*=', '/=')),
        field('value', $._expression),
        ';',
      ),

    _lvalue: ($) => choice($.identifier, $.index_expression, $.field_expression),

    // §4.6: := 新 thunk / &= 共有 / ^= 未評価の計算の複製
    _bind_operator: (_) => choice(':=', '&=', '^='),

    // D-34 / D-38: 作用素式。`break` を重ねた回数だけブロックを抜け、終端が
    // 外側の文脈で起きることを指定する。
    break_statement: ($) =>
      seq(repeat1($._operator), optional(choice($._expression, $.control_terminal)), ';'),

    // 終端は式または制御文。`comefrom` だけは実行が続く。
    control_terminal: ($) =>
      choice(
        'continue',
        seq('goto', field('label', $.label_identifier)),
        seq('comefrom', field('label', $.label_identifier)),
        seq('label', field('label', $.label_identifier)),
      ),

    _operator: ($) => choice('break', $.operator_repeat),

    // D-38: `$` を読んだ時点で作用素の評価規則へ入る。入ったあとは印を重ねない。
    // `$frame { … }` — 呼び出しを伴わない depth 基準
    frame_expression: ($) => seq('$', 'frame', field('body', $.block)),

    operator_repeat: ($) =>
      seq(
        '$',
        'repeat',
        '(',
        field('operator', choice($._operator, $.identifier)),
        ',',
        field('depth', $._expression),
        ')',
      ),

    // D-39: 作用素式に別名を与える。`let`（値）・`fn`（関数）と並ぶ第3の束縛種で、
    // 本体は使用のたびに使用位置で読み直される。
    // D-42: `:=` ではなく `=`。束縛演算子（`:=` / `&=` / `^=`）は thunk の
    // 作り方を区別するためのもので、`flow` は thunk を作らない。
    flow_binding: ($) =>
      seq(
        'flow',
        field('name', $.identifier),
        '=',
        field('value', seq(repeat1($._operator), optional($.control_terminal))),
        ';',
      ),

    // 別名の適用。`return 42;` のような形。他のどの文にも当てはまらないときだけ。
    // 終端のない `return;` は式文と字句的に区別できないのでここでは扱わない
    // （ハイライトは highlights.scm の既知名リストが拾う）。
    operator_application: ($) =>
      prec(
        -1,
        seq(
          field('operator', $.identifier),
          choice($._expression, $.control_terminal),
          ';',
        ),
      ),

    // D-29: jump だがブロックを出ないので巻き戻しは起きない。
    continue_statement: (_) => seq('continue', ';'),

    goto_statement: ($) => seq('goto', field('label', $.label_identifier), ';'),
    label_statement: ($) => seq('label', field('label', $.label_identifier), ';'),
    comefrom_statement: ($) =>
      seq('comefrom', field('label', $.label_identifier), ';'),

    // ブロックは末尾式を持てる（関数の返り値、§3.4）。
    // 末尾がブロックを持つ式（`if ... else ...`）の場合、それを「末尾式」と読むか
    // 「文」と読むかは構文だけでは決まらない——インタプリタは「残りがちょうど1つの
    // 式として読み切れるか」で判定する（interp.rs の eval_block_value）。
    // tree-sitter では文として読む側に倒す。ハイライトには影響しない。
    block: ($) => seq('{', repeat($._statement), optional($._expression_no_block), '}'),

    // ブロックを持つ式（if / loop）は、文の位置では `;` を要求しない。
    // Rust の ExpressionWithBlock と同じ切り分け。
    expression_statement: ($) => seq($._expression_no_block, ';'),

    // ---- 式 ----

    _expression: ($) =>
      choice(
        $._expression_no_block,
        $.if_expression,
        $.loop_expression,
        $.nfor_expression,
        $.switch_expression,
      ),

    _expression_no_block: ($) =>
      choice(
        $.integer,
        $.string,
        $.identifier,
        $.call_expression,
        $.type_ascription,
        $.index_expression,
        $.field_expression,
        $.range_expression,
        $.unary_expression,
        $.binary_expression,
        $.parenthesized_expression,
        $.frame_expression,
      ),

    if_expression: ($) =>
      prec.right(
        seq(
          'if',
          field('condition', $._expression),
          field('consequence', $._statement),
          optional(seq('else', field('alternative', $._statement))),
        ),
      ),

    loop_expression: ($) => seq('loop', field('body', $.block)),

    while_statement: ($) =>
      seq('while', field('condition', $._expression), field('body', $.block)),

    // §3.5: 数値範囲を反復する式。D-9 で返り値型は後置 `->`。
    nfor_expression: ($) =>
      seq(
        'nfor',
        '(',
        field('binder', $.identifier),
        'in',
        field('range', $._expression),
        ')',
        field('body', $.block),
        optional(seq('->', field('return_type', $.type))),
      ),

    // §3.5: イテレータを消費する文。comp 節は break されずに完了した場合に走る。
    ifor_statement: ($) =>
      seq(
        'ifor',
        '(',
        field('binder', $.identifier),
        'in',
        field('iterator', $._expression),
        ')',
        field('body', $.block),
        optional(seq('comp', field('complete', $.block))),
      ),

    // D-32: `match` ではなく `switch`。ADT を持たないので、判定できるのは数値など
    // 言語が直接比較できる値だけであり、パターンマッチではない。`match` という
    // 名前は存在しない destructuring を示唆してしまう（PHECL 3）。
    switch_expression: ($) =>
      seq('switch', field('value', $._expression), '{', repeat($.switch_arm), '}'),

    switch_arm: ($) =>
      seq(field('pattern', choice($.integer, $.wildcard)), '=>', $._statement),

    wildcard: (_) => '_',

    call_expression: ($) =>
      prec(
        PREC.call,
        seq(field('function', choice($.identifier, $.field_expression)), field('arguments', $.argument_list)),
      ),

    argument_list: ($) =>
      seq('(', optional(seq($._expression, repeat(seq(',', $._expression)), optional(','))), ')'),

    // D-8: 呼出側の型アノテーション。宣言側の `->` と同じ意味（返り値型を名指す）
    type_ascription: ($) =>
      prec.left(PREC.ascription, seq($.call_expression, '->', field('type', $.type))),

    index_expression: ($) =>
      prec(PREC.call, seq(field('object', $._expression_no_block), '[', field('index', $._expression), ']')),

    field_expression: ($) =>
      prec(PREC.call, seq(field('object', $._expression_no_block), '.', field('field', $.identifier))),

    range_expression: ($) =>
      prec.left(seq($._expression_no_block, '..', $._expression_no_block)),

    parenthesized_expression: ($) => seq('(', $._expression, ')'),

    unary_expression: ($) =>
      prec(PREC.unary, seq(field('operator', choice('-', '!')), $._expression_no_block)),

    binary_expression: ($) => {
      const table = [
        [PREC.or, '||'],
        [PREC.and, '&&'],
        [PREC.cmp, choice('==', '!=', '<', '>', '<=', '>=')],
        [PREC.add, choice('+', '-')],
        // D-7: 剰余は `mod`（`%` はコメント専用）
        [PREC.mul, choice('*', '/', 'mod')],
      ];
      return choice(
        ...table.map(([p, op]) =>
          prec.left(
            p,
            seq(
              field('left', $._expression_no_block),
              field('operator', op),
              field('right', $._expression_no_block),
            ),
          ),
        ),
      );
    },

    // ---- 語彙 ----

    // 型は後置形式：`i64 array` / `T U pair` / `T U hashmap`
    type: ($) => prec.left(repeat1(choice($.primitive_type, $.type_identifier))),

    primitive_type: (_) =>
      choice('i64', 'u64', 'i32', 'u32', 'f32', 'f64', 'f80', 'u8', 'u16', 'array', 'pair', 'hashmap'),

    type_identifier: ($) => alias($.identifier, $.type_identifier),
    label_identifier: ($) => alias($.identifier, $.label_identifier),

    identifier: (_) => /[A-Za-z_][A-Za-z0-9_]*/,
    integer: (_) => /\d+/,
    string: ($) => seq('"', repeat(choice($.escape_sequence, /[^"\\]+/)), '"'),
    escape_sequence: (_) => token.immediate(/\\./),
  },
});
