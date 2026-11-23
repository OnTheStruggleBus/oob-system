//! [`StoreHandle`]: how tokio callers reach the single writer.
//!
//! [`Store::spawn_writer`] moves the store onto a dedicated thread. Callers
//! send closures over a bounded channel and get the result back on a
//! oneshot; the closures run one at a time, so there is exactly one writer.

use std::thread::JoinHandle;

use tokio::sync::{mpsc, oneshot};

use super::{Store, StoreError};

type Job = Box<dyn FnOnce(&mut Store) + Send>;

/// A cloneable handle to the store's writer thread. The thread exits when
/// the last handle is dropped.
#[derive(Clone)]
pub struct StoreHandle {
    jobs: mpsc::Sender<Job>,
}

impl Store {
    /// Moves the store onto its writer thread; `queue` bounds the number of
    /// waiting jobs (back-pressure for callers).
    pub fn spawn_writer(self, queue: usize) -> std::io::Result<(StoreHandle, JoinHandle<()>)> {
        let (jobs, mut rx) = mpsc::channel::<Job>(queue.max(1));
        let thread = std::thread::Builder::new()
            .name("oob-dac-store".into())
            .spawn(move || {
                let mut store = self;
                while let Some(job) = rx.blocking_recv() {
                    job(&mut store);
                }
            })?;
        Ok((StoreHandle { jobs }, thread))
    }
}

impl StoreHandle {
    fn job<T, F>(f: F) -> (Job, oneshot::Receiver<Result<T, StoreError>>)
    where
        F: FnOnce(&mut Store) -> Result<T, StoreError> + Send + 'static,
        T: Send + 'static,
    {
        let (reply, rx) = oneshot::channel();
        let job: Job = Box::new(move |store| {
            // The caller may have gone away; nothing to do then.
            let _ = reply.send(f(store));
        });
        (job, rx)
    }

    /// Runs `f` on the writer thread and awaits its result.
    pub async fn call<T, F>(&self, f: F) -> Result<T, StoreError>
    where
        F: FnOnce(&mut Store) -> Result<T, StoreError> + Send + 'static,
        T: Send + 'static,
    {
        let (job, rx) = Self::job(f);
        self.jobs
            .send(job)
            .await
            .map_err(|_| StoreError::WriterGone)?;
        rx.await.map_err(|_| StoreError::WriterGone)?
    }

    /// Like [`call`](Self::call), for synchronous callers. Must not be
    /// called from inside an async runtime.
    pub fn call_blocking<T, F>(&self, f: F) -> Result<T, StoreError>
    where
        F: FnOnce(&mut Store) -> Result<T, StoreError> + Send + 'static,
        T: Send + 'static,
    {
        let (job, rx) = Self::job(f);
        self.jobs
            .blocking_send(job)
            .map_err(|_| StoreError::WriterGone)?;
        rx.blocking_recv().map_err(|_| StoreError::WriterGone)?
    }
}
