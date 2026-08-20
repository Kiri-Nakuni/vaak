#!/usr/bin/env bash
# 文法を作り直し、版方に収め、Zed 拡張の rev を新しくする。
#
# **Zed は文法をローカルの道からは読めない。** 必ず git から取るので、
# `editors/tree-sitter-vaak` 自体が git の版方になっている。
set -euo pipefail
cd "$(dirname "$0")/.."
( cd editors/tree-sitter-vaak
  npx --yes tree-sitter-cli@latest generate
  git add -A
  git diff --cached --quiet || git commit -q -m "文法を作り直す"
)
REV=$(git -C editors/tree-sitter-vaak rev-parse HEAD)
sed -i "s|^rev = \".*\"|rev = \"$REV\"|" editors/zed/extension.toml
cp editors/tree-sitter-vaak/queries/highlights.scm editors/zed/languages/vaak/highlights.scm
echo "rev = $REV"
