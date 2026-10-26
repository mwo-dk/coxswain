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

/// The origin the answer lets read it: the page's own, or under `npx tauri dev` (a debug build)
/// the dev server's, `http://localhost:<port>`. Never a preview frame's (`null`) or `*`.
fn allowed_origin(req: &Request<Vec<u8>>) -> String {
    let asked = req.headers().get(header::ORIGIN).and_then(|o| o.to_str().ok()).unwrap_or_default();
    let dev = || tauri::Url::parse(asked).is_ok_and(|u| u.scheme() == "http" && u.host_str() == Some("localhost") && u.path() == "/" && asked == u.origin().ascii_serialization());
    if asked == "tauri://localhost" || asked == "http://tauri.localhost" || (cfg!(debug_assertions) && dev()) { asked.to_string() } else { ORIGIN.to_string() }
}
/// The most one range answer holds, as Tauri's own protocol: a video is read piece by piece.
const MAX_RANGE: u64 = 1000 * 1024;
/// The most a whole-file answer holds (the largest preview reads 500 MiB); a bigger file is
/// read in ranges or not at all, so no request fills the memory.
const MAX_WHOLE: u64 = 512 * 1024 * 1024;

/// The type a file is served as, from its first bytes and its name. A document type that could
/// run script (HTML, XHTML, XML) is served as plain text, so a `.pdf` that holds a web page shows
/// as text, never as a page; an unknown type is plain bytes, not HTML as Tauri's default.
/// `application/pdf` only for bytes that start as a PDF does; any other `.pdf` is plain text, so
/// its frame shows it rather than download it.
fn content_type(magic: &[u8], path: &Path) -> String {
    use tauri::utils::mime_type::MimeType;
    let mime = MimeType::parse_with_fallback(magic, &path.to_string_lossy(), MimeType::OctetStream);
    let pdf = mime == "application/pdf" || path.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf"));
    match mime.as_str() {
        _ if pdf && !magic.starts_with(b"%PDF-") => "text/plain".into(),
        "text/html" | "application/xhtml+xml" | "text/xml" | "application/xml" => "text/plain".into(),
        _ => mime,
    }
}

/// A component as JavaScript's `encodeURIComponent` writes it: the page (`preview.svelte`'s
/// `pageUrl`) asks for its files so, and its policy has to name them alike.
fn encode_component(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// A request for an HTML file as the page it is (`?page`, the preview of an `.html`, `.htm`
/// or `.xhtml`): served as HTML, where every other request for one is plain text.
fn is_page(req: &Request<Vec<u8>>, path: &Path) -> bool {
    req.uri().query().is_some_and(|q| q.split('&').any(|p| p == "page"))
        && path.extension().and_then(|e| e.to_str()).is_some_and(|e| ["html", "htm", "xhtml"].iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// The policy a page is shown under: no script (a sandbox without `allow-scripts`; it keeps its
/// origin, the file protocol's, which alone may load from it), nothing from the web, and
/// pictures, stylesheets, fonts and media only from the page's own folder and below: named by
/// the file's real folder, encoded one segment at a time as the page asks for them, so neither
/// `../private/photo.jpg` nor `/home/me/keys.png` nor a page URL written another way reaches
/// further.
fn page_policy(req: &Request<Vec<u8>>, path: &Path) -> String {
    let uri = req.uri();
    let origin = format!("{}://{}", uri.scheme_str().unwrap_or("asset"), uri.authority().map_or("localhost", |a| a.as_str()));
    let full = path.to_string_lossy();
    let lead = if full.starts_with('/') { "%2F" } else { "" };
    let parent = path.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let segments: Vec<String> = parent.split(['/', '\\']).filter(|s| !s.is_empty()).map(encode_component).collect();
    let dir = format!("{origin}/{lead}{}/", segments.join("/"));
    format!("sandbox allow-same-origin; default-src 'none'; img-src {dir} data:; style-src {dir} 'unsafe-inline'; font-src {dir} data:; media-src {dir}; form-action 'none'")
}

/// The file a request names: `asset://localhost/%2Fhome%2Fme%2Fa.pdf` (Windows:
/// `http://asset.localhost/C%3A%2Fa.pdf`). Only an absolute path without `..`.
fn path_of(req: &Request<Vec<u8>>) -> Option<PathBuf> {
    let raw = req.uri().path().strip_prefix('/')?.as_bytes();
    let path = PathBuf::from(String::from_utf8(coxswain_core::fs::percent_decode(raw)).ok()?);
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
    let origin = allowed_origin(req);
    let answer = || Response::builder().header(header::ACCESS_CONTROL_ALLOW_ORIGIN, &origin).header(header::VARY, "Origin");
    let status = |s: StatusCode| answer().status(s).body(Vec::new()).expect("a plain answer");
    let Some(path) = path_of(req) else { return status(StatusCode::FORBIDDEN) };
    if unasked(&path) {
        return status(StatusCode::FORBIDDEN);
    }
    let read = || -> std::io::Result<Response<Vec<u8>>> {
        let mut file = File::open(&path)?;
        let len = file.metadata()?.len();
        let mut magic = Vec::with_capacity(len.min(8192) as usize);
        (&mut file).take(8192).read_to_end(&mut magic)?;
        let page = is_page(req, &path);
        let xhtml = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("xhtml"));
        let mime = match page {
            true if xhtml => "application/xhtml+xml".to_string(),
            true => "text/html".to_string(),
            false => content_type(&magic, &path),
        };
        let mut resp = answer().header(header::CONTENT_TYPE, &mime).header(header::X_CONTENT_TYPE_OPTIONS, "nosniff").header(header::ACCEPT_RANGES, "bytes");
        // Whatever is shown in a frame of its own runs no script and reaches no origin. Not on a
        // PDF: the browsers' PDF viewers do not open under a sandbox (WebView2 blocks them).
        if page {
            resp = resp.header(header::CONTENT_SECURITY_POLICY, page_policy(req, &path));
        } else if mime != "application/pdf" {
            resp = resp.header(header::CONTENT_SECURITY_POLICY, "sandbox");
        }
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
            None if len > MAX_WHOLE => (resp.status(StatusCode::PAYLOAD_TOO_LARGE), Vec::new()),
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
    fn only_the_apps_own_origins_are_let_read() {
        let from = |o: &str| allowed_origin(&Request::builder().uri("asset://localhost/x").header("origin", o).body(Vec::new()).unwrap());
        assert_eq!(from("tauri://localhost"), "tauri://localhost");
        assert_eq!(from("http://tauri.localhost"), "http://tauri.localhost");
        assert_eq!(from("http://localhost:1420"), if cfg!(debug_assertions) { "http://localhost:1420" } else { ORIGIN });
        for other in ["null", "*", "asset://localhost", "http://asset.localhost", "http://localhost:1420/x", "http://evil.example", ""] {
            assert_eq!(from(other), ORIGIN, "{other}");
        }
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
        assert_eq!(respond(&get(&f, None), none).headers()[header::CONTENT_SECURITY_POLICY], "sandbox");
        assert_eq!(respond(&get(&dir.join("gone"), None), none).status(), StatusCode::NOT_FOUND);
        // `..` and relative paths never reach the disk.
        let up = Request::builder().uri(format!("asset://localhost/{}%2F..%2Fclip.bin", dir.display().to_string().replace('/', "%2F"))).body(Vec::new()).unwrap();
        assert_eq!(respond(&up, none).status(), StatusCode::FORBIDDEN);
        let rel = Request::builder().uri("asset://localhost/clip.bin").body(Vec::new()).unwrap();
        assert_eq!(respond(&rel, none).status(), StatusCode::FORBIDDEN);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_web_page_is_never_served_as_one() {
        let ty = |bytes: &[u8], name: &str| content_type(bytes, Path::new(name));
        let page = b"<!DOCTYPE html><html><script>fetch('asset://localhost/%2Fetc%2Fpasswd')</script>";
        assert_eq!(ty(page, "/home/me/invoice.pdf"), "text/plain");
        assert_eq!(ty(page, "/home/me/page.html"), "text/plain");
        assert_eq!(ty(b"<?xml version=\"1.0\"?><html xmlns=\"http://www.w3.org/1999/xhtml\"/>", "/home/me/a.xhtml"), "text/plain");
        assert_eq!(ty(b"not a pdf at all", "/home/me/a.PDF"), "text/plain");
        assert_eq!(ty(b"%PDFnot", "/home/me/a.bin"), "text/plain");
        assert_eq!(ty(b"\0\x01\x02 unknown", "/home/me/a.weird"), "application/octet-stream");
        assert_eq!(ty(b"%PDF-1.7\n%\xe2\xe3", "/home/me/a.pdf"), "application/pdf");
        assert_eq!(ty(b"\x89PNG\r\n\x1a\nrest", "/home/me/a.png"), "image/png");
        assert_eq!(ty(b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>", "/home/me/a.svg"), "image/svg+xml");
        assert_eq!(ty(b"body { color: red }", "/home/me/a.css"), "text/css");
        // A real PDF opens in the viewer; anything else may not run script in its frame.
        let dir = std::env::temp_dir().join(format!("coxswain-test-type-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (fake, real) = (dir.join("fake.pdf"), dir.join("real.pdf"));
        std::fs::write(&fake, page).unwrap();
        std::fs::write(&real, b"%PDF-1.4 body").unwrap();
        let r = respond(&get(&fake, None), |_: &Path| false);
        assert_eq!((r.headers()[header::CONTENT_TYPE].to_str().unwrap(), r.headers()[header::CONTENT_SECURITY_POLICY].to_str().unwrap()), ("text/plain", "sandbox"));
        assert_eq!(r.headers()[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
        let r = respond(&get(&real, None), |_: &Path| false);
        assert_eq!(r.headers()[header::CONTENT_TYPE], "application/pdf");
        assert!(r.headers().get(header::CONTENT_SECURITY_POLICY).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The preview of an HTML file asks for it as a page (`?page`): HTML, under a policy of its
    /// own folder and no script; asked for any other way, or not an HTML file, it stays text.
    #[test]
    fn a_page_is_shown_under_its_own_folder_only() {
        let dir = std::env::temp_dir().join(format!("coxswain-test-page-{} v2", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let html = dir.join("index.html");
        std::fs::write(&html, "<!doctype html><link rel=stylesheet href=style.css><script>alert(1)</script>").unwrap();
        std::fs::write(dir.join("notes.txt"), "plain").unwrap();
        let ask = |p: &Path, query: &str, encode: &dyn Fn(&str) -> String| {
            let req = Request::builder().uri(format!("asset://localhost/{}{query}", encode(&p.to_string_lossy()))).body(Vec::new()).unwrap();
            respond(&req, |_: &Path| false)
        };
        let whole = |s: &str| encode_component(s);
        let r = ask(&html, "?page", &whole);
        assert_eq!(r.headers()[header::CONTENT_TYPE], "text/html");
        let csp = r.headers()[header::CONTENT_SECURITY_POLICY].to_str().unwrap().to_string();
        assert!(csp.starts_with("sandbox allow-same-origin;") && !csp.contains("allow-scripts"), "{csp}");
        assert!(csp.contains("default-src 'none'") && csp.contains("form-action 'none'"), "{csp}");
        // The folder by its real path, one segment at a time, however the page itself was asked for.
        let parent = dir.to_string_lossy().into_owned();
        let segs: Vec<String> = parent.split(['/', '\\']).filter(|s| !s.is_empty()).map(encode_component).collect();
        let want = format!("style-src asset://localhost/{}{}/ 'unsafe-inline'", if parent.starts_with('/') { "%2F" } else { "" }, segs.join("/"));
        assert!(csp.contains(&want), "{csp}\n{want}");
        assert!(!csp.contains("asset://localhost/ ") && !csp.contains("asset://localhost/;"), "never the whole protocol");
        // Without ?page, or a file that is no page: text under a plain sandbox, as before.
        let r = ask(&html, "", &whole);
        assert_eq!((r.headers()[header::CONTENT_TYPE].to_str().unwrap(), r.headers()[header::CONTENT_SECURITY_POLICY].to_str().unwrap()), ("text/plain", "sandbox"));
        let r = ask(&dir.join("notes.txt"), "?page", &whole);
        assert_eq!(r.headers()[header::CONTENT_SECURITY_POLICY], "sandbox");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
