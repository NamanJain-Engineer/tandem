//! Clipboard images staged for agents (F-CD-05): PNGs under
//! `$DATA/staging/<pane>/`, deleted after 7 days.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use tandem_core::{ErrorCode, IpcError};

/// Staged files older than this are swept at startup.
pub const KEEP_FOR: Duration = Duration::from_secs(7 * 24 * 3600);

/// Encodes RGBA pixels as a PNG in `dir` and returns its path.
pub fn write_png(dir: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<PathBuf, IpcError> {
    let io = |e: std::io::Error| IpcError::new(ErrorCode::Io, e.to_string());
    if width == 0 || height == 0 || rgba.len() != (width as usize) * (height as usize) * 4 {
        return Err(IpcError::new(
            ErrorCode::Invalid,
            "The clipboard image is empty",
        ));
    }
    std::fs::create_dir_all(dir).map_err(io)?;
    let stamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = dir.join(format!("pasted-{stamp}.png"));
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;
        writer
            .write_image_data(rgba)
            .map_err(|e| IpcError::new(ErrorCode::Internal, e.to_string()))?;
    }
    tandem_core::atomic::atomic_write(&path, &bytes, Some(0o600)).map_err(io)?;
    Ok(path)
}

/// Deletes staged files older than `keep` (and empty pane folders).
pub fn sweep(root: &Path, keep: Duration) {
    let Ok(panes) = std::fs::read_dir(root) else {
        return;
    };
    let now = SystemTime::now();
    for pane in panes.flatten() {
        let Ok(files) = std::fs::read_dir(pane.path()) else {
            continue;
        };
        for file in files.flatten() {
            let old = file
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| now.duration_since(t).ok())
                .is_some_and(|age| age > keep);
            if old {
                let _ = std::fs::remove_file(file.path());
            }
        }
        let _ = std::fs::remove_dir(pane.path());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_cd_05_3_a_staged_image_is_a_png_that_decodes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pixels: Vec<u8> = (0..4 * 3 * 2).map(|i| (i * 10) as u8).collect();
        let path = write_png(&dir.path().join("pn_1"), 3, 2, &pixels).expect("write");
        let decoder = png::Decoder::new(std::fs::File::open(&path).expect("open"));
        let mut reader = decoder.read_info().expect("png header");
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).expect("frame");
        assert_eq!((info.width, info.height), (3, 2));
        assert_eq!(&buf[..info.buffer_size()], &pixels[..]);
        assert!(
            write_png(dir.path(), 3, 2, &[0; 5]).is_err(),
            "size mismatch"
        );
    }

    #[test]
    fn sweep_deletes_only_old_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pane = dir.path().join("pn_1");
        std::fs::create_dir_all(&pane).expect("mkdir");
        std::fs::write(pane.join("new.png"), b"x").expect("write");
        sweep(dir.path(), Duration::from_secs(3600));
        assert!(pane.join("new.png").exists());
        sweep(dir.path(), Duration::ZERO);
        std::thread::sleep(Duration::from_millis(10));
        sweep(dir.path(), Duration::ZERO);
        assert!(!pane.exists(), "an emptied pane folder goes too");
    }
}
