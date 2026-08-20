//! Vaak の言語サーバ。stdio で話す。
fn main() {
    vaak::lsp::Server::new().run();
}
