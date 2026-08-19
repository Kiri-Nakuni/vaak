#!/usr/bin/env bash
# tree-sitter 文法を再生成してコミットし、Zed 拡張の rev を更新する。
#
# Zed はローカルパスでの文法参照を受け付けず、必ず repository + rev で git から
# clone する。そのため editors/tree-sitter-reod はローカル git リポジトリになっており、
# grammar.js を編集するたびにコミットと rev の更新が要る。それを1コマンドにまとめる。
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
grammar="$root/editors/tree-sitter-reod"
ext="$root/editors/zed/extension.toml"

cd "$grammar"

echo "==> parser を生成"
npx --yes tree-sitter-cli@latest generate

echo "==> 例をパースして検証"
fail=0
for f in "$root"/examples/*.reod test/syntax-coverage.reod; do
    if npx --yes tree-sitter-cli@latest parse "$f" 2>&1 | grep -qE "ERROR|MISSING"; then
        echo "  NG  $(basename "$f")"
        fail=1
    else
        echo "  ok  $(basename "$f")"
    fi
done
[ "$fail" -eq 0 ] || { echo "パースに失敗しました。rev は更新しません。"; exit 1; }

echo "==> クエリを検証"
for q in "$root"/editors/zed/languages/reod/*.scm; do
    npx --yes tree-sitter-cli@latest query "$q" test/syntax-coverage.reod >/dev/null
    echo "  ok  $(basename "$q")"
done

if [ -n "$(git status --porcelain)" ]; then
    git add -A
    git -c user.name="reod" -c user.email="reod@localhost" \
        commit -q -m "regenerate parser"
    echo "==> コミットしました"
else
    echo "==> 変更なし"
fi

sha="$(git rev-parse HEAD)"
sed -i -E "s|^rev = \".*\"|rev = \"$sha\"|" "$ext"
echo "==> extension.toml の rev を $sha に更新"
echo
echo "Zed でコマンドパレット → 'zed: reload extensions'（初回は 'zed: install dev extension' で $root/editors/zed を指定）"
