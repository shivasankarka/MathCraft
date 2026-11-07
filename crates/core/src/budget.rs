//! Cooperative limits for long computations: a wall-clock deadline, a cancel flag and an operation counter.
//!
//! The kernel installs a [`Budget`] on its thread; hot loops in algebra and evaluation call [`tick`], which fails
//! with `Cancelled`, `Timeout` or `TooLarge` so nothing can hang the app. With no budget installed, `tick` is free.

use crate::error::{MathError, MathResult};
use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct Budget {
    pub deadline: Option<Instant>,
    pub cancel: Arc<AtomicBool>,
    pub max_ops: u64,
    ops: u64,
}

impl Budget {
    pub fn new(timeout: Option<Duration>, cancel: Arc<AtomicBool>, max_ops: u64) -> Self {
        Budget { deadline: timeout.map(|t| Instant::now() + t), cancel, max_ops, ops: 0 }
    }
}

thread_local! {
    static CURRENT: RefCell<Option<Budget>> = const { RefCell::new(None) };
}

/// Install `budget` for the current thread; restored when the returned guard drops.
pub fn install(budget: Budget) -> Guard {
    let prev = CURRENT.with(|c| c.replace(Some(budget)));
    Guard { prev }
}

pub struct Guard {
    prev: Option<Budget>,
}

impl Drop for Guard {
    fn drop(&mut self) {
        let prev = self.prev.take();
        CURRENT.with(|c| *c.borrow_mut() = prev);
    }
}

/// Count one unit of work and check the limits.
pub fn tick() -> MathResult<()> {
    CURRENT.with(|c| {
        let mut slot = c.borrow_mut();
        let Some(b) = slot.as_mut() else { return Ok(()) };
        b.ops += 1;
        if b.ops > b.max_ops {
            return Err(MathError::too_large("computation exceeded its step budget"));
        }
        if b.ops % 256 == 0 {
            if b.cancel.load(Ordering::Relaxed) {
                return Err(MathError::Cancelled);
            }
            if b.deadline.is_some_and(|d| Instant::now() >= d) {
                return Err(MathError::Timeout);
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_budget_stops_work() {
        let _g = install(Budget::new(None, Arc::new(AtomicBool::new(false)), 10));
        let mut result = Ok(());
        for _ in 0..100 {
            result = tick();
            if result.is_err() {
                break;
            }
        }
        assert!(matches!(result, Err(MathError::TooLarge(_))));
    }

    #[test]
    fn no_budget_is_free() {
        for _ in 0..1000 {
            assert!(tick().is_ok());
        }
    }
}
