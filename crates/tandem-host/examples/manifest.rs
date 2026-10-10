//! Writes `manifest.json` for the binaries under a folder
//! (`scripts/build-tandem-host.sh`), or checks a bundled folder before a
//! release ships (`manifest --check <dir> --require linux-x86_64,…`).

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--check") {
        let Some(dir) = args.get(1) else {
            eprintln!("usage: manifest --check <dir> [--require a,b]");
            std::process::exit(2);
        };
        let required: Vec<&str> = match args.iter().position(|a| a == "--require") {
            Some(i) => args
                .get(i + 1)
                .map(|r| r.split(',').filter(|p| !p.is_empty()).collect())
                .unwrap_or_default(),
            None => Vec::new(),
        };
        match tandem_host::manifest::Manifest::verify(
            std::path::Path::new(dir),
            tandem_host::protocol::PROTOCOL,
            tandem_host::protocol::VERSION,
            &required,
        ) {
            Ok(m) => {
                let platforms: Vec<&str> = m.binaries.keys().map(String::as_str).collect();
                println!("tandem-host {}: {}", m.version, platforms.join(", "));
            }
            Err(problems) => {
                for p in problems {
                    eprintln!("tandem-host bundle: {p}");
                }
                std::process::exit(1);
            }
        }
        return;
    }
    let Some(dir) = args.first() else {
        eprintln!("usage: manifest <dir>");
        std::process::exit(2);
    };
    match tandem_host::manifest::Manifest::generate(
        std::path::Path::new(dir),
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
