//! `tandem-host`: the helper Tandem uploads to SSH hosts. Subcommands `serve`
//! (JSON-RPC over stdio), `daemon` (PTY keeper) and `version`
//! (ARCHITECTURE §2.11, §13.1). Built in M14.

fn main() {
    match std::env::args().nth(1).as_deref() {
        Some("version") | Some("--version") => {
            println!("tandem-host {}", env!("CARGO_PKG_VERSION"))
        }
        _ => {
            eprintln!("usage: tandem-host <serve|daemon|version>");
            std::process::exit(2);
        }
    }
}
