#include "tree_sitter/parser.h"

#include <stdbool.h>
#include <stdint.h>

enum TokenType {
  LINE_COMMENT,
  BLOCK_COMMENT,
};

void *tree_sitter_vaak_external_scanner_create(void) { return NULL; }

void tree_sitter_vaak_external_scanner_destroy(void *payload) { (void)payload; }

unsigned tree_sitter_vaak_external_scanner_serialize(void *payload, char *buffer) {
  (void)payload;
  (void)buffer;
  return 0;
}

void tree_sitter_vaak_external_scanner_deserialize(
    void *payload, const char *buffer, unsigned length) {
  (void)payload;
  (void)buffer;
  (void)length;
}

static void advance(TSLexer *lexer) { lexer->advance(lexer, false); }

static void skip(TSLexer *lexer) { lexer->advance(lexer, true); }

bool tree_sitter_vaak_external_scanner_scan(
    void *payload, TSLexer *lexer, const bool *valid_symbols) {
  (void)payload;
  while (lexer->lookahead == ' ' || lexer->lookahead == '\t' ||
         lexer->lookahead == '\r' || lexer->lookahead == '\n') {
    skip(lexer);
  }
  if (lexer->lookahead != '%') {
    return false;
  }

  advance(lexer);
  if (lexer->lookahead != '{') {
    if (!valid_symbols[LINE_COMMENT]) {
      return false;
    }
    while (!lexer->eof(lexer) && lexer->lookahead != '\n') {
      advance(lexer);
    }
    lexer->mark_end(lexer);
    lexer->result_symbol = LINE_COMMENT;
    return true;
  }
  if (!valid_symbols[BLOCK_COMMENT]) {
    return false;
  }
  advance(lexer);

  uint32_t depth = 1;
  while (depth > 0) {
    if (lexer->eof(lexer)) {
      // 編集途中でも残りをコメントとして色付けする。
      break;
    }
    if (lexer->lookahead == '%') {
      advance(lexer);
      if (lexer->lookahead == '{') {
        advance(lexer);
        depth += 1;
      }
      continue;
    }
    if (lexer->lookahead == '}') {
      advance(lexer);
      if (lexer->lookahead == '%') {
        advance(lexer);
        depth -= 1;
      }
      continue;
    }
    advance(lexer);
  }

  lexer->mark_end(lexer);
  lexer->result_symbol = BLOCK_COMMENT;
  return true;
}
