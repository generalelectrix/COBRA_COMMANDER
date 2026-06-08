//! Cooperative shutdown for long-lived worker threads.
//!
//! A thread whose only input is a channel stops when that channel's sender
//! drops, so it needs no signal. This module serves loops with no closable
//! input: they observe a shared [`Shutdown`] flag and return, and [`Workers`]
//! joins their handles together on request.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use log::error;

/// State shared between a [`Shutdown`] and its clones.
#[derive(Default)]
struct Inner {
    /// Read lock-free by [`Shutdown::triggered`] on hot paths.
    flag: AtomicBool,
    /// Held only to coordinate the condvar, so a [`Shutdown::sleep`] waiter
    /// cannot miss a trigger that lands between its flag check and its wait.
    guard: Mutex<()>,
    wake: Condvar,
}

/// A signal a worker observes to learn it should stop.
#[derive(Clone, Default)]
pub struct Shutdown(Arc<Inner>);

impl Shutdown {
    /// True once shutdown has been requested.
    pub fn triggered(&self) -> bool {
        self.0.flag.load(Ordering::Acquire)
    }

    /// Request shutdown and wake every thread blocked in [`Shutdown::sleep`].
    fn trigger(&self) {
        let _guard = self.0.guard.lock().expect("shutdown lock");
        self.0.flag.store(true, Ordering::Release);
        self.0.wake.notify_all();
    }

    /// Block until shutdown is triggered or `dur` elapses.
    pub fn sleep(&self, dur: Duration) {
        let guard = self.0.guard.lock().expect("shutdown lock");
        let _ = self
            .0
            .wake
            .wait_timeout_while(guard, dur, |_| !self.triggered())
            .expect("shutdown lock");
    }
}

/// A registry of poll-driven worker threads that are signalled and joined
/// together.
#[derive(Default)]
pub struct Workers {
    shutdown: Shutdown,
    handles: Mutex<Vec<JoinHandle<()>>>,
}

/// The names of workers that did not exit within the join timeout.
pub struct Stragglers(pub Vec<String>);

static WORKERS: OnceLock<Workers> = OnceLock::new();

/// The process-wide worker registry.
pub fn workers() -> &'static Workers {
    WORKERS.get_or_init(Workers::default)
}

impl Workers {
    /// Spawn a named worker and track its handle for
    /// [`Workers::shutdown_and_join`].
    ///
    /// The worker may poll the [`Shutdown`] to stop; one driven only by a
    /// channel can ignore it and return when its sender drops instead.
    pub fn spawn(&self, name: &str, body: impl FnOnce(Shutdown) + Send + 'static) {
        let shutdown = self.shutdown.clone();
        let handle = thread::Builder::new()
            .name(name.to_string())
            .spawn(move || body(shutdown))
            .expect("spawn worker thread");
        self.handles.lock().expect("workers lock").push(handle);
    }

    /// Request shutdown and join every registered worker, abandoning any that
    /// outlive `timeout`. Returns the names of the abandoned threads.
    ///
    /// The whole set is awaited together rather than one handle at a time, so a
    /// channel-driven worker that only stops once a flag-driven worker drops its
    /// sender is not gated on the order the two were registered.
    pub fn shutdown_and_join(&self, timeout: Duration) -> Stragglers {
        self.shutdown.trigger();
        let handles: Vec<JoinHandle<()>> = self
            .handles
            .lock()
            .expect("workers lock")
            .drain(..)
            .collect();
        let deadline = Instant::now() + timeout;
        while handles.iter().any(|h| !h.is_finished()) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        let mut stragglers = Vec::new();
        for handle in handles {
            let name = handle.thread().name().unwrap_or("<unnamed>").to_string();
            if handle.is_finished() {
                if handle.join().is_err() {
                    error!("worker thread {name} panicked during shutdown");
                }
            } else {
                stragglers.push(name);
            }
        }
        Stragglers(stragglers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::UdpSocket;

    /// The shutdown bug, captured: a poll-driven worker that owns a bound
    /// socket must stop on signal and free its port for immediate rebind.
    #[test]
    fn shutdown_joins_polling_worker_and_frees_its_socket() {
        let workers = Workers::default();
        let socket = UdpSocket::bind("127.0.0.1:0").expect("bind ephemeral");
        let addr = socket.local_addr().expect("local addr");
        socket
            .set_read_timeout(Some(Duration::from_millis(50)))
            .expect("set read timeout");

        workers.spawn("test-listener", move |shutdown| {
            let mut buf = [0u8; 16];
            while !shutdown.triggered() {
                // Times out periodically so the loop re-checks the flag, just
                // like the real OSC listener's read timeout.
                let _ = socket.recv_from(&mut buf);
            }
            // `socket` drops here on return, freeing the port.
        });

        let Stragglers(stragglers) = workers.shutdown_and_join(Duration::from_secs(2));
        assert!(
            stragglers.is_empty(),
            "worker did not stop within timeout: {stragglers:?}"
        );

        UdpSocket::bind(addr).expect("port should be free immediately after shutdown");
    }

    /// A worker blocked in `sleep` wakes the instant shutdown is triggered
    /// instead of waiting out its full duration.
    #[test]
    fn sleep_wakes_immediately_on_shutdown() {
        let workers = Workers::default();
        let started = Instant::now();
        workers.spawn("sleeper", |shutdown| {
            // Would block for a minute if the condvar never woke it.
            shutdown.sleep(Duration::from_secs(60));
        });
        thread::sleep(Duration::from_millis(50));
        let Stragglers(stragglers) = workers.shutdown_and_join(Duration::from_secs(2));
        assert!(
            stragglers.is_empty(),
            "sleeper did not wake: {stragglers:?}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "sleep blocked instead of waking on the signal"
        );
    }

    /// A worker that ignores the flag is reported as a straggler rather than
    /// hanging the join forever.
    #[test]
    fn shutdown_reports_a_worker_that_ignores_the_flag() {
        let workers = Workers::default();
        workers.spawn("stubborn", |_shutdown| {
            thread::sleep(Duration::from_millis(500));
        });
        let Stragglers(stragglers) = workers.shutdown_and_join(Duration::from_millis(100));
        assert_eq!(stragglers, vec!["stubborn".to_string()]);
    }
}
