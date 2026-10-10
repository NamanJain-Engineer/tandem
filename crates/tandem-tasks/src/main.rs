//! `tandem-tasks mcp --dir <tasks dir> --connection <id>`: the stdio MCP
//! server bundled as a sidecar (ARCHITECTURE §9.1). Logs go to stderr only;
//! stdout carries the protocol.

use std::path::PathBuf;

use anyhow::{bail, Context};
use tandem_tasks::mcp::{serve_stdio, TasksServer};
use tandem_tasks::TaskStore;

const USAGE: &str =
    "usage: tandem-tasks mcp --dir <tasks dir> (--connection <id> | --connection-env <VAR>)";

struct Args {
    dir: PathBuf,
    connection: String,
}

fn parse(args: &[String]) -> anyhow::Result<Args> {
    if args.first().map(String::as_str) != Some("mcp") {
        bail!(USAGE);
    }
    let (mut dir, mut connection) = (None, None);
    let mut it = args[1..].iter();
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--dir" => dir = it.next().map(PathBuf::from),
            "--connection" => connection = it.next().cloned(),
            // Terminal agents: the pane's env carries its session's
            // connection, since the engine's config is shared by every pane.
            "--connection-env" => {
                connection = it.next().and_then(|var| std::env::var(var).ok());
                if connection.is_none() {
                    // No connection in this session: every call is refused.
                    connection = Some(String::new());
                }
            }
            _ => bail!(USAGE),
        }
    }
    match (dir, connection) {
        (Some(dir), Some(connection)) => Ok(Args { dir, connection }),
        _ => bail!(USAGE),
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if matches!(
        args.first().map(String::as_str),
        Some("--version") | Some("version")
    ) {
        println!("tandem-tasks {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let args = match parse(&args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let store = TaskStore::open(&args.dir).context("opening the task store")?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .context("starting the runtime")?;
    runtime.block_on(serve_stdio(TasksServer::new(store, args.connection)))?;
    Ok(())
}
