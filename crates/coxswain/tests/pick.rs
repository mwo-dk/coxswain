//! `coxswain --pick` with standard output piped: the panels are not drawn there, and without a
//! terminal at all it says so (exit 2) and prints nothing.

#[cfg(unix)]
#[test]
fn a_pipe_is_not_refused_but_no_terminal_is() {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    let home = std::env::temp_dir().join(format!("coxswain-test-pick-home-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_coxswain"));
    cmd.arg("--pick").env("HOME", &home).env("XDG_CONFIG_HOME", home.join("c")).env("XDG_CACHE_HOME", home.join("k")).env("XDG_DATA_HOME", home.join("d")).env("XDG_STATE_HOME", home.join("s"));
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    // A session of its own: no controlling terminal, so /dev/tty cannot be opened.
    // SAFETY: setsid only changes the child's session.
    unsafe { cmd.pre_exec(|| if libc_setsid() < 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) }) };
    let out = cmd.output().unwrap();
    let _ = std::fs::remove_dir_all(&home);
    assert_eq!(out.status.code(), Some(2), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(out.stdout.is_empty(), "nothing but paths goes to stdout");
    assert!(String::from_utf8_lossy(&out.stderr).contains("terminal"), "{}", String::from_utf8_lossy(&out.stderr));
}

#[cfg(unix)]
fn libc_setsid() -> i32 {
    unsafe extern "C" {
        fn setsid() -> i32;
    }
    // SAFETY: plain system call.
    unsafe { setsid() }
}
