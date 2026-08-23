const esbuild = require("esbuild");
const fs = require("node:fs");
const path = require("node:path");

const production = process.argv.includes("--production");
const watch = process.argv.includes("--watch");
const root = __dirname;
const out = path.join(root, "out");

function copyRuntimeFiles() {
  fs.mkdirSync(out, { recursive: true });

  // vscode-languageclient は Unix でのプロセス終了時にこのシェルスクリプトを使う。
  // JavaScript を束ねてもこのファイルは自動で入らないので、明示的に隣へ置く。
  const clientRoot = path.dirname(require.resolve("vscode-languageclient/package.json"));
  const clientNode = path.join(clientRoot, "lib", "node");
  const terminate = path.join(out, "terminateProcess.sh");
  fs.copyFileSync(path.join(clientNode, "terminateProcess.sh"), terminate);
  fs.chmodSync(terminate, 0o755);

  // 依存を単一 JS に束ねると元の LICENSE は VSIX へ入らない。
  // lockfile が定める実行時依存の許諾文を、同じビルドで一緒に収録する。
  const licenses = path.join(out, "licenses");
  fs.rmSync(licenses, { recursive: true, force: true });
  const lock = JSON.parse(fs.readFileSync(path.join(root, "package-lock.json"), "utf8"));
  for (const [packagePath, metadata] of Object.entries(lock.packages)) {
    if (!packagePath.startsWith("node_modules/") || metadata.dev) continue;

    const source = path.join(root, ...packagePath.split("/"));
    const manifest = JSON.parse(fs.readFileSync(path.join(source, "package.json"), "utf8"));
    const license = fs
      .readdirSync(source)
      .find((name) => /^(licen[cs]e|copying)(\..*)?$/i.test(name));
    if (!license) throw new Error(`${manifest.name} ${manifest.version} の許諾文が見つかりません`);

    const name = manifest.name.replace(/^@/, "").replaceAll("/", "-");
    const destination = path.join(licenses, `${name}-${manifest.version}`);
    fs.mkdirSync(destination, { recursive: true });
    fs.copyFileSync(path.join(source, license), path.join(destination, license));
  }
}

async function main() {
  const options = {
    entryPoints: [path.join(root, "src", "extension.ts")],
    bundle: true,
    external: ["vscode"],
    format: "cjs",
    minify: production,
    outfile: path.join(out, "extension.js"),
    platform: "node",
    sourcemap: !production,
  };

  if (watch) {
    const context = await esbuild.context(options);
    await context.watch();
  } else {
    await esbuild.build(options);
  }
  copyRuntimeFiles();
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
