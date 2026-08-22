#!/usr/bin/env node
// vendored tree-sitter 文法を作り直す。
//
// 通常は生成物と試験だけを更新する:
//
//   node scripts/sync-grammar.mjs
//
// Zed は git のコミットを取得するため、同じコミット自身の SHA を manifest に
// 書けない。文法を含むコミットを一度作った後、次の変更として pin する:
//
//   node scripts/sync-grammar.mjs --pin
//   git commit editors/zed/extension.toml
//
// `--pin REV REPOSITORY` と明示することもできる。REV に parser.c が無ければ
// 失敗するので、存在しない commit を manifest に書くことはない。

import { execFileSync } from 'node:child_process';
import { copyFileSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const grammar = join(root, 'editors', 'tree-sitter-vaak');
const query = join(grammar, 'queries', 'highlights.scm');
const zedQuery = join(root, 'editors', 'zed', 'languages', 'vaak', 'highlights.scm');
const manifestPath = join(root, 'editors', 'zed', 'extension.toml');
const treeSitterVersion = '0.25.10';
const npx = process.platform === 'win32' ? 'npx.cmd' : 'npx';

function run(command, args, cwd = root, capture = false) {
  return execFileSync(command, args, {
    cwd,
    encoding: capture ? 'utf8' : undefined,
    stdio: capture ? ['ignore', 'pipe', 'inherit'] : 'inherit',
    // Node.js は Windows の .cmd を直接 spawn できない。
    shell: process.platform === 'win32' && command.endsWith('.cmd'),
  });
}

run(npx, ['--yes', `tree-sitter-cli@${treeSitterVersion}`, 'generate'], grammar);
run(npx, ['--yes', `tree-sitter-cli@${treeSitterVersion}`, 'test'], grammar);
copyFileSync(query, zedQuery);

if (process.argv[2] !== '--pin') {
  console.log('文法を生成し、Zed の問合せを同期した');
  console.log('Zed の rev は文法を含むコミット後に: node scripts/sync-grammar.mjs --pin');
  process.exit(0);
}

const revision = process.argv[3] ?? 'HEAD';
const rev = run('git', ['rev-parse', `${revision}^{commit}`], root, true).trim();
const repository = process.argv[4]
  ?? run('git', ['remote', 'get-url', 'origin'], root, true).trim();
run('git', ['cat-file', '-e', `${rev}:editors/tree-sitter-vaak/src/parser.c`]);

const tomlString = value => value.replaceAll('\\', '\\\\').replaceAll('"', '\\"');
let manifest = readFileSync(manifestPath, 'utf8');
const eol = manifest.includes('\r\n') ? '\r\n' : '\n';
if (!/^repository = .*$/m.test(manifest) || !/^rev = .*$/m.test(manifest)) {
  throw new Error('extension.toml に repository または rev が無い');
}
manifest = manifest.replace(/^path = .*\r?\n/m, '');
manifest = manifest.replace(
  /^repository = .*$/m,
  `repository = "${tomlString(repository)}"`,
);
manifest = manifest.replace(
  /^rev = .*$/m,
  `rev = "${tomlString(rev)}"${eol}path = "editors/tree-sitter-vaak"`,
);
writeFileSync(manifestPath, manifest, 'utf8');

console.log(`repository = ${repository}`);
console.log(`rev = ${rev}`);
