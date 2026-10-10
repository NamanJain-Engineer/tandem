//! The mock OAuth server and GitHub-shaped upstream for the desktop E2E
//! suite (test builds only: `--features test-vendors`). Usage:
//! `mock-plugins <port>`.

#[path = "../../tests/support/mock_as.rs"]
mod mock_as;

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::io::Result<()> {
    let port = std::env::args()
        .nth(1)
        .and_then(|p| p.parse().ok())
        .unwrap_or(8791);
    let server = mock_as::MockAs::start(port).await?;
    println!("mock-plugins listening on {}", server.uri());
    std::future::pending::<()>().await;
    Ok(())
}
