//! Remote PTYs (F-RM-01): a session whose bytes come from an SSH host.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tandem_pty::{
    wait_until, FrameKind, OutputSink, PtyManager, RemoteFeed, RemotePty, SpawnSpec, VecSink,
};

/// A host that echoes what's typed and exits 0 when killed.
#[derive(Default)]
struct FakeHost {
    feed: Mutex<Option<RemoteFeed>>,
    sizes: Mutex<Vec<(u16, u16)>>,
    kills: Mutex<u32>,
}

impl RemotePty for FakeHost {
    fn write(&self, data: &[u8]) -> std::io::Result<()> {
        if let Some(f) = self.feed.lock().expect("feed").as_ref() {
            f.data(data.to_vec());
        }
        Ok(())
    }
    fn resize(&self, cols: u16, rows: u16) {
        self.sizes.lock().expect("sizes").push((cols, rows));
    }
    fn kill(&self) {
        *self.kills.lock().expect("kills") += 1;
        if let Some(f) = self.feed.lock().expect("feed").as_ref() {
            f.exit(Some(0));
        }
    }
}

fn spec(pane: &str) -> SpawnSpec {
    SpawnSpec {
        pane_id: pane.into(),
        program: PathBuf::from("remote"),
        args: Vec::new(),
        cwd: PathBuf::from("/home/tandem/project"),
        env: Default::default(),
        cols: 80,
        rows: 24,
        scrollback_lines: 1000,
        mark_token: None,
    }
}

#[test]
fn f_rm_01_remote_pane_streams_writes_resizes_and_ends_through_its_host() {
    let mgr = PtyManager::new();
    let host = Arc::new(FakeHost::default());
    let sink = Arc::new(VecSink::default());
    let (info, feed) = mgr
        .spawn_remote(
            spec("r1"),
            4242,
            37,
            host.clone(),
            Some(sink.clone() as Arc<dyn OutputSink>),
        )
        .expect("spawn");
    *host.feed.lock().expect("feed") = Some(feed.clone());
    assert_eq!(info.pid, 4242, "the remote PID");
    // Diagnostics shows how long the host took to start it (NFR §5.7, D-823).
    let metrics = mgr.metrics();
    assert_eq!(
        metrics
            .iter()
            .find(|p| p.pane_id == "r1")
            .map(|p| p.spawn_ms),
        Some(37)
    );
    assert_eq!(mgr.pid("r1"), Some(4242));
    assert!(mgr.is_remote("r1"));
    feed.data(b"hi\r\n".to_vec());
    mgr.write("r1", b"typed").expect("write");
    assert!(wait_until(Duration::from_secs(5), || {
        String::from_utf8_lossy(&sink.data()).contains("hi\r\ntyped")
    }));
    mgr.resize("r1", 100, 30).expect("resize");
    assert_eq!(host.sizes.lock().expect("sizes").as_slice(), [(100, 30)]);
    // The scrollback and screen work as for a local pane.
    assert!(mgr.screen_text("r1").unwrap_or_default().contains("hi"));
    let exit = mgr.kill("r1").expect("kill");
    assert_eq!(exit.code, Some(0));
    assert_eq!(*host.kills.lock().expect("kills"), 1);
    assert!(wait_until(Duration::from_secs(5), || sink.has_exit()));
    let frames = sink.frames.lock().expect("frames");
    assert_eq!(frames.last().map(|f| f.kind), Some(FrameKind::Exit));
}

#[test]
fn f_rm_01_quitting_ends_remote_panes_through_the_host_not_local_signals() {
    let mgr = PtyManager::new();
    let host = Arc::new(FakeHost::default());
    // A PID that is ours locally must never be signalled for a remote pane.
    let (_, feed) = mgr
        .spawn_remote(spec("r2"), std::process::id(), 0, host.clone(), None)
        .expect("spawn");
    *host.feed.lock().expect("feed") = Some(feed);
    mgr.shutdown_within(Duration::from_secs(2));
    assert_eq!(*host.kills.lock().expect("kills"), 1);
    assert!(mgr.panes().is_empty());
}
