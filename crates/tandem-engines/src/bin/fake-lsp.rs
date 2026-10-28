//! `fake-lsp`: the test language server (F-CD-23, feature `fixtures`). The
//! E2E puts it on PATH under a real server's name.

#[tokio::main(flavor = "current_thread")]
async fn main() {
    tandem_engines::fake_lsp::serve(tokio::io::stdin(), tokio::io::stdout()).await;
}
