//! The kernel: a worker thread that owns a [`Runner`] so the UI never blocks on computation.
//!
//! The thread has a large stack (deep but bounded recursion in the algebra), survives panics by resetting its state
//! and reporting [`Event::Reset`], and can be interrupted at any time.

use crate::notebook::{CellId, Job};
use crate::runner::{CellResult, Event, JobResult, Runner, execute};
use mathcraft_eval::EvalError;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

enum Cmd {
    Run(Vec<Job>),
    Forget(CellId),
    Reset,
    Shutdown,
}

pub struct Kernel {
    tx: Sender<Cmd>,
    pub events: Receiver<Event>,
    cancel: Arc<AtomicBool>,
    busy: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

const KERNEL_STACK: usize = 256 * 1024 * 1024;

impl Kernel {
    /// Start the kernel thread. `wake` is called after every event so a UI can repaint.
    pub fn spawn(wake: impl Fn() + Send + 'static) -> std::io::Result<Kernel> {
        let (tx, rx) = channel::<Cmd>();
        let (etx, events) = channel::<Event>();
        let cancel = Arc::new(AtomicBool::new(false));
        let busy = Arc::new(AtomicBool::new(false));
        let (c2, b2) = (cancel.clone(), busy.clone());
        let handle = std::thread::Builder::new().name("mathcraft-kernel".into()).stack_size(KERNEL_STACK).spawn(move || {
            let mut runner = Runner::new(c2.clone());
            while let Ok(cmd) = rx.recv() {
                match cmd {
                    Cmd::Forget(id) => runner.forget_cell(id),
                    Cmd::Reset => runner.reset(),
                    Cmd::Shutdown => break,
                    Cmd::Run(jobs) => {
                        b2.store(true, Ordering::Relaxed);
                        let current: Mutex<Option<CellId>> = Mutex::new(None);
                        let outcome = catch_unwind(AssertUnwindSafe(|| {
                            execute(&mut runner, jobs, &mut |ev| {
                                if let (Event::Started(id), Ok(mut cur)) = (&ev, current.lock()) {
                                    *cur = Some(*id);
                                }
                                let _ = etx.send(ev);
                                wake();
                            });
                        }));
                        b2.store(false, Ordering::Relaxed);
                        if outcome.is_err() {
                            // A bug in the engine. Report it against the running cell and start over with a clean kernel.
                            let id = current.lock().ok().and_then(|c| *c);
                            runner.reset();
                            if let Some(id) = id {
                                let error = EvalError::new("internal error: the computation engine crashed. Please report this; the notebook state was reset");
                                let _ = etx.send(Event::Finished(
                                    id,
                                    JobResult::Done(CellResult { stdout: String::new(), result: Err(error), elapsed: Duration::ZERO }),
                                ));
                            }
                            let _ = etx.send(Event::Reset);
                            wake();
                        }
                    }
                }
            }
        })?;
        Ok(Kernel { tx, events, cancel, busy, handle: Some(handle) })
    }

    pub fn run(&self, jobs: Vec<Job>) {
        let _ = self.tx.send(Cmd::Run(jobs));
    }

    pub fn forget_cell(&self, id: CellId) {
        let _ = self.tx.send(Cmd::Forget(id));
    }

    pub fn reset(&self) {
        let _ = self.tx.send(Cmd::Reset);
    }

    /// Stop the cell that is running and cancel the ones queued behind it. Does nothing when idle.
    pub fn interrupt(&self) {
        if self.busy.load(Ordering::Relaxed) {
            self.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Relaxed)
    }
}

impl Drop for Kernel {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        let _ = self.tx.send(Cmd::Shutdown);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}
