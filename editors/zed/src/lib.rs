//! Zed から Vaak の言語サーバを起動する。
//!
//! **文法定義は持たない。** 色分けは意味トークンで来る——
//! 字句器は一つであり、Zed が見る色も検査器が見る字句も同じ実装から出る。

use zed_extension_api::{self as zed, settings::LspSettings, LanguageServerId, Result};

struct VaakExtension;

const BIN: &str = "vaak-lsp";

impl zed::Extension for VaakExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        // **設定で明示されていればそれに従う。** 最優先
        //
        // ```json
        // "lsp": { "vaak-lsp": { "binary": { "path": "/…/vaak-lsp" } } }
        // ```
        if let Ok(s) = LspSettings::for_worktree(id.as_ref(), worktree) {
            if let Some(bin) = s.binary {
                if let Some(path) = bin.path {
                    return Ok(zed::Command {
                        command: path,
                        args: bin.arguments.unwrap_or_default(),
                        env: Vec::new(),
                    });
                }
            }
        }

        // 次に PATH。`cargo install --path .` すればここに入る
        if let Some(path) = worktree.which(BIN) {
            return Ok(zed::Command { command: path, args: Vec::new(), env: Vec::new() });
        }

        // 最後に、開いているワークツリーで `cargo build` した場所。
        //
        // **拡張は WASI の砂場の中で動く**ので `std::fs` はホストの道を見られない。
        // だから `worktree.read_text_file` で当たりを取る——
        // 実行ファイルは読めなくても、**在ることは分かる。**
        let root = worktree.root_path();
        for sub in ["target/release", "target/debug"] {
            let path = format!("{root}/{sub}/{BIN}");
            if worktree.read_text_file(&path).is_ok() {
                return Ok(zed::Command { command: path, args: Vec::new(), env: Vec::new() });
            }
        }

        Err(format!(
            "`{BIN}` が見つかりません。次のどれかをしてください。\n\
             \n\
             1. Vaak のリポジトリで `cargo install --path .` \n\
                （`~/.cargo/bin` に入り、PATH から見つかります）\n\
             2. settings.json に道を書く:\n\
                \"lsp\": {{ \"{BIN}\": {{ \"binary\": {{ \"path\": \"/…/{BIN}\" }} }} }}\n\
             \n\
             `cargo build --release` だけでは、Zed が PATH からしか探せないことがあります。"
        ))
    }
}

zed::register_extension!(VaakExtension);
