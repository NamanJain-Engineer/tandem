//! Writes `manifest.json` for the binaries under a folder
//! (`scripts/build-tandem-host.sh`).

fn main() {
    let Some(dir) = std::env::args().nth(1) else {
        eprintln!("usage: manifest <dir>");
        std::process::exit(2);
    };
    match tandem_host::manifest::Manifest::generate(
        std::path::Path::new(&dir),
        tandem_host::protocol::PROTOCOL,
        tandem_host::protocol::VERSION,
    ) {
        Ok(m) if !m.binaries.is_empty() => {}
        Ok(_) => {
            eprintln!("no tandem-host binaries under {dir}");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
