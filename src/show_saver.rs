use std::sync::mpsc;
use std::thread;

use crate::show_file::{self, ShowFile, ShowPath};

/// Off-thread worker that writes submitted show files to disk.
pub struct ShowSaver {
    tx: mpsc::Sender<SaveRequest>,
    _handle: thread::JoinHandle<()>,
}

struct SaveRequest {
    path: ShowPath,
    file: ShowFile,
}

impl ShowSaver {
    /// Spawn the worker thread.
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel::<SaveRequest>();
        let handle = thread::Builder::new()
            .name("show-saver".to_string())
            .spawn(move || {
                for req in rx {
                    if let Err(e) = show_file::save(&req.path, &req.file) {
                        log::error!("Show save failed for {}: {e:#}", req.path.display());
                    }
                }
            })
            .expect("failed to spawn show-saver thread");
        Self {
            tx,
            _handle: handle,
        }
    }

    /// Submit a save request. Returns immediately; write errors surface in
    /// the log.
    pub fn submit(&self, path: ShowPath, file: ShowFile) {
        if let Err(e) = self.tx.send(SaveRequest { path, file }) {
            log::error!("Show saver channel closed: {e}");
        }
    }
}
