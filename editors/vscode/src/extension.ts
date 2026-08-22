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
  const command = cfg.get<string>("server.path", "vaak-lsp");

  const serverOptions: ServerOptions = {
    run: { command, transport: TransportKind.stdio },
    debug: { command, transport: TransportKind.stdio },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "vaak" }],
    synchronize: {
      fileEvents: workspace.createFileSystemWatcher("**/*.vaak"),
    },
  };

  client = new LanguageClient("vaak", "Vaak", serverOptions, clientOptions);
  client.start().catch((e) => {
    // **色分けは生きている。** 言語サーバが無くても編集はできる
    window.showWarningMessage(
      `Vaak: \`${command}\` を起動できません（${e}）。\n` +
        "`cargo install --path .` するか、`vaak.server.path` に道を書いてください。" +
        "色分けは言語サーバが無くても効きます。",
    );
  });
}

export function deactivate(): Thenable<void> | undefined {
  return client?.stop();
}
