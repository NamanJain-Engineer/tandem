//! `tandem-wake`: the "Hey Conductor" keyword spotter as a sidecar (F-CN-07).
//! The app's ONNX Runtime (Parakeet) and sherpa-onnx's can't share one
//! binary, so detection runs here. The app writes frames to stdin: a
//! little-endian u32 sample count, then that many f32le samples at 16 kHz
//! mono; a count of 0 resets the stream. This prints `ready` once the model
//! has loaded and `wake` each time the phrase ends.
//!
//!   tandem-wake --model <dir> [--sensitivity low|medium|high]

use std::io::{BufWriter, Read, Write};
use std::path::PathBuf;

use tandem_voice::wake::sherpa::SherpaKws;
use tandem_voice::wake::{Sensitivity, WakeDetector};

/// At most 10 s per frame.
const MAX_FRAME: usize = 160_000;

fn main() {
    let mut model = None;
    let mut sensitivity = Sensitivity::Medium;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--model" => model = args.next().map(PathBuf::from),
            "--sensitivity" => sensitivity = Sensitivity::parse(&args.next().unwrap_or_default()),
            _ => {}
        }
    }
    let Some(model) = model else {
        eprintln!("usage: tandem-wake --model <dir> [--sensitivity low|medium|high]");
        std::process::exit(2);
    };
    let mut kws = match SherpaKws::load(&model, sensitivity) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("tandem-wake: {e}");
            std::process::exit(1);
        }
    };
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    if writeln!(out, "ready").and_then(|_| out.flush()).is_err() {
        return;
    }
    let mut input = std::io::stdin().lock();
    let mut len = [0u8; 4];
    let mut buf = Vec::new();
    while input.read_exact(&mut len).is_ok() {
        let n = u32::from_le_bytes(len) as usize;
        if n == 0 {
            kws.reset();
            continue;
        }
        if n > MAX_FRAME {
            eprintln!("tandem-wake: frame too large");
            std::process::exit(1);
        }
        buf.resize(n * 4, 0);
        if input.read_exact(&mut buf).is_err() {
            break;
        }
        let samples: Vec<f32> = buf
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        if kws.accept(&samples) && writeln!(out, "wake").and_then(|_| out.flush()).is_err() {
            break;
        }
    }
}
