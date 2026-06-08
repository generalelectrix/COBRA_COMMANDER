use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use crate::show_file::{self, ShowFile, ShowPath};

const SAVE_THROTTLE: Duration = Duration::from_millis(500);

/// Off-thread worker that writes submitted show files to disk.
pub struct ShowSaver {
    tx: mpsc::Sender<SaveRequest>,
}

struct SaveRequest {
    path: ShowPath,
    file: ShowFile,
}

impl ShowSaver {
    /// Spawn the worker thread.
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel::<SaveRequest>();
        crate::worker::spawn("show-saver", move |_shutdown| run(rx));
        Self { tx }
    }

    /// Submit a save request. Returns immediately; write errors surface in
    /// the log.
    pub fn submit(&self, path: ShowPath, file: ShowFile) {
        if let Err(e) = self.tx.send(SaveRequest { path, file }) {
            log::error!("Show saver channel closed: {e}");
        }
    }
}

fn run(rx: mpsc::Receiver<SaveRequest>) {
    let mut last_write: Option<Instant> = None;
    let mut buffered: Option<SaveRequest> = None;
    loop {
        let recv_result = match (buffered.as_ref(), last_write) {
            (Some(_), Some(t)) => {
                let wait = (t + SAVE_THROTTLE).saturating_duration_since(Instant::now());
                rx.recv_timeout(wait)
            }
            _ => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match recv_result {
            Ok(req) => {
                let can_fire = last_write.is_none_or(|t| t.elapsed() >= SAVE_THROTTLE);
                if can_fire {
                    write_or_log(&req);
                    last_write = Some(Instant::now());
                } else {
                    buffered = Some(req);
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if let Some(req) = buffered.take() {
                    write_or_log(&req);
                    last_write = Some(Instant::now());
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                if let Some(req) = buffered.take() {
                    write_or_log(&req);
                }
                return;
            }
        }
    }
}

fn write_or_log(req: &SaveRequest) {
    match show_file::save(&req.path, &req.file) {
        Ok(()) => log::debug!("Show saved to {}", req.path.display()),
        Err(e) => log::error!("Show save failed for {}: {e:#}", req.path.display()),
    }
}
