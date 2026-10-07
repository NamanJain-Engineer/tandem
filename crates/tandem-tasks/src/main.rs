//! `tandem-tasks mcp --dir <tasks dir> --connection <id>`: stdio MCP server
//! bundled as a sidecar (ARCHITECTURE §9.1).

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version") | Some("version") => {
            println!("tandem-tasks {}", env!("CARGO_PKG_VERSION"))
        }
        _ => {
            eprintln!("usage: tandem-tasks mcp --dir <tasks dir> --connection <id>");
            std::process::exit(2);
        }
    }
}
