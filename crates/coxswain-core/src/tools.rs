//! Programs installed on the machine that Coxswain can use: found the way `which` finds them
//! (plus the folders their installers use off PATH), and run with a time limit.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

/// Where installers put these outside PATH.
fn extra_paths(program: &str) -> Vec<PathBuf> {
    let v: &[&str] = match program {
        "soffice" if cfg!(target_os = "macos") => &["/Applications/LibreOffice.app/Contents/MacOS/soffice"],
        "soffice" if cfg!(windows) => &[r"C:\Program Files\LibreOffice\program\soffice.exe"],
        "tesseract" if cfg!(windows) => &[r"C:\Program Files\Tesseract-OCR\tesseract.exe"],
        _ => &[],
    };
    v.iter().map(PathBuf::from).collect()
}

/// A program on PATH (or in its usual install folder), like `which`.
pub fn which(program: &str) -> Option<PathBuf> {
    let names = if cfg!(windows) { vec![format!("{program}.exe"), format!("{program}.cmd"), format!("{program}.bat")] } else { vec![program.to_string()] };
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
        .chain(extra_paths(program))
        .find(|p| p.is_file())
}

/// Wait for `child`, killing it when it runs past `timeout`.
pub fn wait(child: &mut Child, timeout: Duration) -> std::io::Result<ExitStatus> {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(std::io::ErrorKind::TimedOut, "took too long"));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// `program` with `args` at the lowest priority, no input, its output kept up to `max` bytes;
/// `None` when it fails or runs past `timeout`.
pub fn output(program: &std::path::Path, args: &[&std::ffi::OsStr], timeout: Duration, max: u64) -> Option<Vec<u8>> {
    let mut c = low(program);
    c.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = c.spawn().ok()?;
    let mut out = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut v = vec![];
        let _ = (&mut out).take(max).read_to_end(&mut v);
        // Whatever is past the limit is drained, so the program is not stuck writing it.
        let _ = std::io::copy(&mut out, &mut std::io::sink());
        v
    });
    let status = wait(&mut child, timeout).ok()?;
    let v = reader.join().ok()?;
    status.success().then_some(v)
}

/// `program` at the lowest priority the platform offers.
pub fn low(program: &std::path::Path) -> Command {
    match which("nice").filter(|_| cfg!(unix)) {
        Some(nice) => {
            let mut c = Command::new(nice);
            c.args(["-n", "19"]).arg(program);
            c
        }
        None => {
            #[cfg_attr(not(windows), allow(unused_mut))]
            let mut c = Command::new(program);
            #[cfg(windows)]
            // IDLE_PRIORITY_CLASS | CREATE_NO_WINDOW
            std::os::windows::process::CommandExt::creation_flags(&mut c, 0x0000_0040 | 0x0800_0000);
            c
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_run_with_a_limit() {
        assert_eq!(which("no-such-program-coxswain"), None);
        if cfg!(unix) {
            let sh = which("sh").unwrap();
            let args = |s: &str| [std::ffi::OsStr::new("-c"), std::ffi::OsStr::new(s)].map(|a| a.to_owned());
            let run = |s: &str, t: u64, max: u64| output(&sh, &args(s).iter().map(|a| a.as_os_str()).collect::<Vec<_>>(), Duration::from_millis(t), max);
            assert_eq!(run("echo hello", 5000, 100), Some(b"hello\n".to_vec()));
            assert_eq!(run("yes | head -c 100000", 5000, 10).map(|v| v.len()), Some(10), "cut at the limit, the rest drained");
            assert_eq!(run("exit 3", 5000, 100), None);
            let start = Instant::now();
            assert_eq!(run("sleep 10", 200, 100), None);
            assert!(start.elapsed() < Duration::from_secs(5), "stopped at the time limit");
        }
    }
}
