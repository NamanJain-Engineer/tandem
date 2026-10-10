//! `tandem-host`: the helper Vibezeo uploads to SSH hosts (ARCHITECTURE
//! §2.11, §13.1). `serve` pipes one connection to the session daemon,
//! `daemon` keeps the PTYs, `run` starts a thread's engine, `version`
//! prints this build's version.

use std::path::PathBuf;

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

#[cfg(unix)]
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from(&args[0]));
    let code = match args.get(1).map(String::as_str) {
        Some("version") | Some("--version") => {
            println!("tandem-host {}", tandem_host::protocol::VERSION);
            0
        }
        Some("serve") => match tandem_host::serve::serve(&exe, &home()) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("tandem-host: {e}");
                1
            }
        },
        Some("daemon") => match tandem_host::daemon::main(home()) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("tandem-host: {e}");
                1
            }
        },
        Some("run") => match tandem_host::serve::parse_run(&args[2..]) {
            Ok(run) => tandem_host::serve::run(run, &home()).unwrap_or_else(|e| {
                eprintln!("tandem-host: {e}");
                127
            }),
            Err(e) => {
                eprintln!("tandem-host run: {e}");
                2
            }
        },
        _ => {
            eprintln!("usage: tandem-host <serve|daemon|run|version>");
            2
        }
    };
    std::process::exit(code);
}

#[cfg(not(unix))]
fn main() {
    let _ = home();
    eprintln!("tandem-host runs on Linux and macOS hosts");
    std::process::exit(2);
}
