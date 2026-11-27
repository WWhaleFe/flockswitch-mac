//! Wake from sleep and screen unlock (wake_macos.m): the network thread looks for the
//! other computers again at once instead of waiting for its own timers.

use std::sync::atomic::{AtomicBool, Ordering};

static PENDING: AtomicBool = AtomicBool::new(false);

extern "C" {
    fn fs_wake_observe(callback: extern "C" fn());
}

extern "C" fn on_event() {
    notify();
}

/// Starts listening. Call from the main thread once the AppKit event loop is running.
pub fn start() {
    // Safety: registers a plain C callback that only sets a flag.
    unsafe { fs_wake_observe(on_event) }
}

/// Records that the Mac woke up or was unlocked.
pub fn notify() {
    PENDING.store(true, Ordering::Release);
}

/// True once per wake-up or unlock.
pub fn take() -> bool {
    PENDING.swap(false, Ordering::AcqRel)
}

#[cfg(test)]
mod tests {
    #[test]
    fn an_event_is_taken_once() {
        super::take();
        assert!(!super::take());
        super::notify();
        super::notify();
        assert!(super::take());
        assert!(!super::take());
    }
}
