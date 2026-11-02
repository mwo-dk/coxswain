//! Stop for a running file operation (copy, move, delete, extract, pack): one flag, set by the
//! app's Stop, looked at between files and inside a large file's copy, so a Stop is answered
//! within a second. `undo::run` holds it for the thread it works on (`during`), so the file
//! code deep below it (`fs`, `archive`) asks `check` without a flag passed down every call.

use std::cell::RefCell;
use std::io::{self, Read};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

thread_local! {
    static NOW: RefCell<Option<Arc<AtomicBool>>> = const { RefCell::new(None) };
    static BYTES: RefCell<Option<Arc<Bytes>>> = const { RefCell::new(None) };
}

/// How far the file operation on a thread is in bytes, once it knows how many there are in all
/// (an extract: its archive's files, or a tar's packed bytes): counted by `Reader` as they
/// come, so one large file shows how far it is too, its rate and the time left.
#[derive(Debug, Default)]
pub struct Bytes {
    done: AtomicU64,
    all: AtomicU64,
    since: std::sync::Mutex<Option<Instant>>,
}

impl Bytes {
    /// How far: the bytes done and of how many, `None` while that is not known.
    pub fn now(&self) -> Option<(u64, u64)> {
        let all = self.all.load(Ordering::Relaxed);
        (all > 0).then(|| (self.done.load(Ordering::Relaxed).min(all), all))
    }

    /// "1.2 GB of 4.5 GB · 80.0 MB/s · 2 min left"; the rate and the time left once a second
    /// has gone by. `None` while the total is not known.
    pub fn line(&self) -> Option<String> {
        let (done, all) = self.now()?;
        let of = crate::t!("job.bytes", "done" => crate::settings::human(done), "all" => crate::settings::human(all));
        let secs = self.since.lock().unwrap_or_else(|e| e.into_inner()).map_or(0.0, |s| s.elapsed().as_secs_f64());
        if secs < 1.0 || done == 0 {
            return Some(of);
        }
        let rate = done as f64 / secs;
        let left = ((all - done) as f64 / rate).ceil() as u64;
        Some(crate::t!("job.bytes_rate", "of" => of, "rate" => crate::settings::human(rate as u64), "left" => crate::transcribe::span(left)))
    }
}

/// `f`, with `bytes` counting what this thread's `Reader`s read while it runs.
pub(crate) fn counting<T>(bytes: &Arc<Bytes>, f: impl FnOnce() -> T) -> T {
    struct Reset(Option<Arc<Bytes>>);
    impl Drop for Reset {
        fn drop(&mut self) {
            BYTES.with(|b| *b.borrow_mut() = self.0.take());
        }
    }
    let _reset = Reset(BYTES.with(|b| b.borrow_mut().replace(bytes.clone())));
    f()
}

/// From here on this thread's operation counts `all` bytes, none done yet.
pub(crate) fn expect(all: u64) {
    BYTES.with(|b| {
        if let Some(b) = b.borrow().as_ref() {
            b.done.store(0, Ordering::Relaxed);
            b.all.store(all, Ordering::Relaxed);
            *b.since.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
        }
    });
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
    LOOK.with(|l| l.borrow().as_ref().map(|f| f()));
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
    /// A test's look at what is on its way, at each Stop look.
    pub(crate) static LOOK: RefCell<Option<Box<dyn Fn()>>> = const { RefCell::new(None) };
}

/// What `check` fails with: told apart from other errors by `is`, also when wrapped.
#[derive(Debug)]
struct Stopped;

impl std::fmt::Display for Stopped {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&crate::t!("stop.stopped"))
    }
}

impl std::error::Error for Stopped {}

/// An error once Stop was pressed: what is on its way stops there.
pub(crate) fn check() -> io::Result<()> {
    if asked() {
        Err(io::Error::other(Stopped))
    } else {
        Ok(())
    }
}

/// Whether `e` is Stop's own error (`check`), however deep another error wraps it.
pub(crate) fn is(e: &io::Error) -> bool {
    let mut at: Option<&(dyn std::error::Error + 'static)> = e.get_ref().map(|r| r as _);
    while let Some(err) = at {
        if err.is::<Stopped>() {
            return true;
        }
        // An io::Error's `source` skips the error it holds: that is looked at first.
        at = match err.downcast_ref::<io::Error>() {
            Some(io) => io.get_ref().map(|r| r as _),
            None => err.source(),
        };
    }
    false
}

/// A reader that fails once Stop is pressed, so a copy through it (`io::copy`, an archive
/// written or unpacked) stops inside a large file.
pub(crate) struct Reader<R>(pub R);

impl<R: Read> Read for Reader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        check()?;
        let n = self.0.read(buf)?;
        BYTES.with(|b| b.borrow().as_ref().map(|b| b.done.fetch_add(n as u64, Ordering::Relaxed)));
        Ok(n)
    }
}
