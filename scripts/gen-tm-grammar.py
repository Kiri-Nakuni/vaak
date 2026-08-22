#!/usr/bin/env python3
"""VS Code の TextMate 文法を Rust の実装から作る。

**鍵語を手で二度書かない。** `src/lexer.rs` と `src/parser.rs` が唯一の出所である。
足したらこれを走らせること。`tests/grammar.rs` が食い違いを見張っている。
"""
import pathlib, re, sys

root = pathlib.Path(__file__).resolve().parent.parent
lex = (root / 'src/lexer.rs').read_text(encoding='utf-8')
kw = re.findall(r'"([a-z0-9]+)" => Tok::', lex)
if '"mod" => {' in lex:
    kw.append('mod')
bools = sorted(k for k in kw if k in ('true', 'false'))
kw = sorted(set(k for k in kw if k not in ('true', 'false')))
types = sorted(set(re.findall(r'"([a-z0-9]+)" => ValueType::',
                              (root / 'src/parser.rs').read_text(encoding='utf-8'))))

def alt(ws):
    return '|'.join(sorted(ws, key=len, reverse=True))

out = root / 'editors/vscode/syntaxes/vaak.tmLanguage.json'
text = (root / 'scripts/vaak.tmLanguage.template.json').read_text(encoding='utf-8')
text = text.replace('@VAAK_BOOLEANS@', alt(bools))
text = text.replace('@VAAK_KEYWORDS@', alt(kw))
text = text.replace('@VAAK_TYPES@', alt(types))
if '--check' in sys.argv:
    sys.exit(0 if out.read_text(encoding='utf-8') == text else 1)
out.write_text(text, encoding='utf-8')
print(f"鍵語 {len(kw)}、型 {len(types)}、真偽 {bools}")
