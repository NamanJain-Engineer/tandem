//! PTYs that run on another machine (F-RM-01): the process lives on an SSH
//! host and its bytes arrive over the host's connection. The session here
//! is a normal one (ring, marks, side channel, batching, flow control);
//! only reading, writing, resizing and ending go through [`RemotePty`] and
//! [`RemoteFeed`] instead of a local PTY. A remote PID is never signalled
//! locally.

use std::io::{Read, Write};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};

use portable_pty::{Child, ChildKiller, ExitStatus};

/// What the session asks of the remote side.
pub trait RemotePty: Send + Sync {
    fn write(&self, data: &[u8]) -> std::io::Result<()>;
    fn resize(&self, cols: u16, rows: u16);
    /// Ends the remote process tree; its exit arrives through the feed.
    fn kill(&self);
}

type ExitState = Arc<(Mutex<Option<Option<i32>>>, Condvar)>;

/// Where the remote side delivers output and the exit.
#[derive(Clone)]
pub struct RemoteFeed {
    data: Arc<Mutex<Option<mpsc::Sender<Vec<u8>>>>>,
    exit: ExitState,
}

impl RemoteFeed {
    pub fn data(&self, bytes: Vec<u8>) {
        if bytes.is_empty() {
            return;
        }
        if let Ok(tx) = self.data.lock() {
            if let Some(tx) = tx.as_ref() {
                let _ = tx.send(bytes);
            }
        }
    }

    /// The process ended (`None`: its code is unknown, e.g. the host is gone).
    pub fn exit(&self, code: Option<i32>) {
        let (lock, cv) = &*self.exit;
        if let Ok(mut e) = lock.lock() {
            if e.is_none() {
                *e = Some(code);
            }
        }
        cv.notify_all();
        // The reader sees the end of the stream.
        if let Ok(mut tx) = self.data.lock() {
            tx.take();
        }
    }
}

pub(crate) fn channel(
    pid: u32,
    backend: Arc<dyn RemotePty>,
) -> (RemoteFeed, FeedReader, RemoteChild) {
    let (tx, rx) = mpsc::channel();
    let exit: ExitState = Arc::default();
    (
        RemoteFeed {
            data: Arc::new(Mutex::new(Some(tx))),
            exit: exit.clone(),
        },
        FeedReader {
            rx,
            pending: Vec::new(),
        },
        RemoteChild { pid, exit, backend },
    )
}

pub(crate) struct FeedReader {
    rx: mpsc::Receiver<Vec<u8>>,
    pending: Vec<u8>,
}

impl Read for FeedReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pending.is_empty() {
            match self.rx.recv() {
                Ok(d) => self.pending = d,
                Err(_) => return Ok(0),
            }
        }
        let n = buf.len().min(self.pending.len());
        buf[..n].copy_from_slice(&self.pending[..n]);
        self.pending.drain(..n);
        Ok(n)
    }
}

pub(crate) struct RemoteWriter(pub Arc<dyn RemotePty>);

impl Write for RemoteWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Waits for the feed's exit, as a local child is waited for.
#[derive(Clone)]
pub(crate) struct RemoteChild {
    pid: u32,
    exit: ExitState,
    backend: Arc<dyn RemotePty>,
}

impl std::fmt::Debug for RemoteChild {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteChild")
            .field("pid", &self.pid)
            .finish()
    }
}

fn status(code: Option<i32>) -> ExitStatus {
    match code {
        Some(c) => ExitStatus::with_exit_code(c as u32),
        None => ExitStatus::with_signal("SIGHUP"),
    }
}

impl ChildKiller for RemoteChild {
    fn kill(&mut self) -> std::io::Result<()> {
        self.backend.kill();
        Ok(())
    }

    fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
        Box::new(self.clone())
    }
}

impl Child for RemoteChild {
    fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        Ok(self.exit.0.lock().ok().and_then(|e| *e).map(status))
    }

    fn wait(&mut self) -> std::io::Result<ExitStatus> {
        let (lock, cv) = &*self.exit;
        let mut e = lock.lock().map_err(|_| std::io::Error::other("poisoned"))?;
        while e.is_none() {
            e = cv.wait(e).map_err(|_| std::io::Error::other("poisoned"))?;
        }
        Ok(status(e.unwrap_or(None)))
    }

    fn process_id(&self) -> Option<u32> {
        Some(self.pid)
    }

    #[cfg(windows)]
    fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> {
        None
    }
}
