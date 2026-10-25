//! OpenSCAD, in both apps: the picture the desktop app's preview and thumbnails show of a
//! `.scad` model, and *Export 3D model* (to STL or 3MF, next to the original).
//!
//! OpenSCAD runs installed first, else from its container image (`[preview.images] openscad`),
//! through `media`'s pipeline, which gives it the model's folder (a model includes others
//! beside it). The picture is not OpenSCAD's own (that needs a graphics card's OpenGL, which a
//! container has not): OpenSCAD makes the model as STL, and Coxswain draws that itself
//! (`picture`), lit from the upper left, seen from the front right and above, the way OpenSCAD
//! opens a model. A flat (2D) model is OpenSCAD's own SVG drawing of it.

use std::path::Path;

use crate::media::{How, Run};
use crate::t;

/// The name under `[preview.images]`.
pub const OPENSCAD: &str = "openscad";

/// A model by its name.
pub fn is_model(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("scad"))
}

/// The picture's size in pixels.
const W: u32 = 1024;
const H: u32 = 768;

/// Whether OpenSCAD can use Manifold (`--backend=manifold`), which makes a model in a second
/// where CGAL takes minutes: its image always, an installed one when its help names the option
/// (from the 2025 snapshots on). Asked once.
pub(crate) fn manifold(how: &How) -> bool {
    static LOCAL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    match how {
        How::Container { .. } => true,
        How::Local { path, .. } => *LOCAL.get_or_init(|| {
            crate::tools::capture(
                {
                    let mut c = crate::tools::command(path);
                    c.arg("--help");
                    c
                },
                std::time::Duration::from_secs(10),
                256 * 1024,
            )
            .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("--backend") || String::from_utf8_lossy(&o.stderr).contains("--backend"))
        }),
    }
}

/// Did OpenSCAD refuse an STL because the model is flat (2D)?
fn flat(said: &str) -> bool {
    said.contains("not a 3D object")
}

/// The model OpenSCAD makes of `run`'s file as a picture (SVG): drawn here from its STL, or
/// OpenSCAD's own SVG of a flat model.
pub(crate) fn picture_of(run: &mut Run, manifold: bool) -> Result<String, String> {
    let (input, stl) = (run.input.clone(), run.out("model.stl"));
    let mut a = vec!["-o", stl.as_str(), "--export-format", "binstl"];
    if manifold {
        a.push("--backend=manifold");
    }
    a.push(input.as_str());
    match run.go("openscad", &a) {
        Ok(_) => {}
        Err(e) if flat(&e) => {
            let svg = run.out("model.svg");
            run.go("openscad", &["-o", svg.as_str(), input.as_str()])?;
            return read(&run.scratch.join("model.svg")).map(|b| String::from_utf8_lossy(&b).into_owned());
        }
        Err(e) => return Err(e),
    }
    let tris = stl_file(&run.scratch.join("model.stl"))?;
    let mut png = vec![];
    picture(&tris, W, H, std::time::Instant::now() + DRAW_TIME)?.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(crate::media::png_in_svg(&png, W, H))
}

/// *Export 3D model*: `run`'s file as STL (binary) or 3MF, into `name` in the scratch folder.
pub(crate) fn export(run: &mut Run, stl: bool, name: &str, manifold: bool) -> Result<(), String> {
    let (input, out) = (run.input.clone(), run.out(name));
    let mut a = vec!["-o", out.as_str(), "--export-format", if stl { "binstl" } else { "3mf" }];
    if manifold {
        a.push("--backend=manifold");
    }
    a.push(input.as_str());
    run.go("openscad", &a).map(drop).map_err(|e| if flat(&e) { format!("{}\n{e}", t!("scad.flat")) } else { e })
}

/// What OpenSCAD wrote into the scratch folder, at most `MAX_STL` (`images::read_result`).
fn read(path: &Path) -> Result<Vec<u8>, String> {
    crate::images::read_result(path, MAX_STL, || t!("scad.too_large"))
}

fn stl_file(path: &Path) -> Result<Vec<[V; 3]>, String> {
    stl(&read(path)?).ok_or_else(|| t!("scad.empty"))
}

/// STL read at most (about four million triangles), and triangles drawn.
pub(crate) const MAX_STL: u64 = 200 << 20;
const MAX_TRIANGLES: usize = 4_000_000;

type V = [f32; 3];

/// The triangles of an STL, binary or text. None for what is neither, or holds none.
pub(crate) fn stl(b: &[u8]) -> Option<Vec<[V; 3]>> {
    let text = b.starts_with(b"solid") && b.iter().take(1024).any(|c| *c == b'\n') && std::str::from_utf8(&b[..b.len().min(4096)]).is_ok_and(|s| s.contains("facet") || s.contains("endsolid"));
    let tris = if text { stl_text(b) } else { stl_binary(b) }?;
    (!tris.is_empty()).then_some(tris)
}

fn stl_binary(b: &[u8]) -> Option<Vec<[V; 3]>> {
    let n = u32::from_le_bytes(b.get(80..84)?.try_into().ok()?) as usize;
    if b.len() < 84 + n.checked_mul(50)? || n > MAX_TRIANGLES {
        return None;
    }
    let f = |i: usize| f32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    Some(
        (0..n)
            .map(|t| {
                let at = 84 + t * 50 + 12;
                let v = |k: usize| [f(at + k * 12), f(at + k * 12 + 4), f(at + k * 12 + 8)];
                [v(0), v(1), v(2)]
            })
            .collect(),
    )
}

fn stl_text(b: &[u8]) -> Option<Vec<[V; 3]>> {
    let s = String::from_utf8_lossy(b);
    let mut out = vec![];
    let mut tri: Vec<V> = vec![];
    for line in s.lines() {
        let mut w = line.split_whitespace();
        if w.next() != Some("vertex") {
            continue;
        }
        let p: Vec<f32> = w.filter_map(|x| x.parse().ok()).collect();
        tri.push(<V>::try_from(p).ok()?);
        if tri.len() == 3 {
            out.push([tri[0], tri[1], tri[2]]);
            tri.clear();
            if out.len() > MAX_TRIANGLES {
                return None;
            }
        }
    }
    Some(out)
}

/// The model drawn as a picture `w` × `h`, as RGBA: seen from the front right and above (as
/// OpenSCAD opens it), each face shaded by how it faces the light, on a clear background.
/// Pixels the triangles' boxes may cover in all, and how long drawing may take: a model made to
/// be many screen-sized triangles is refused, not drawn for minutes.
const PIXEL_BUDGET: f64 = 300e6;
const DRAW_TIME: std::time::Duration = std::time::Duration::from_secs(20);

/// The box of a triangle on the picture, clamped to it: (x0, x1, y0, y1).
fn bbox(a: V, b: V, c: V, w: u32, h: u32) -> (u32, u32, u32, u32) {
    let x0 = a[0].min(b[0]).min(c[0]).floor().clamp(0.0, (w - 1) as f32) as u32;
    let x1 = a[0].max(b[0]).max(c[0]).ceil().clamp(0.0, (w - 1) as f32) as u32;
    let y0 = a[1].min(b[1]).min(c[1]).floor().clamp(0.0, (h - 1) as f32) as u32;
    let y1 = a[1].max(b[1]).max(c[1]).ceil().clamp(0.0, (h - 1) as f32) as u32;
    (x0, x1, y0, y1)
}

pub(crate) fn picture(tris: &[[V; 3]], w: u32, h: u32, deadline: std::time::Instant) -> Result<image::RgbaImage, String> {
    // Turned 25° about the vertical, tipped 35° toward the viewer: OpenSCAD's own first view.
    let (az, el) = (25f32.to_radians(), 55f32.to_radians());
    let view = |p: V| -> V {
        let (x, y) = (p[0] * az.cos() + p[1] * az.sin(), -p[0] * az.sin() + p[1] * az.cos());
        let (y2, z2) = (y * el.cos() - p[2] * el.sin(), y * el.sin() + p[2] * el.cos());
        // Screen x right, screen y up, depth toward the viewer.
        [x, z2, -y2]
    };
    let seen: Vec<[V; 3]> = tris.iter().map(|t| [view(t[0]), view(t[1]), view(t[2])]).collect();
    let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
    for v in seen.iter().flatten() {
        for k in 0..3 {
            lo[k] = lo[k].min(v[k]);
            hi[k] = hi[k].max(v[k]);
        }
    }
    let span = ((hi[0] - lo[0]) / w as f32).max((hi[1] - lo[1]) / h as f32).max(f32::EPSILON) * 1.08;
    let (cx, cy) = ((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0);
    let to = |v: V| -> V { [(v[0] - cx) / span + w as f32 / 2.0, h as f32 / 2.0 - (v[1] - cy) / span, v[2]] };
    let mut img = image::RgbaImage::new(w, h);
    let mut depth = vec![f32::MIN; (w * h) as usize];
    let light = {
        let l = [-0.4f32, 0.7, 0.6];
        let n = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt();
        [l[0] / n, l[1] / n, l[2] / n]
    };
    // OpenSCAD's yellow, on any pane.
    let base = [249.0f32, 215.0, 44.0];
    // The work first: the pixels every triangle's box covers, refused past the budget.
    let work: f64 = seen
        .iter()
        .map(|t| {
            let (x0, x1, y0, y1) = bbox(to(t[0]), to(t[1]), to(t[2]), w, h);
            (x1 - x0 + 1) as f64 * (y1 - y0 + 1) as f64
        })
        .sum();
    if work > PIXEL_BUDGET {
        return Err(t!("scad.too_large"));
    }
    for (i, t) in seen.iter().enumerate() {
        if i % 4096 == 0 && std::time::Instant::now() > deadline {
            return Err(t!("scad.too_large"));
        }
        let (a, b, c) = (to(t[0]), to(t[1]), to(t[2]));
        let (u, v) = ([t[1][0] - t[0][0], t[1][1] - t[0][1], t[1][2] - t[0][2]], [t[2][0] - t[0][0], t[2][1] - t[0][1], t[2][2] - t[0][2]]);
        let mut n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        if len == 0.0 {
            continue;
        }
        n = [n[0] / len, n[1] / len, n[2] / len];
        // A face seen from behind (an open model, or wound the other way) is lit as its front.
        if n[2] < 0.0 {
            n = [-n[0], -n[1], -n[2]];
        }
        // A key light from the upper left, and a softer one from the eye, so no side goes dark.
        let shade = (0.35 + 0.4 * (n[0] * light[0] + n[1] * light[1] + n[2] * light[2]).max(0.0) + 0.45 * n[2]).min(1.0);
        let px = image::Rgba([(base[0] * shade) as u8, (base[1] * shade) as u8, (base[2] * shade) as u8, 255]);
        let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        if area.abs() < 1e-12 {
            continue;
        }
        let (x0, x1, y0, y1) = bbox(a, b, c, w, h);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let w0 = ((b[0] - fx) * (c[1] - fy) - (b[1] - fy) * (c[0] - fx)) / area;
                let w1 = ((c[0] - fx) * (a[1] - fy) - (c[1] - fy) * (a[0] - fx)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let z = w0 * a[2] + w1 * b[2] + w2 * c[2];
                let i = (y * w + x) as usize;
                if z > depth[i] {
                    depth[i] = z;
                    img.put_pixel(x, y, px);
                }
            }
        }
    }
    Ok(img)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit cube as binary STL.
    fn cube() -> Vec<u8> {
        let v = |i: u8| [(i & 1) as f32, (i >> 1 & 1) as f32, (i >> 2 & 1) as f32];
        let faces = [[0, 1, 3, 2], [4, 6, 7, 5], [0, 4, 5, 1], [2, 3, 7, 6], [0, 2, 6, 4], [1, 5, 7, 3]];
        let mut b = vec![0u8; 80];
        b.extend(12u32.to_le_bytes());
        for f in faces {
            for t in [[f[0], f[1], f[2]], [f[0], f[2], f[3]]] {
                b.extend([0u8; 12]);
                for i in t {
                    for x in v(i) {
                        b.extend(x.to_le_bytes());
                    }
                }
                b.extend([0u8; 2]);
            }
        }
        b
    }

    /// Needs OpenSCAD installed, or with `COXSWAIN_SCAD_CONTAINER=1` podman with its image
    /// pulled: `cargo test -p coxswain-core -- --ignored real_openscad`. With `PNG` set, the
    /// picture is written there too.
    #[test]
    #[ignore]
    fn real_openscad() {
        use crate::media::{Op, Program};
        crate::i18n::set_language("en-GB");
        let d = std::env::temp_dir().join(format!("coxswain-real-scad-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let container = std::env::var("COXSWAIN_SCAD_CONTAINER").is_ok();
        let cfg = crate::config::PreviewConfig { prefer: if container { "container" } else { "local" }.into(), ..Default::default() };
        std::fs::write(d.join("lib.scad"), "r = 6;\n").unwrap();
        let part = d.join("-part.scad");
        std::fs::write(&part, "include <lib.scad>\ndifference() { cube([30,20,10], center=true); cylinder(h=12, r=r, center=true, $fn=64); }\n").unwrap();
        let flat = d.join("plate.scad");
        std::fs::write(&flat, "difference() { square(40); translate([20,20]) circle(8); }\n").unwrap();
        let stop = std::sync::atomic::AtomicBool::new(false);
        let run = |op: Op, f: &Path| crate::media::run_all(&cfg, &op, &[f.to_path_buf()], &stop, |_| {}, |_| {}).remove(0);
        let stl = run(Op::Stl, &part).unwrap();
        assert_eq!(stl, d.join("-part.stl"));
        assert!(stl_file(&stl).unwrap().len() > 100, "the hole is round");
        assert_eq!(run(Op::ThreeMf, &part).unwrap(), d.join("-part.3mf"));
        let e = run(Op::Stl, &flat).unwrap_err();
        assert!(e.starts_with(&t!("scad.flat")), "{e}");
        let how = crate::media::how(&cfg, Program::Openscad).unwrap();
        for (f, want) in [(&part, "data:image/png;base64,"), (&flat, "<path")] {
            let svg = d.join("model.svg");
            crate::media::preview(&how, &Op::Model, f, &svg, std::time::Duration::from_secs(120)).unwrap();
            assert!(std::fs::read_to_string(&svg).unwrap().contains(want), "{}", f.display());
        }
        if let Ok(png) = std::env::var("PNG") {
            picture(&stl_file(&stl).unwrap(), 800, 600, std::time::Instant::now() + DRAW_TIME).unwrap().save(png).unwrap();
        }
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn scad_refuses_a_model_of_many_screen_sized_triangles_at_once() {
        crate::i18n::set_language("en-GB");
        // Two million triangles, each nearly the whole picture: hours of drawing, refused by the
        // work counted first, not by the deadline: that is a day away, and the refusal comes
        // within a hang guard's time.
        let big = [[-1.0f32, -1.0, 0.0], [1.0, -1.0, 0.0], [0.0, 1.0, 0.5]];
        let tris = vec![big; 2_000_000];
        let start = std::time::Instant::now();
        let day = start + std::time::Duration::from_secs(24 * 3600);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || tx.send(picture(&tris, W, H, day).map(drop)));
        let refused = rx.recv_timeout(crate::test_limit(std::time::Duration::from_secs(30)));
        assert_eq!(refused.expect("drawn, not refused").unwrap_err(), t!("scad.too_large"));
        // A past deadline stops even work under the budget.
        assert!(picture(&[big; 100], W, H, start).is_err());
    }

    #[test]
    fn scad_draws_a_cube_with_three_shades() {
        let tris = stl(&cube()).unwrap();
        assert_eq!(tris.len(), 12);
        let img = picture(&tris, 200, 150, std::time::Instant::now() + DRAW_TIME).unwrap();
        let mut shades: Vec<[u8; 4]> = img.pixels().map(|p| p.0).filter(|p| p[3] == 255).collect();
        assert!(shades.len() > 200 * 150 / 6, "the cube fills the picture: {}", shades.len());
        shades.sort();
        shades.dedup();
        assert_eq!(shades.len(), 3, "three faces seen, each its own shade: {shades:?}");
        assert_eq!(img.get_pixel(0, 0).0[3], 0, "a clear background");
        let text = b"solid c\n facet normal 0 0 1\n  outer loop\n   vertex 0 0 0\n   vertex 1 0 0\n   vertex 0 1 0\n  endloop\n endfacet\nendsolid c\n";
        assert_eq!(stl(text).unwrap().len(), 1);
        assert!(stl(b"solid nothing\nendsolid\n").is_none());
        let mut short = cube();
        short.truncate(200);
        assert!(stl(&short).is_none(), "a count past the end");
    }
}
