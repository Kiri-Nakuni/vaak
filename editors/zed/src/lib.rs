//! Zed から Vaak の言語サーバを起動する。
//!
//! **文法定義は持たない。** 色分けは意味トークンで来る——
//! 字句器は一つであり、Zed が見る色も検査器が見る字句も同じ実装から出る。

use zed_extension_api::{self as zed, LanguageServerId, Result};

struct VaakExtension;

const BIN: &str = "vaak-lsp";

impl zed::Extension for VaakExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        _id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        // 1. PATH にあるもの
        // 2. ワークツリーで `cargo build --release` したもの
        // 3. `cargo build` したもの
        let path = worktree
            .which(BIN)
            .or_else(|| {
                let root = worktree.root_path();
                for sub in ["target/release", "target/debug"] {
                    let p = format!("{root}/{sub}/{BIN}");
                    if std::fs::metadata(&p).is_ok() {
                        return Some(p);
                    }
                }
                None
            })
            .ok_or_else(|| {
                format!(
                    "`{BIN}` が見つかりません。\n\
                     Vaak のリポジトリで `cargo build --release` を走らせるか、\n\
                     `{BIN}` を PATH に置いてください。"
                )
            })?;

        Ok(zed::Command { command: path, args: Vec::new(), env: Vec::new() })
    }
}

zed::register_extension!(VaakExtension);
