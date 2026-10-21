//! Stop for a running file operation (copy, move, delete, extract, pack): one flag, set by the
//! app's Stop, looked at between files and inside a large file's copy, so a Stop is answered
//! within a second. `undo::run` holds it for the thread it works on (`during`), so the file
//! code deep below it (`fs`, `archive`) asks `check` without a flag passed down every call.

use std::cell::RefCell;
use std::io::{self, Read};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

thread_local! {
    static NOW: RefCell<Option<Arc<AtomicBool>>> = const { RefCell::new(None) };
}

/// `f`, with `stop` as this thread's Stop while it runs.
pub(crate) fn during<T>(stop: &Arc<AtomicBool>, f: impl FnOnce() -> T) -> T {
    struct Reset(Option<Arc<AtomicBool>>);
    impl Drop for Reset {
        fn drop(&mut self) {
            NOW.with(|n| *n.borrow_mut() = self.0.take());
        }
    }
    let _reset = Reset(NOW.with(|n| n.borrow_mut().replace(stop.clone())));
    f()
}

/// Whether Stop was pressed for the operation on this thread.
pub(crate) fn asked() -> bool {
    #[cfg(test)]
    if TRIP.get() > 0 {
        TRIP.set(TRIP.get() - 1);
        if TRIP.get() == 0 {
            NOW.with(|n| n.borrow().as_ref().map(|s| s.store(true, Ordering::Relaxed)));
        }
    }
    NOW.with(|n| n.borrow().as_ref().is_some_and(|s| s.load(Ordering::Relaxed)))
}

#[cfg(test)]
thread_local! {
    /// A test's Stop, pressed at the given look: `asked` counts down to it.
    pub(crate) static TRIP: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// An error once Stop was pressed: what is on its way stops there.
pub(crate) fn check() -> io::Result<()> {
    if asked() {
        Err(io::Error::other(crate::t!("stop.stopped")))
    } else {
        Ok(())
    }
}

/// A reader that fails once Stop is pressed, so a copy through it (`io::copy`, an archive
/// written or unpacked) stops inside a large file.
pub(crate) struct Reader<R>(pub R);

impl<R: Read> Read for Reader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        check()?;
        self.0.read(buf)
    }
}
