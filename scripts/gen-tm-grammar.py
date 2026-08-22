#!/usr/bin/env python3
"""VS Code の TextMate 文法を Rust の実装から作る。

**鍵語を手で二度書かない。** `src/lexer.rs` と `src/parser.rs` が唯一の出所である。
足したらこれを走らせること。`tests/grammar.rs` が食い違いを見張っている。
"""
import json, pathlib, re, sys

root = pathlib.Path(__file__).resolve().parent.parent
lex = (root / 'src/lexer.rs').read_text()
kw = re.findall(r'"([a-z0-9]+)" => Tok::', lex)
if '"mod" => {' in lex:
    kw.append('mod')
bools = sorted(k for k in kw if k in ('true', 'false'))
kw = sorted(set(k for k in kw if k not in ('true', 'false')))
types = sorted(set(re.findall(r'"([a-z0-9]+)" => ValueType::',
                              (root / 'src/parser.rs').read_text())))

def alt(ws):
    return '|'.join(sorted(ws, key=len, reverse=True))

g = {
  "$schema": "https://raw.githubusercontent.com/martinring/tmlanguage/master/tmlanguage.json",
  "name": "Vaak",
  "scopeName": "source.vaak",
  "patterns": [{"include": "#comment"}, {"include": "#string"}, {"include": "#number"},
               {"include": "#keyword"}, {"include": "#type"}, {"include": "#flow"},
               {"include": "#function"}, {"include": "#operator"}],
  "repository": {
    "comment": {"patterns": [
      {"name": "comment.block.vaak", "begin": "%\\{", "end": "\\}%"},
      {"name": "comment.line.percentage.vaak", "match": "%.*$"}
    ]},
    "string": {"name": "string.quoted.double.vaak", "begin": "\"", "end": "\"",
               "patterns": [{"name": "constant.character.escape.vaak", "match": "\\\\."}]},
    "number": {"patterns": [
      {"name": "constant.numeric.float.vaak",
       "match": "\\b[0-9][0-9_]*\\.[0-9][0-9_]*([eE][+-]?[0-9]+)?\\b"},
      {"name": "constant.numeric.integer.vaak",
       "match": "\\b(0[xX][0-9a-fA-F_]+|0[bB][01_]+|0[oO][0-7_]+|[0-9][0-9_]*)\\b"}
    ]},
    "keyword": {"patterns": [
      {"name": "constant.language.boolean.vaak", "match": r"\b(" + alt(bools) + r")\b"},
      {"name": "keyword.control.vaak", "match": r"\b(" + alt(kw) + r")\b"}
    ]},
    "type": {"name": "support.type.vaak", "match": r"\b(" + alt(types) + r")\b"},
    "flow": {"name": "entity.name.function.macro.vaak", "match": r"\$[A-Za-z_][A-Za-z0-9_]*"},
    "function": {"name": "entity.name.function.vaak",
                 "match": r"\b([A-Za-z_][A-Za-z0-9_]*)(?=\s*\()"},
    "operator": {"name": "keyword.operator.vaak",
                 "match": r"\?\?|\|>|->|=>|:=|&=|[+\-*/]=|<<=|>>=|\^=|\|=|<<|>>|<=|>=|==|!=|&&|\|\||[+\-*/!&^|<>=.]"}
  }
}
out = root / 'editors/vscode/syntaxes/vaak.tmLanguage.json'
text = json.dumps(g, ensure_ascii=False, indent=2) + "\n"
if '--check' in sys.argv:
    sys.exit(0 if out.read_text() == text else 1)
out.write_text(text)
print(f"鍵語 {len(kw)}、型 {len(types)}、真偽 {bools}")
