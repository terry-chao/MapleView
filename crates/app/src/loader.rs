//! Background decode pool.
//!
//! Two things make browsing feel instant:
//!
//! * **Cancellation.** Every visible request bumps a generation counter. A worker
//!   that finishes decoding a frame the user has already navigated away from
//!   throws the result away instead of uploading it.
//! * **Prefetch.** Neighbours are decoded into the cache on a separate thread, so
//!   they never queue behind the image the user is actually waiting for.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::JoinHandle;

use crossbeam_channel::{Receiver, Sender, unbounded};
use mapleview_core::{DecodeHint, Decoded, ImageCache, decode_file_with};

/// Called from worker threads to wake the UI when something finishes.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// How many images a single user action may keep decoding at once.
const MAX_WORKERS: usize = 6;

enum Job {
    /// A request the user is waiting for; reports back and is cancellable.
    Load {
        generation: u64,
        path: PathBuf,
        hint: DecodeHint,
    },
    /// A speculative decode that only warms the cache.
    Prefetch { path: PathBuf, hint: DecodeHint },
}

/// A finished load attempt.
pub struct Outcome {
    pub generation: u64,
    pub path: PathBuf,
    pub result: Result<Arc<Decoded>, String>,
}

pub struct Loader {
    jobs: Sender<Job>,
    prefetches: Sender<Job>,
    results: Receiver<Outcome>,
    generation: Arc<AtomicU64>,
    workers: Vec<JoinHandle<()>>,
}

impl Loader {
    pub fn new(cache: ImageCache, waker: Waker) -> Self {
        let (job_tx, job_rx) = unbounded();
        let (prefetch_tx, prefetch_rx) = unbounded();
        let (result_tx, result_rx) = unbounded();
        let generation = Arc::new(AtomicU64::new(0));

        let mut workers = Vec::with_capacity(worker_count() + 1);
        for index in 0..worker_count() {
            workers.push(spawn_worker(
                format!("mapleview-decode-{index}"),
                job_rx.clone(),
                result_tx.clone(),
                cache.clone(),
                Arc::clone(&generation),
                Arc::clone(&waker),
            ));
        }
        workers.push(spawn_worker(
            "mapleview-prefetch".to_owned(),
            prefetch_rx,
            result_tx,
            cache,
            Arc::clone(&generation),
            waker,
        ));

        Self {
            jobs: job_tx,
            prefetches: prefetch_tx,
            results: result_rx,
            generation,
            workers,
        }
    }

    /// Queues a load, cancelling every outstanding one.
    pub fn request(&self, path: PathBuf, hint: DecodeHint) -> u64 {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.jobs.send(Job::Load {
            generation,
            path,
            hint,
        });
        generation
    }

    /// Queues a speculative decode that only warms the cache.
    pub fn prefetch(&self, path: PathBuf, hint: DecodeHint) {
        let _ = self.prefetches.send(Job::Prefetch { path, hint });
    }

    pub fn poll(&self) -> Option<Outcome> {
        self.results.try_recv().ok()
    }

    /// Drops every outstanding request without queueing a replacement.
    pub fn cancel(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }
}

impl Drop for Loader {
    fn drop(&mut self) {
        // Closing the channels lets the workers observe a disconnect and exit.
        self.cancel();
        let (dead_tx, _) = unbounded::<Job>();
        self.jobs = dead_tx.clone();
        self.prefetches = dead_tx;
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

fn worker_count() -> usize {
    (num_cpus::get() / 2).clamp(2, MAX_WORKERS)
}

fn spawn_worker(
    name: String,
    jobs: Receiver<Job>,
    results: Sender<Outcome>,
    cache: ImageCache,
    generation: Arc<AtomicU64>,
    waker: Waker,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name(name)
        .spawn(move || {
            while let Ok(job) = jobs.recv() {
                match job {
                    Job::Load {
                        generation: request_id,
                        path,
                        hint,
                    } => {
                        if is_stale(request_id, &generation) {
                            continue;
                        }

                        let result = match cache.get(&path, hint.target) {
                            Some(hit) => Ok(hit),
                            None => decode_file_with(&path, hint)
                                .map_err(|error| error.to_string())
                                .map(|decoded| cache.insert(decoded)),
                        };

                        if is_stale(request_id, &generation) {
                            continue;
                        }
                        let _ = results.send(Outcome {
                            generation: request_id,
                            path,
                            result,
                        });
                        waker();
                    }
                    Job::Prefetch { path, hint } => {
                        if cache.get(&path, hint.target).is_some() {
                            continue;
                        }
                        if let Ok(decoded) = decode_file_with(&path, hint) {
                            cache.insert(decoded);
                        }
                    }
                }
            }
        })
        .expect("failed to spawn a decode worker")
}

fn is_stale(generation: u64, current: &AtomicU64) -> bool {
    generation != current.load(Ordering::SeqCst)
}
