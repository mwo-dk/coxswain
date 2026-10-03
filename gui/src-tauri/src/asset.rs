//! The `asset:` protocol the page loads files from (pictures, PDFs, fonts, media, Office files,
//! a page's own CSS), in place of Tauri's own: the same, except that it refuses a file only in
//! the cloud that the user has not asked for, so no preview downloads one by itself, whatever
//! the page asks.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use tauri::http::{header, Request, Response, StatusCode};

/// The page's origin, which may read what this protocol serves.
const ORIGIN: &str = if cfg!(windows) { "http://tauri.localhost" } else { "tauri://localhost" };
/// The most one range answer holds, as Tauri's own protocol: a video is read piece by piece.
const MAX_RANGE: u64 = 1000 * 1024;

/// The file a request names: `asset://localhost/%2Fhome%2Fme%2Fa.pdf` (Windows:
/// `http://asset.localhost/C%3A%2Fa.pdf`). Only an absolute path without `..`.
fn path_of(req: &Request<Vec<u8>>) -> Option<PathBuf> {
    let raw = req.uri().path().strip_prefix('/')?.as_bytes();
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        match (raw[i], raw.get(i + 1).copied().and_then(hex), raw.get(i + 2).copied().and_then(hex)) {
            (b'%', Some(h), Some(l)) => {
                out.push((h * 16 + l) as u8);
                i += 3;
            }
            (b, ..) => {
                out.push(b);
                i += 1;
            }
        }
    }
    let path = PathBuf::from(String::from_utf8(out).ok()?);
    (path.is_absolute() && !path.components().any(|c| c == Component::ParentDir)).then_some(path)
}

/// `bytes=a-b`, `bytes=a-` or `bytes=-n` against a file of `len` bytes: the first range only,
/// at most `MAX_RANGE` long. `None` when it cannot be met.
fn range(header: &str, len: u64) -> Option<(u64, u64)> {
    let first = header.trim().strip_prefix("bytes=")?.split(',').next()?.trim();
    let (a, b) = first.split_once('-')?;
    let (start, end) = match (a.trim(), b.trim()) {
        ("", n) => (len.checked_sub(n.parse::<u64>().ok()?.min(len))?, len.checked_sub(1)?),
        (a, "") => (a.parse().ok()?, len.checked_sub(1)?),
        (a, b) => (a.parse().ok()?, b.parse::<u64>().ok()?.min(len.checked_sub(1)?)),
    };
    (start <= end && start < len).then(|| (start, end.min(start + MAX_RANGE - 1)))
}

/// The answer to one request. `unasked` tells a file only in the cloud that the user has not
/// asked for (`coxswain_core::cloud::unasked`; a test passes its own).
pub fn respond(req: &Request<Vec<u8>>, unasked: impl Fn(&Path) -> bool) -> Response<Vec<u8>> {
    let status = |s: StatusCode| Response::builder().status(s).header(header::ACCESS_CONTROL_ALLOW_ORIGIN, ORIGIN).body(Vec::new()).expect("a plain answer");
    let Some(path) = path_of(req) else { return status(StatusCode::FORBIDDEN) };
    if unasked(&path) {
        return status(StatusCode::FORBIDDEN);
    }
    let read = || -> std::io::Result<Response<Vec<u8>>> {
        let mut file = File::open(&path)?;
        let len = file.metadata()?.len();
        let mut magic = Vec::with_capacity(len.min(8192) as usize);
        (&mut file).take(8192).read_to_end(&mut magic)?;
        let mime = tauri::utils::mime_type::MimeType::parse(&magic, &path.to_string_lossy());
        let resp = Response::builder().header(header::ACCESS_CONTROL_ALLOW_ORIGIN, ORIGIN).header(header::CONTENT_TYPE, mime).header(header::ACCEPT_RANGES, "bytes");
        let ranged = req.headers().get(header::RANGE).and_then(|r| r.to_str().ok());
        let (resp, body) = match ranged.map(|r| range(r, len)) {
            Some(None) => (resp.status(StatusCode::RANGE_NOT_SATISFIABLE).header(header::CONTENT_RANGE, format!("bytes */{len}")), Vec::new()),
            Some(Some((start, end))) => {
                let mut buf = Vec::with_capacity((end + 1 - start) as usize);
                file.seek(SeekFrom::Start(start))?;
                file.take(end + 1 - start).read_to_end(&mut buf)?;
                let resp = resp.status(StatusCode::PARTIAL_CONTENT).header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{len}"));
                (resp.header(header::ACCESS_CONTROL_EXPOSE_HEADERS, "content-range"), buf)
            }
            None if req.method() == tauri::http::Method::HEAD => (resp.header(header::CONTENT_LENGTH, len), Vec::new()),
            None => {
                if len > magic.len() as u64 {
                    magic.reserve((len - magic.len() as u64) as usize);
                    file.read_to_end(&mut magic)?;
                }
                (resp, magic)
            }
        };
        Ok(resp.body(body).expect("a file's answer"))
    };
    match read() {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => status(StatusCode::NOT_FOUND),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => status(StatusCode::FORBIDDEN),
        Err(_) => status(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(path: &Path, range: Option<&str>) -> Request<Vec<u8>> {
        let enc: String = path.to_string_lossy().bytes().map(|b| if b.is_ascii_alphanumeric() { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
        let mut r = Request::builder().uri(format!("asset://localhost/{enc}"));
        if let Some(v) = range {
            r = r.header("range", v);
        }
        r.body(Vec::new()).unwrap()
    }

    #[test]
    fn a_file_only_in_the_cloud_is_refused_until_asked_for() {
        let dir = std::env::temp_dir().join(format!("coxswain-test-asset-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (photo, online) = (dir.join("photo.png"), dir.join("cloud.pdf"));
        std::fs::write(&photo, b"\x89PNG\r\n\x1a\nrest").unwrap();
        std::fs::write(&online, b"%PDF-1.4 secret").unwrap();
        let mut asked = false;
        let cloud = |asked: bool| move |p: &Path| p.ends_with("cloud.pdf") && !asked;
        let r = respond(&get(&photo, None), cloud(asked));
        assert_eq!((r.status(), r.body().as_slice()), (StatusCode::OK, &b"\x89PNG\r\n\x1a\nrest"[..]));
        assert_eq!(r.headers()[header::CONTENT_TYPE], "image/png");
        let r = respond(&get(&online, None), cloud(asked));
        assert_eq!(r.status(), StatusCode::FORBIDDEN);
        assert!(r.body().is_empty());
        assert_eq!(respond(&get(&online, Some("bytes=0-3")), cloud(asked)).status(), StatusCode::FORBIDDEN);
        // The user pressed "Download and preview".
        asked = true;
        let r = respond(&get(&online, None), cloud(asked));
        assert_eq!((r.status(), r.body().as_slice()), (StatusCode::OK, &b"%PDF-1.4 secret"[..]));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn ranges_and_bad_paths() {
        let dir = std::env::temp_dir().join(format!("coxswain-test-range-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("clip.bin");
        std::fs::write(&f, b"0123456789").unwrap();
        let none = |_: &Path| false;
        let r = respond(&get(&f, Some("bytes=2-4")), none);
        assert_eq!((r.status(), r.body().as_slice()), (StatusCode::PARTIAL_CONTENT, &b"234"[..]));
        assert_eq!(r.headers()[header::CONTENT_RANGE], "bytes 2-4/10");
        assert_eq!(respond(&get(&f, Some("bytes=7-")), none).body().as_slice(), b"789");
        assert_eq!(respond(&get(&f, Some("bytes=-3")), none).body().as_slice(), b"789");
        assert_eq!(respond(&get(&f, Some("bytes=8-99")), none).body().as_slice(), b"89");
        assert_eq!(respond(&get(&f, Some("bytes=10-")), none).status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(respond(&get(&dir.join("gone"), None), none).status(), StatusCode::NOT_FOUND);
        // `..` and relative paths never reach the disk.
        let up = Request::builder().uri(format!("asset://localhost/{}%2F..%2Fclip.bin", dir.display().to_string().replace('/', "%2F"))).body(Vec::new()).unwrap();
        assert_eq!(respond(&up, none).status(), StatusCode::FORBIDDEN);
        let rel = Request::builder().uri("asset://localhost/clip.bin").body(Vec::new()).unwrap();
        assert_eq!(respond(&rel, none).status(), StatusCode::FORBIDDEN);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
