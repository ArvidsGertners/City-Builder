//! Minimal persistent worker pool (spike stand-in for Bevy's ComputeTaskPool): the caller runs
//! share 0, workers run shares 1..n; spin-then-yield waiting.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

type Job = &'static (dyn Fn(usize) + Sync);

struct Shared {
    epoch: AtomicU64,
    done: AtomicUsize,
    quit: AtomicBool,
    job: Mutex<Option<Job>>,
}

pub struct Pool {
    n: usize,
    shared: Arc<Shared>,
    handles: Vec<JoinHandle<()>>,
}

fn wait_spin(i: &mut u32) {
    *i += 1;
    if *i < 4000 {
        std::hint::spin_loop();
    } else {
        std::thread::yield_now();
    }
}

impl Pool {
    pub fn new(n: usize) -> Self {
        let shared = Arc::new(Shared {
            epoch: AtomicU64::new(0),
            done: AtomicUsize::new(0),
            quit: AtomicBool::new(false),
            job: Mutex::new(None),
        });
        let handles = (1..n)
            .map(|w| {
                let sh = shared.clone();
                std::thread::spawn(move || {
                    let mut seen = 0u64;
                    loop {
                        let mut spins = 0;
                        let e = loop {
                            let e = sh.epoch.load(Ordering::Acquire);
                            if e != seen || sh.quit.load(Ordering::Acquire) {
                                break e;
                            }
                            wait_spin(&mut spins);
                        };
                        if sh.quit.load(Ordering::Acquire) {
                            return;
                        }
                        seen = e;
                        let job = sh.job.lock().map(|j| *j).unwrap_or(None);
                        if let Some(f) = job {
                            f(w);
                        }
                        sh.done.fetch_add(1, Ordering::AcqRel);
                    }
                })
            })
            .collect();
        Pool { n, shared, handles }
    }

    pub fn threads(&self) -> usize {
        self.n
    }

    /// Runs `f(share)` for share in 0..n (share 0 on the caller) and waits for all.
    pub fn run(&self, f: &(dyn Fn(usize) + Sync)) {
        if self.n == 1 {
            f(0);
            return;
        }
        // SAFETY (spike): the reference only lives until every worker has signalled `done`
        // below, before this function returns.
        let job: Job = unsafe { std::mem::transmute::<&(dyn Fn(usize) + Sync), Job>(f) };
        if let Ok(mut j) = self.shared.job.lock() {
            *j = Some(job);
        }
        self.shared.done.store(0, Ordering::Release);
        self.shared.epoch.fetch_add(1, Ordering::AcqRel);
        f(0);
        let mut spins = 0;
        while self.shared.done.load(Ordering::Acquire) < self.n - 1 {
            wait_spin(&mut spins);
        }
        if let Ok(mut j) = self.shared.job.lock() {
            *j = None;
        }
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.shared.quit.store(true, Ordering::Release);
        for h in self.handles.drain(..) {
            let _ = h.join();
        }
    }
}
