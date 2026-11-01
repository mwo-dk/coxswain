//! Fetch and Pull (`remote::run`) against real Jujutsu, Subversion and CVS repositories made here:
//! a jj clone of a bare git repository, an svnadmin repository over `file://` and served by
//! `svnserve` with a password wanted, a local CVSROOT and the same served by `cvs pserver`. Each
//! part is skipped, and says so, where its tool is missing; CI installs them on Linux. The test
//! runs in a home of its own, as jj, svn and cvs write their settings into the home.
#![cfg(unix)]

use std::net::{TcpListener, TcpStream};
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use coxswain_core::remote::{self, Op};
use coxswain_core::t;

fn installed(program: &str) -> bool {
    Command::new(program).arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

/// `program args` in `dir`, which must succeed: what it printed.
fn sh(dir: &Path, program: &str, args: &[&str]) -> String {
    let o = Command::new(program).current_dir(dir).args(args).stdin(Stdio::null()).output().unwrap();
    assert!(o.status.success(), "{program} {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// `remote::run`, with a limit only against a hang (a server that never answers).
fn go(dir: &Path, op: Op) -> std::io::Result<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let dir = dir.to_path_buf();
    std::thread::spawn(move || tx.send(remote::run(&dir, op, &AtomicBool::new(false), &mut |_| {})));
    rx.recv_timeout(coxswain_core::test_limit(std::time::Duration::from_secs(60))).expect("the tool hangs")
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap()
}

/// A server on a port of its own: each connection gets the program `cmd` makes, the connection
/// as its stdin and stdout (as inetd runs `svnserve -i` and `cvs pserver`). Dropped, the port
/// closes: nothing answers there any more.
struct Served {
    port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

fn serve(cmd: impl Fn() -> Command + Send + 'static) -> Served {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let thread = std::thread::spawn(move || {
        let mut children: Vec<Child> = vec![];
        for s in l.incoming() {
            if flag.load(Ordering::Relaxed) {
                break;
            }
            let Ok(s) = s else { continue };
            let fd = OwnedFd::from(s);
            let out = fd.try_clone().unwrap();
            if let Ok(c) = cmd().stdin(Stdio::from(fd)).stdout(Stdio::from(out)).stderr(Stdio::null()).spawn() {
                children.push(c);
            }
        }
        for mut c in children {
            let _ = c.kill();
            let _ = c.wait();
        }
    });
    Served { port, stop, thread: Some(thread) }
}

impl Drop for Served {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        let _ = self.thread.take().map(|t| t.join());
    }
}

#[test]
fn fetch_and_pull_against_real_servers() {
    let t: PathBuf = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("vcs-remote-{}", std::process::id()));
    let home = t.join("home");
    std::fs::create_dir_all(&home).unwrap();
    // SAFETY: the only test of this binary, before it starts a thread.
    unsafe {
        std::env::set_var("HOME", &home);
        std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
        std::env::set_var("XDG_CACHE_HOME", home.join(".cache"));
        std::env::set_var("XDG_DATA_HOME", home.join(".local/share"));
        std::env::set_var("GIT_CONFIG_GLOBAL", "/dev/null");
        std::env::set_var("GIT_CONFIG_NOSYSTEM", "1");
        std::env::set_var("LC_ALL", "C");
    }
    let mut ran = vec![];
    if installed("jj") && installed("git") {
        jujutsu(&t.join("jj"));
        ran.push("Jujutsu");
    } else {
        eprintln!("SKIPPED: Jujutsu (jj or git is not installed)");
    }
    if installed("svn") && installed("svnadmin") && installed("svnserve") {
        subversion(&t.join("svn"));
        ran.push("Subversion");
    } else {
        eprintln!("SKIPPED: Subversion (svn, svnadmin or svnserve is not installed)");
    }
    if installed("cvs") {
        cvs(&t.join("cvs"));
        ran.push("CVS");
    } else {
        eprintln!("SKIPPED: CVS (cvs is not installed)");
    }
    eprintln!("tried: {ran:?}");
    let _ = std::fs::remove_dir_all(&t);
}

fn git(dir: &Path, args: &[&str]) -> String {
    let mut all = vec!["-c", "user.name=Ada", "-c", "user.email=ada@example.org", "-c", "init.defaultBranch=main"];
    all.extend_from_slice(args);
    sh(dir, "git", &all)
}

fn jujutsu(t: &Path) {
    std::fs::create_dir_all(t).unwrap();
    git(t, &["init", "-q", "--bare", "remote.git"]);
    git(t, &["clone", "-q", "remote.git", "there"]);
    let there = t.join("there");
    let push = |file: &str| {
        std::fs::write(there.join(file), file).unwrap();
        git(&there, &["add", "-A"]);
        git(&there, &["commit", "-q", "-m", file]);
        git(&there, &["push", "-q", "origin", "HEAD:main"]);
        git(&there, &["rev-parse", "HEAD"])
    };
    push("a.txt");
    sh(t, "jj", &["git", "clone", "remote.git", "j"]);
    let j = t.join("j");
    let main = || sh(&j, "jj", &["--no-pager", "log", "-r", "main", "--no-graph", "-T", "commit_id"]);
    let parent = || sh(&j, "jj", &["--no-pager", "log", "-r", "@-", "--no-graph", "-T", "commit_id"]);
    let start = parent();

    let new = push("b.txt");
    assert_eq!(go(&j, Op::Fetch).unwrap(), t!("remote.fetched_jj"));
    assert_eq!(main(), new, "the fetch moves the tracked bookmark");
    assert!(!j.join("b.txt").exists(), "a fetch leaves the working copy");

    let new = push("c.txt");
    assert_eq!(go(&j, Op::Pull).unwrap(), t!("remote.pulled_jj"));
    assert_eq!(main(), new);
    assert_eq!(parent(), start, "the working copy stays where it is: moving it is a rebase");
    assert!(!j.join("c.txt").exists());

    // Nothing there: jj's own words come back.
    let gone = t.join("gone.git");
    sh(&j, "jj", &["git", "remote", "set-url", "origin", gone.to_str().unwrap()]);
    let e = go(&j, Op::Fetch).unwrap_err().to_string();
    assert!(e.contains("gone.git"), "{e}");
}

fn subversion(t: &Path) {
    std::fs::create_dir_all(t).unwrap();
    sh(t, "svnadmin", &["create", "repo"]);
    let url = format!("file://{}", t.join("repo").display());
    let svn = |dir: &Path, args: &[&str]| {
        let mut all = vec!["--non-interactive"];
        all.extend_from_slice(args);
        sh(dir, "svn", &all)
    };
    svn(t, &["checkout", "-q", &url, "a"]);
    svn(t, &["checkout", "-q", &url, "b"]);
    let (a, b) = (t.join("a"), t.join("b"));
    std::fs::write(a.join("f.txt"), "one\n").unwrap();
    svn(&a, &["add", "-q", "f.txt"]);
    svn(&a, &["commit", "-q", "-m", "one"]);

    let e = go(&b, Op::Fetch).unwrap_err();
    assert_eq!(e.kind(), std::io::ErrorKind::Unsupported);
    assert_eq!(e.to_string(), t!("remote.no_fetch"));
    let said = go(&b, Op::Pull).unwrap();
    assert!(said.contains('1'), "svn's last line names the revision: {said}");
    assert_eq!(read(&b.join("f.txt")), "one\n");

    // Changed here and on the server: refused, nothing written.
    std::fs::write(a.join("f.txt"), "two\n").unwrap();
    svn(&a, &["commit", "-q", "-m", "two"]);
    std::fs::write(b.join("f.txt"), "mine\n").unwrap();
    assert_eq!(go(&b, Op::Pull).unwrap_err().to_string(), t!("remote.would_touch", "files" => "f.txt"));
    assert_eq!(read(&b.join("f.txt")), "mine\n");
    // Changed here only, another file new on the server: the update goes ahead around it.
    svn(&b, &["revert", "-q", "f.txt"]);
    svn(&b, &["update", "-q"]);
    std::fs::write(b.join("f.txt"), "mine\n").unwrap();
    std::fs::write(a.join("g.txt"), "new\n").unwrap();
    svn(&a, &["add", "-q", "g.txt"]);
    svn(&a, &["commit", "-q", "-m", "g"]);
    go(&b, Op::Pull).unwrap();
    assert_eq!(read(&b.join("g.txt")), "new\n");
    assert_eq!(read(&b.join("f.txt")), "mine\n");

    // A server that wants a password the working copy has not kept: svn's words come back.
    std::fs::write(t.join("repo/conf/svnserve.conf"), "[general]\nanon-access = none\nauth-access = write\npassword-db = passwd\n").unwrap();
    std::fs::write(t.join("repo/conf/passwd"), "[users]\nada = secret\n").unwrap();
    let root = t.join("repo");
    let server = serve(move || {
        let mut c = Command::new("svnserve");
        c.arg("-i").arg("-r").arg(&root);
        c
    });
    let at = format!("svn://127.0.0.1:{}/", server.port);
    svn(t, &["checkout", "-q", "--no-auth-cache", "--username", "ada", "--password", "secret", &at, "c"]);
    let c = t.join("c");
    std::fs::write(a.join("h.txt"), "new\n").unwrap();
    svn(&a, &["add", "-q", "h.txt"]);
    svn(&a, &["commit", "-q", "-m", "h"]);
    let e = go(&c, Op::Pull).unwrap_err().to_string();
    assert!(e.starts_with("svn: E"), "the failing lines first: {e}");
    assert!(e.contains("E170001"), "authentication: {e}");
    assert!(!c.join("h.txt").exists());

    // Nothing answers there any more.
    drop(server);
    let e = go(&c, Op::Pull).unwrap_err().to_string();
    assert!(e.contains("E170013"), "unable to connect: {e}");
}

fn cvs(t: &Path) {
    let root = t.join("root");
    let imp = t.join("import");
    std::fs::create_dir_all(&imp).unwrap();
    let cvs = |dir: &Path, args: &[&str]| {
        let mut all = vec!["-f", "-q", "-d", root.to_str().unwrap()];
        all.extend_from_slice(args);
        sh(dir, "cvs", &all)
    };
    cvs(t, &["init"]);
    std::fs::write(imp.join("f.txt"), "one\n").unwrap();
    cvs(&imp, &["import", "-m", "start", "mod", "vendor", "start"]);
    cvs(t, &["checkout", "-d", "a", "mod"]);
    cvs(t, &["checkout", "-d", "b", "mod"]);
    let (a, b) = (t.join("a"), t.join("b"));

    assert_eq!(go(&b, Op::Fetch).unwrap_err().to_string(), t!("remote.no_fetch"));
    std::fs::write(a.join("f.txt"), "two\n").unwrap();
    cvs(&a, &["commit", "-m", "two"]);
    assert_eq!(go(&b, Op::Pull).unwrap(), t!("remote.updated_cvs", "n" => "1"));
    assert_eq!(read(&b.join("f.txt")), "two\n");
    assert_eq!(go(&b, Op::Pull).unwrap(), t!("remote.updated_cvs", "n" => "0"));

    // Changed here and on the server: refused, nothing written.
    std::fs::write(a.join("f.txt"), "three\n").unwrap();
    cvs(&a, &["commit", "-m", "three"]);
    std::fs::write(b.join("f.txt"), "mine\n").unwrap();
    assert_eq!(go(&b, Op::Pull).unwrap_err().to_string(), t!("remote.would_touch", "files" => "f.txt"));
    assert_eq!(read(&b.join("f.txt")), "mine\n");
    std::fs::remove_file(b.join("f.txt")).unwrap();

    // The same repository served by `cvs pserver`, no password given (no `cvs login`): refused
    // with cvs's own words, and nothing written.
    // ada has a password there (a crypt(3) hash no password here gives).
    std::fs::write(root.join("CVSROOT/passwd"), "ada:ab01FAX.bQRSU\n").unwrap();
    let r = root.clone();
    let server = serve(move || {
        let mut c = Command::new("cvs");
        c.arg("-f").arg(format!("--allow-root={}", r.display())).arg("pserver");
        c
    });
    std::fs::write(b.join("CVS/Root"), format!(":pserver:ada@127.0.0.1:{}{}\n", server.port, root.display())).unwrap();
    let e = go(&b, Op::Pull).unwrap_err().to_string();
    // cvs 1.11 says "try \"cvs login\"", 1.12 "Empty password used - try 'cvs login'".
    assert!(e.contains("cvs login") || e.contains("authorization failed"), "{e}");
    assert!(!b.join("f.txt").exists());

    drop(server);
    let e = go(&b, Op::Pull).unwrap_err().to_string();
    assert!(e.contains("127.0.0.1"), "cannot connect: {e}");
    assert!(!b.join("f.txt").exists());
}
