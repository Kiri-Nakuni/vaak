const childProcess = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

const extensionRoot = __dirname;
const repositoryRoot = path.resolve(extensionRoot, "..", "..");
const binRoot = path.join(extensionRoot, "bin");
const portable = process.argv.includes("--portable");
const target = `${process.platform}-${process.arch}`;
const supportedTargets = new Set([
  "win32-x64",
  "win32-arm64",
  "linux-x64",
  "linux-arm64",
  "darwin-x64",
  "darwin-arm64",
]);

function run(command, args, cwd) {
  const result = childProcess.spawnSync(command, args, {
    cwd,
    stdio: "inherit",
    windowsHide: true,
  });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}

fs.rmSync(binRoot, { recursive: true, force: true });

if (!portable) {
  if (!supportedTargets.has(target)) {
    throw new Error(`VSIXの同梱targetに未対応: ${target}`);
  }
  const cargoTarget = path.join(repositoryRoot, "target", "vscode-lsp", target);
  run(
    "cargo",
    ["build", "--release", "--bin", "vaak-lsp", "--target-dir", cargoTarget],
    repositoryRoot,
  );

  const executable = process.platform === "win32" ? "vaak-lsp.exe" : "vaak-lsp";
  const source = path.join(cargoTarget, "release", executable);
  const destinationDir = path.join(binRoot, target);
  fs.mkdirSync(destinationDir, { recursive: true });
  fs.copyFileSync(source, path.join(destinationDir, executable));
}

const vsceRoot = path.dirname(require.resolve("@vscode/vsce/package.json"));
const vsce = path.join(vsceRoot, "vsce");
const output = portable ? "vaak-portable.vsix" : `vaak-${target}.vsix`;
const args = [vsce, "package", "--out", output];
if (!portable) args.push("--target", target);
run(process.execPath, args, extensionRoot);
