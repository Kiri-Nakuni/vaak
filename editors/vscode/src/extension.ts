// Vaak の VS Code 拡張。
//
// **色分けは二層ある。**
//
// | | いつ効くか |
// |---|---|
// | TextMate 文法 | 開いた瞬間から。**言語サーバが要らない** |
// | 意味トークン | 言語サーバが繋がってから。組み込みの型・関数・`$名前` を見分ける |
//
// VS Code は LSP の意味トークンを扱えるので、**Zed と違って文法だけで終わらない。**

import { workspace, ExtensionContext, window } from "vscode";
import { existsSync } from "node:fs";
import { join } from "node:path";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

export function activate(context: ExtensionContext) {
  const cfg = workspace.getConfiguration("vaak");
  if (!cfg.get<boolean>("server.enable", true)) {
    return;
  }
  const configured = cfg.get<string>("server.path", "").trim();
  const target = `${process.platform}-${process.arch}`;
  const executable = process.platform === "win32" ? "vaak-lsp.exe" : "vaak-lsp";
  const bundled = context.asAbsolutePath(join("bin", target, executable));
  // 明示設定を最優先し、target 別 VSIX なら同梱版、portable VSIX なら PATH を使う。
  const command = configured || (existsSync(bundled) ? bundled : "vaak-lsp");

  const serverOptions: ServerOptions = {
    run: { command, transport: TransportKind.stdio },
    debug: { command, transport: TransportKind.stdio },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "vaak" }],
    synchronize: {
      fileEvents: workspace.createFileSystemWatcher("**/*.{vaak,vk}"),
    },
  };

  client = new LanguageClient("vaak", "Vaak", serverOptions, clientOptions);
  client.start().catch((e) => {
    // **色分けは生きている。** 言語サーバが無くても編集はできる
    window.showWarningMessage(
      `Vaak: \`${command}\` を起動できません（${e}）。\n` +
        "platform版VSIXを使うか、`vaak-lsp`をPATHに入れるか、" +
        "`vaak.server.path`に道を書いてください。" +
        "色分けは言語サーバが無くても効きます。",
    );
  });
}

export function deactivate(): Thenable<void> | undefined {
  return client?.stop();
}
