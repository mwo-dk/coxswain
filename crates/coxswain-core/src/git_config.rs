//! The git config as git uses it in a folder, read once when the view opens: every key with
//! the value that counts, its scope and the file it comes from, the values set earlier (and so
//! overridden, or one of a key's several values) below it. Secrets are hidden until asked.
//! Coxswain never writes the config (design/adr/0001-version-control-is-shown-not-written.md):
//! "Open in your git tool" is for changing it.

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// What a hidden secret shows as.
pub const HIDDEN: &str = "••••";

/// One key's value, as `git config --list --show-origin --show-scope` gives it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Entry {
    /// The key as git names it: `remote.origin.url`.
    pub key: String,
    /// None for a bare key (`[core] bare` with no `=`), which git reads as true.
    pub value: Option<String>,
    /// The value as shown while it is hidden; None when there is nothing to hide.
    pub hidden: Option<String>,
    /// The heading it is listed under: `core`, `remote "origin"`, with any secret hidden.
    pub section: String,
    /// The key's last part: `url`.
    pub name: String,
    /// `system`, `global`, `local`, `worktree` or `command` (`unknown` for a blob).
    pub scope: String,
    /// The file it comes from; None for the command line, a blob, standard input.
    pub file: Option<PathBuf>,
    /// Where it comes from when no file: "command line", `blob:…`.
    pub origin: String,
    /// The file is included by another (`include.path`, `includeIf`).
    pub included: bool,
    /// Set again later: not the value git uses (for a key with several values, one of them).
    pub earlier: bool,
    /// The line to copy while it is hidden: `key=value` with the secret as `HIDDEN`.
    pub line: String,
}

impl Entry {
    /// The line to copy: `key=value`, as `git -c` takes it; with `shown`, the secret too.
    pub fn copy(&self, shown: bool) -> String {
        if !shown {
            return self.line.clone();
        }
        match &self.value {
            Some(v) => format!("{}={v}", self.key),
            None => self.key.clone(),
        }
    }
}

/// `git config --list` in `dir` (or its nearest folder on disk: a history or an archive is
/// none), every scope with its origin, NUL-separated. Nothing of the repository runs: listing
/// the config runs no hook, filter or monitor. Outside a repository: system and global only.
pub fn command(dir: &Path) -> std::process::Command {
    let dir = dir.ancestors().find(|a| a.is_dir()).unwrap_or(dir);
    let mut c = crate::tools::command("git");
    c.arg("-C")
        .arg(dir)
        .args(["config", "--list", "--show-origin", "--show-scope", "-z"])
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null());
    c
}

/// The config as git uses it in `dir`, by heading and key, each key's winner first.
pub fn read(dir: &Path) -> Result<Vec<Entry>, String> {
    let out = command(dir).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        let said = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if said.is_empty() {
            crate::t!("gitconfig.failed")
        } else {
            said
        });
    }
    let base = dir.ancestors().find(|a| a.is_dir()).unwrap_or(dir);
    Ok(parse(&out.stdout, base))
}

/// The name of the repository `dir` is in (its work tree's folder), for the view's title.
pub fn repo_name(dir: &Path) -> Option<String> {
    let dir = dir.ancestors().find(|a| a.is_dir())?;
    let out = crate::git::command_with(dir, &[])
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let top = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Some(Path::new(&top).file_name()?.to_string_lossy().into_owned())
}

fn is_include(key: &str) -> bool {
    key == "include.path" || (key.starts_with("includeif.") && key.ends_with(".path"))
}

/// git's `-z` output: `scope NUL origin NUL key [LF value] NUL`, in the order git reads it
/// (the last of a key wins). A relative file (`.git/config`) is the repository's, found from
/// `base` up.
pub fn parse(out: &[u8], base: &Path) -> Vec<Entry> {
    let text = String::from_utf8_lossy(out);
    let mut parts = text.split('\0');
    let mut list: Vec<Entry> = vec![];
    // The files being read, the outermost first: an included one is read where its include is.
    let mut files: Vec<String> = vec![];
    let mut last: Option<(String, Option<String>, String)> = None;
    while let (Some(scope), Some(origin), Some(kv)) = (parts.next(), parts.next(), parts.next()) {
        if scope.is_empty() {
            break;
        }
        let (key, value) = match kv.split_once('\n') {
            Some((k, v)) => (k, Some(v.to_string())),
            None => (kv, None),
        };
        if files.last().map(String::as_str) != Some(origin) {
            // Right after an include in the same scope, of a file of that name: included.
            let included = last.as_ref().is_some_and(|(s, v, k)| {
                s == scope
                    && is_include(k)
                    && v.as_deref().and_then(|v| Path::new(v).file_name())
                        == Path::new(origin).file_name()
            });
            if included {
                files.push(origin.to_string());
            } else if let Some(i) = files.iter().position(|f| f == origin) {
                files.truncate(i + 1);
            } else {
                files = vec![origin.to_string()];
            }
        }
        let file = origin.strip_prefix("file:").map(|f| {
            let p = Path::new(f);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                base.ancestors()
                    .map(|a| a.join(p))
                    .find(|a| a.exists())
                    .unwrap_or_else(|| base.join(p))
            }
        });
        let origin_text = match origin.trim_end_matches(':') {
            "command line" => crate::t!("gitconfig.command_line"),
            o => o.to_string(),
        };
        let (key_hidden, hidden) = hide(key, value.as_deref());
        let shown_key = key_hidden.as_deref().unwrap_or(key);
        let (section, name) = heading(shown_key);
        let line = match hidden.as_deref().or(value.as_deref()) {
            Some(v) => format!("{shown_key}={v}"),
            None => shown_key.to_string(),
        };
        list.push(Entry {
            key: key.to_string(),
            value: value.clone(),
            // A hidden key (a URL with a password) hides the line even when the value is plain.
            hidden: hidden.or_else(|| key_hidden.and(value.clone())),
            section,
            name,
            scope: scope.to_string(),
            file,
            origin: origin_text,
            included: files.len() > 1,
            earlier: false,
            line,
        });
        last = Some((scope.to_string(), value, key.to_string()));
    }
    // The last of each key is what git uses; the ones before it are earlier.
    let mut seen = std::collections::HashSet::new();
    for e in list.iter_mut().rev() {
        e.earlier = !seen.insert(e.key.clone());
    }
    // By heading and key, each key's winner first and then the earlier ones, latest first.
    let mut list: Vec<(usize, Entry)> = list.into_iter().enumerate().collect();
    list.sort_by(|(i, a), (j, b)| {
        (&a.section, &a.name, std::cmp::Reverse(i)).cmp(&(
            &b.section,
            &b.name,
            std::cmp::Reverse(j),
        ))
    });
    list.into_iter().map(|(_, e)| e).collect()
}

/// `remote.origin.url` → (`remote "origin"`, `url`); `core.bare` → (`core`, `bare`).
fn heading(key: &str) -> (String, String) {
    let (section, rest) = key.split_once('.').unwrap_or((key, ""));
    match rest.rsplit_once('.') {
        Some((sub, name)) => (format!("{section} \"{sub}\""), name.to_string()),
        None => (section.to_string(), rest.to_string()),
    }
}

/// The key and the value as shown while hidden, each None when it has nothing to hide: the
/// password of a URL's user, an `http.*.extraHeader`'s value (its name stays), and the value
/// of a key whose last part names a password, a token, a secret or a credential.
pub fn hide(key: &str, value: Option<&str>) -> (Option<String>, Option<String>) {
    let key_hidden = hide_urls(key);
    let Some(value) = value else {
        return (key_hidden, None);
    };
    let lower = key.to_ascii_lowercase();
    let last = lower.rsplit('.').next().unwrap_or(&lower);
    let secret = [
        "pass",
        "passwd",
        "password",
        "pwd",
        "token",
        "secret",
        "apikey",
        "credential",
        "credentials",
    ]
    .iter()
    .any(|s| last.ends_with(s));
    let hidden = if secret && !value.is_empty() {
        Some(HIDDEN.to_string())
    } else if last == "extraheader" {
        Some(match value.split_once(':') {
            Some((name, _)) => format!("{name}: {HIDDEN}"),
            None => HIDDEN.to_string(),
        })
    } else if lower.starts_with("credential.") && last == "helper" && value.starts_with('!') && {
        let v = value.to_ascii_lowercase();
        ["password", "token", "secret"]
            .iter()
            .any(|s| v.contains(s))
    } {
        // A helper written out as a shell line that holds the secret itself.
        Some(HIDDEN.to_string())
    } else {
        hide_urls(value)
    };
    (key_hidden, hidden)
}

/// `s` with the password of every URL in it hidden (`https://ada:••••@host`), the user too when
/// it is the only part over http (`https://••••@host`, a token), and a query's tokens
/// (`?access_token=••••`). None when there is none.
fn hide_urls(s: &str) -> Option<String> {
    let mut out = String::new();
    let mut rest = s;
    let mut changed = false;
    while let Some(at) = rest.find("://") {
        let scheme_start = rest[..at]
            .rfind(|c: char| !(c.is_ascii_alphanumeric() || "+-.".contains(c)))
            .map_or(0, |i| i + 1);
        let scheme = rest[scheme_start..at].to_ascii_lowercase();
        out.push_str(&rest[..at + 3]);
        rest = &rest[at + 3..];
        let end = rest
            .find(|c: char| c.is_whitespace() || c == '"' || c == '\'')
            .unwrap_or(rest.len());
        let (url, after) = rest.split_at(end);
        let host_end = url.find('/').unwrap_or(url.len());
        let path = match url[..host_end].rfind('@') {
            Some(i) => {
                let auth = &url[..i];
                match auth.split_once(':') {
                    Some((user, _)) => out.push_str(&format!("{user}:{HIDDEN}")),
                    None if scheme == "http" || scheme == "https" => out.push_str(HIDDEN),
                    None => out.push_str(auth),
                }
                changed |= auth.contains(':') || scheme == "http" || scheme == "https";
                &url[i..]
            }
            None => url,
        };
        match path.split_once('?') {
            Some((before, query)) => {
                out.push_str(before);
                out.push('?');
                let params: Vec<String> = query
                    .split('&')
                    .map(|p| match p.split_once('=') {
                        Some((k, _))
                            if ["token", "secret", "pass", "key", "sig", "auth"]
                                .iter()
                                .any(|s| k.to_ascii_lowercase().contains(s)) =>
                        {
                            changed = true;
                            format!("{k}={HIDDEN}")
                        }
                        _ => p.to_string(),
                    })
                    .collect();
                out.push_str(&params.join("&"));
            }
            None => out.push_str(path),
        }
        rest = after;
    }
    out.push_str(rest);
    changed.then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_config_hides_secrets() {
        let h = |k: &str, v: &str| hide(k, Some(v)).1;
        let some = |s: &str| Some(s.to_string());
        assert_eq!(
            h("remote.origin.url", "https://ada:s3cret@example.org/r.git"),
            some("https://ada:••••@example.org/r.git")
        );
        assert_eq!(
            h("remote.origin.url", "https://ghp_abc@github.com/a/b"),
            some("https://••••@github.com/a/b")
        );
        // An ssh user and an scp-like address are no secret.
        assert_eq!(h("remote.origin.url", "ssh://git@host:22/a@b"), None);
        assert_eq!(h("remote.origin.url", "git@github.com:a/b.git"), None);
        assert_eq!(h("remote.origin.url", "https://github.com/a/b"), None);
        assert_eq!(
            h("remote.o.url", "https://h/a.git?access_token=t&x=1"),
            some("https://h/a.git?access_token=••••&x=1")
        );
        assert_eq!(
            h("http.https://h/.extraheader", "Authorization: Bearer abc"),
            some("Authorization: ••••")
        );
        assert_eq!(h("http.extraHeader", "abc"), some("••••"));
        for k in [
            "sendemail.smtpPass",
            "x.apiToken",
            "a.b.SECRET",
            "c.password",
            "d.credential",
        ] {
            assert_eq!(h(k, "v"), some("••••"), "{k}");
        }
        assert_eq!(h("credential.helper", "store"), None);
        assert_eq!(
            h("credential.helper", "!f() { echo password=x; }; f"),
            some("••••")
        );
        assert_eq!(h("user.name", "Ada"), None);
        assert_eq!(h("core.passive", "x"), None);
        // A password in the key itself (`url.<base>.insteadOf`).
        assert_eq!(
            hide("url.https://u:p@h/.insteadof", Some("gh:")).0,
            some("url.https://u:••••@h/.insteadof")
        );
        assert_eq!(hide("core.bare", None), (None, None));
    }

    fn z(parts: &[&str]) -> Vec<u8> {
        parts.iter().flat_map(|p| p.bytes().chain([0])).collect()
    }

    #[test]
    fn git_config_parse_wins_includes_and_order() {
        let out = z(&[
            "system",
            "file:/etc/gitconfig",
            "user.name\nSys",
            "global",
            "file:/h/.gitconfig",
            "include.path\nextra.inc",
            "global",
            "file:/h/extra.inc",
            "core.pager\nless",
            "global",
            "file:/h/.gitconfig",
            "user.name\nGlo",
            "local",
            "file:/r/.git/config",
            "remote.origin.url\nhttps://u:pw@h/r",
            "local",
            "file:/r/.git/config",
            "remote.origin.fetch\n+a",
            "local",
            "file:/r/.git/config",
            "remote.origin.fetch\n+b",
            "local",
            "file:/r/.git/config",
            "core.bare",
            "command",
            "command line:",
            "user.name\nCmd",
        ]);
        let v = parse(&out, Path::new("/r"));
        let rows: Vec<(&str, &str, Option<&str>, bool)> = v
            .iter()
            .map(|e| {
                (
                    e.section.as_str(),
                    e.name.as_str(),
                    e.value.as_deref(),
                    e.earlier,
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                ("core", "bare", None, false),
                ("core", "pager", Some("less"), false),
                ("include", "path", Some("extra.inc"), false),
                ("remote \"origin\"", "fetch", Some("+b"), false),
                ("remote \"origin\"", "fetch", Some("+a"), true),
                ("remote \"origin\"", "url", Some("https://u:pw@h/r"), false),
                ("user", "name", Some("Cmd"), false),
                ("user", "name", Some("Glo"), true),
                ("user", "name", Some("Sys"), true),
            ]
        );
        let pager = &v[1];
        assert!(pager.included && pager.scope == "global");
        assert_eq!(pager.file.as_deref(), Some(Path::new("/h/extra.inc")));
        // Back in .gitconfig after the include: not included.
        assert!(!v[7].included);
        let cmd = &v[6];
        assert_eq!((cmd.scope.as_str(), cmd.file.as_ref()), ("command", None));
        let url = &v[5];
        assert_eq!(url.hidden.as_deref(), Some("https://u:••••@h/r"));
        assert_eq!(url.copy(false), "remote.origin.url=https://u:••••@h/r");
        assert_eq!(url.copy(true), "remote.origin.url=https://u:pw@h/r");
        assert_eq!(v[0].copy(false), "core.bare");
    }

    /// The real git, with the system and global config pointed at files of the test's own,
    /// never the user's.
    #[test]
    fn git_config_read_a_repository() {
        let Some(d) = crate::history::tests::repo("gitconfig") else {
            return;
        };
        let global = d.with_extension("global");
        std::fs::write(
            &global,
            "[user]\n\tname = Global\n[sendemail]\n\tsmtpPass = hush\n",
        )
        .unwrap();
        let git = |args: &[&str]| {
            let ok = crate::tools::command("git")
                .arg("-C")
                .arg(&d)
                .args(args)
                .status()
                .unwrap();
            assert!(ok.success());
        };
        git(&["config", "user.name", "Local"]);
        let run = |dir: &Path| {
            let out = command(dir)
                .env("GIT_CONFIG_GLOBAL", &global)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env_remove("GIT_CONFIG_COUNT")
                .output()
                .unwrap();
            assert!(out.status.success());
            parse(&out.stdout, dir)
        };
        std::fs::create_dir(d.join("sub")).unwrap();
        let v = run(&d.join("sub"));
        let names: Vec<_> = v
            .iter()
            .filter(|e| e.key == "user.name")
            .map(|e| (e.value.as_deref(), e.scope.as_str(), e.earlier))
            .collect();
        assert_eq!(
            names,
            [
                (Some("Local"), "local", false),
                (Some("Global"), "global", true)
            ]
        );
        let local = v.iter().find(|e| e.scope == "local").unwrap();
        assert_eq!(local.file.as_deref(), Some(d.join(".git/config").as_path()));
        let pass = v.iter().find(|e| e.key == "sendemail.smtppass").unwrap();
        assert_eq!(pass.hidden.as_deref(), Some(HIDDEN));
        assert!(!pass.line.contains("hush"));
        assert_eq!(
            repo_name(&d.join("sub")),
            d.file_name().map(|n| n.to_string_lossy().into_owned())
        );
        // Outside a repository: the global config only.
        let out =
            std::env::temp_dir().join(format!("coxswain-gitconfig-out-{}", std::process::id()));
        std::fs::create_dir_all(&out).unwrap();
        if crate::git::Status::read(&out).is_none() {
            assert!(run(&out).iter().all(|e| e.scope == "global"));
        }
        let _ = std::fs::remove_dir_all(&out);
        let _ = std::fs::remove_file(global);
        std::fs::remove_dir_all(d).unwrap();
    }
}
