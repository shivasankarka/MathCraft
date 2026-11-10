//! A monotonic clock in milliseconds. Native builds read `Instant`; on `wasm32-unknown-unknown`, where `Instant`
//! panics, the host (the page, through the wasm glue) installs its own clock once with [`set_host_clock`].

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use std::sync::OnceLock;
    use std::time::Instant;

    pub fn now_ms() -> f64 {
        static START: OnceLock<Instant> = OnceLock::new();
        START.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use std::sync::OnceLock;

    static HOST: OnceLock<fn() -> f64> = OnceLock::new();

    /// Install the host's clock (milliseconds, e.g. `performance.now()`). Only the first call has an effect.
    pub fn set_host_clock(clock: fn() -> f64) {
        let _ = HOST.set(clock);
    }

    pub fn now_ms() -> f64 {
        HOST.get().map_or(0.0, |clock| clock())
    }
}

pub use imp::*;

#[cfg(test)]
mod tests {
    #[test]
    fn the_clock_does_not_run_backwards() {
        let a = super::now_ms();
        let b = super::now_ms();
        assert!(b >= a);
    }
}
