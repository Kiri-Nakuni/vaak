// 改行終端トークン。
//
// アトリビュート（D-18r）と関数定義の返り値型（D-21r）は改行で終端する。
// 型は後置形式で識別子の列（`i64 array`）なので、終端がないと次の行の先頭の
// 識別子を型として吸い込んでしまう——インタプリタ側は各トークンの行番号を
// 見て切っている（interp.rs の skip_type_to_eol）。tree-sitter には
// 行の概念がないので、外部スキャナで明示的に切る。

#include "tree_sitter/parser.h"

enum TokenType { LINE_END, BLOCK_COMMENT };

// 無名標準ライブラリのブロックコメントを、道具経路ではコメントとして扱う。
//
//   comment;
//       ここは壊れていてよい
//   label _comment_;
//
// 実行経路ではこれは「flat な goto で読み飛ばされる区間」であり、道具経路では
// 「解析されない区間」である。**別の構造ではなく粗視化**なので相似が保たれる。
//
// ただし tree-sitter は flow の束縛を追えないので、**名前を決め打ちしている**。
// ユーザーが `comment` を再束縛すると食い違う——道具経路の既知の近似である。
static bool match_word(TSLexer *lexer, const char *w) {
  for (const char *p = w; *p; p++) {
    if (lexer->lookahead != (int32_t)*p) return false;
    lexer->advance(lexer, false);
  }
  return true;
}

static void skip_blanks(TSLexer *lexer) {
  while (lexer->lookahead == ' ' || lexer->lookahead == '\t' ||
         lexer->lookahead == '\r' || lexer->lookahead == '\n') {
    lexer->advance(lexer, false);
  }
}

void *tree_sitter_reod_external_scanner_create(void) { return NULL; }
void tree_sitter_reod_external_scanner_destroy(void *payload) { (void)payload; }

unsigned tree_sitter_reod_external_scanner_serialize(void *payload, char *buffer) {
  (void)payload;
  (void)buffer;
  return 0;
}

void tree_sitter_reod_external_scanner_deserialize(void *payload, const char *buffer,
                                                 unsigned length) {
  (void)payload;
  (void)buffer;
  (void)length;
}

bool tree_sitter_reod_external_scanner_scan(void *payload, TSLexer *lexer,
                                          const bool *valid_symbols) {
  (void)payload;
  if (valid_symbols[LINE_END]) {

    // 行内の空白を読み飛ばす
    while (lexer->lookahead == ' ' || lexer->lookahead == '\t' ||
           lexer->lookahead == '\r') {
      lexer->advance(lexer, true);
    }

    // 行末コメントは行の一部として扱う（`%` はコメント専用、D-7）
    if (lexer->lookahead == '%') {
      while (lexer->lookahead != '\n' && !lexer->eof(lexer)) {
        lexer->advance(lexer, true);
    }
    }

    if (lexer->lookahead == '\n') {
      lexer->advance(lexer, true);
      lexer->result_symbol = LINE_END;
      return true;
    }
    if (lexer->eof(lexer)) {
        lexer->result_symbol = LINE_END;
        return true;
    }
    }

  if (valid_symbols[BLOCK_COMMENT]) {
    while (lexer->lookahead == ' ' || lexer->lookahead == '\t' ||
           lexer->lookahead == '\r' || lexer->lookahead == '\n') {
      lexer->advance(lexer, true);
    }
    if (lexer->lookahead != 'c') return false;
    if (!match_word(lexer, "comment")) return false;
    skip_blanks(lexer);
    if (lexer->lookahead != ';') return false;
    lexer->advance(lexer, false);
    while (!lexer->eof(lexer)) {
      if (lexer->lookahead == 'l') {
        if (match_word(lexer, "label")) {
          skip_blanks(lexer);
          if (match_word(lexer, "_comment_")) {
            skip_blanks(lexer);
            if (lexer->lookahead == ';') {
              lexer->advance(lexer, false);
              lexer->result_symbol = BLOCK_COMMENT;
              return true;
            }
          }
        }
        continue;
      }
      lexer->advance(lexer, false);
    }
    return false;
  }

  return false;
}
