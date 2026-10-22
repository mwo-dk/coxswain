//! ffmpeg and ImageMagick, in both apps: the facts of a video or a song (length, size in
//! pixels, codecs, tracks), the pictures the desktop app's preview and thumbnails show (a
//! frame of a video, a song's waveform, HEIC, GIMP and RAW photos), and the two actions,
//! *Convert video or audio* (to MP4, MP3 or FLAC, its sound, a clip) and *Convert or edit
//! pictures* (to JPEG, PNG or WebP, smaller, without its location, turned).
//!
//! ffmpeg runs as installed, else from its container image (`[preview.images] ffmpeg`);
//! ImageMagick from its container first (`[preview.images] imagemagick`), else as installed,
//! held by a policy to the picture formats it is asked for (`policy`). A container gets no
//! network and sees one file, read-only; each run writes into a folder of its own in the
//! cache, and a result is put next to the original only under a name no file has, with
//! `create_new`: nothing is ever written over. A file's name is never an argument: the
//! container sees `/src/in.<ending>`, an installed ffmpeg `file:<path>`, an installed
//! ImageMagick a link or copy named `in.<ending>`, so no name is read as an option or a coder.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crate::config::PreviewConfig;
use crate::t;

/// Their names under `[preview.images]`.
pub const FFMPEG: &str = "ffmpeg";
pub const MAGICK: &str = "imagemagick";

/// Videos the desktop app's player plays; ffmpeg gives them a thumbnail and their facts.
pub const VIDEO: &[&str] = &["mp4", "webm", "mkv", "mov", "m4v", "ogv"];
/// Videos no web view plays: the preview shows a frame of one, and offers MP4.
pub const VIDEO_OTHER: &[&str] = &[
    "avi", "wmv", "flv", "mpg", "mpeg", "m2v", "ts", "m2ts", "mts", "3gp", "3g2", "vob", "asf", "rm", "rmvb", "divx",
    "ogm", "f4v", "mxf", "dv", "nut",
];
/// Sound the player plays; ffmpeg draws its waveform.
pub const AUDIO: &[&str] = &["mp3", "flac", "wav", "ogg", "m4a", "opus", "aac"];
/// Sound no web view plays: its waveform and facts.
pub const AUDIO_OTHER: &[&str] = &[
    "wma", "ape", "ac3", "dts", "amr", "aiff", "aif", "au", "mka", "wv", "tta", "caf", "w64", "mp2", "oga", "spx", "ra",
];
/// Apple's photos: shown by an installed ImageMagick or ffmpeg, else ImageMagick's image.
pub const HEIF: &[&str] = &["heic", "heif"];
/// Pictures only ImageMagick reads (TGA, PCX, PSD and the other old ones Coxswain draws itself).
pub const MAGICK_ONLY: &[&str] = &["xcf", "dpx", "cin", "fits", "fts", "sgi", "sun", "ras", "xwd", "pict", "pct", "miff", "wbmp"];
/// Cameras' RAW photos: ImageMagick with LibRaw reads them, installed or in its image.
pub const RAW: &[&str] = &[
    "dng", "cr2", "cr3", "crw", "nef", "nrw", "arw", "srf", "sr2", "orf", "rw2", "raf", "pef", "srw", "x3f", "erf", "kdc",
    "dcr", "mrw", "3fr", "iiq", "rwl",
];
/// Pictures *Convert or edit pictures* takes, besides those above.
const PICTURES: &[&str] = &["jpg", "jpeg", "png", "webp", "gif", "bmp", "tif", "tiff", "avif", "jxl", "psd", "tga"];
/// Formats a result keeps as they are; any other comes out as JPEG (a photo) or PNG.
const WRITES: &[&str] = &["jpg", "jpeg", "png", "webp", "gif", "bmp", "tif", "tiff"];

/// Demuxers ffmpeg may use: those of real media files, none that reads a list of other files
/// or addresses (HLS, concat, image sequences).
const DEMUXERS: &str = "mov,mp4,m4a,3gp,3g2,mj2,matroska,webm,avi,asf,flv,mpeg,mpegts,mpegps,mpegvideo,m4v,h264,hevc,ogg,wav,mp3,flac,aac,ac3,eac3,dts,amr,aiff,au,ape,wv,rm,mxf,nut,caf,w64,tta,dv,mp2,spdif,wsaud";

fn ending(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase()
}

fn among(path: &Path, lists: &[&[&str]]) -> bool {
    let e = ending(path);
    lists.iter().any(|l| l.contains(&e.as_str()))
}

/// A video or a song, by its name: *Convert video or audio* takes it.
pub fn is_media(path: &Path) -> bool {
    among(path, &[VIDEO, VIDEO_OTHER, AUDIO, AUDIO_OTHER])
}

/// A video by its name.
pub fn is_video(path: &Path) -> bool {
    among(path, &[VIDEO, VIDEO_OTHER])
}

/// A picture *Convert or edit pictures* takes, by its name.
pub fn is_picture(path: &Path) -> bool {
    among(path, &[PICTURES, HEIF, MAGICK_ONLY, RAW])
}

/// The endings by kind, for the desktop app's page: what its preview shows how, and what each
/// action takes (`pictures`).
pub fn endings() -> std::collections::BTreeMap<&'static str, Vec<&'static str>> {
    [
        ("video", VIDEO.to_vec()),
        ("video_other", VIDEO_OTHER.to_vec()),
        ("audio", AUDIO.to_vec()),
        ("audio_other", AUDIO_OTHER.to_vec()),
        ("heif", HEIF.to_vec()),
        ("magick", MAGICK_ONLY.to_vec()),
        ("raw", RAW.to_vec()),
        ("pictures", [PICTURES, HEIF, MAGICK_ONLY, RAW].concat()),
    ]
    .into_iter()
    .collect()
}

/// Which program does the work.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Program {
    Ffmpeg,
    Magick,
    /// GDAL, for geodata (`geo`).
    Gdal,
    /// OpenSCAD, for models (`scad`).
    Openscad,
    /// KiCad's `kicad-cli`, for schematics and boards (`kicad`).
    Kicad,
}

impl Program {
    /// The programs whose work the actions offer, in the desktop app's order of `media_ops`.
    pub const ALL: [Program; 5] = [Program::Ffmpeg, Program::Magick, Program::Gdal, Program::Openscad, Program::Kicad];

    /// The action that runs it on files, and its feature in F1.
    pub fn action(self) -> crate::config::Action {
        use crate::config::Action;
        match self {
            Program::Ffmpeg => Action::ConvertMedia,
            Program::Magick => Action::EditPictures,
            Program::Gdal => Action::ConvertGeodata,
            Program::Openscad => Action::ExportModel,
            Program::Kicad => Action::ExportKicad,
        }
    }
    pub fn feature(self) -> &'static str {
        match self {
            Program::Ffmpeg => "convert_media",
            Program::Magick => "edit_pictures",
            Program::Gdal => "geodata",
            Program::Openscad => "openscad",
            Program::Kicad => "kicad",
        }
    }
    /// The status line when none of the files is one it takes.
    pub fn none(self) -> String {
        t!(match self {
            Program::Ffmpeg => "media.none_media",
            Program::Magick => "media.none_pictures",
            Program::Gdal => "geo.none",
            Program::Openscad => "scad.none",
            Program::Kicad => "kicad.none",
        })
    }

    /// Its image's name under `[preview.images]`.
    pub fn tool(self) -> &'static str {
        match self {
            Program::Ffmpeg => FFMPEG,
            Program::Magick => MAGICK,
            Program::Gdal => crate::geo::GDAL,
            Program::Openscad => crate::scad::OPENSCAD,
            Program::Kicad => crate::kicad::KICAD,
        }
    }
    /// What is looked for on the PATH: GDAL's programs are installed together.
    fn local(self) -> &'static str {
        match self {
            Program::Ffmpeg => "ffmpeg",
            Program::Magick => "magick",
            Program::Gdal => "gdalinfo",
            Program::Openscad => "openscad",
            Program::Kicad => "kicad-cli",
        }
    }
    fn name(self) -> &'static str {
        match self {
            Program::Ffmpeg => "ffmpeg",
            Program::Magick => "ImageMagick",
            Program::Gdal => "GDAL",
            Program::Openscad => "OpenSCAD",
            Program::Kicad => "KiCad",
        }
    }
    /// The texts of `missing`: what to get it from, and what needs it.
    fn needs(self) -> (&'static str, &'static str) {
        match self {
            Program::Ffmpeg => ("media.install_ffmpeg", "media.needs_ffmpeg"),
            Program::Magick => ("media.install_magick", "media.needs_magick"),
            Program::Gdal => ("geo.install_gdal", "geo.needs_gdal"),
            Program::Openscad => ("scad.install", "scad.needs"),
            Program::Kicad => ("kicad.install", "kicad.needs"),
        }
    }
}

/// How a program runs here.
#[derive(Clone, Debug, PartialEq)]
pub enum How {
    Local { program: Program, path: PathBuf },
    Container { program: Program, rt: &'static str, path: PathBuf, image: String },
}

impl How {
    fn program(&self) -> Program {
        match self {
            How::Local { program, .. } | How::Container { program, .. } => *program,
        }
    }
}

/// How `program` runs for an action, or why it cannot: what to install, or which setting to
/// change. ffmpeg: as installed first; ImageMagick: its container first, as `[preview]
/// prefer` and `[preview.tools]` allow.
pub fn how(cfg: &PreviewConfig, program: Program) -> Result<How, String> {
    let tool = program.tool();
    let pref = crate::preview_tools::asked(cfg, tool);
    let pref = pref.as_str();
    let local = crate::tools::which(program.local()).map(|path| How::Local { program, path });
    let image = cfg.images.get(tool).filter(|i| !i.is_empty()).cloned();
    let container = match (crate::tools::container_runtime(cfg), &image) {
        (Some((rt, path)), Some(image)) => Some(How::Container { program, rt, path, image: image.clone() }),
        _ => None,
    };
    let found = match (pref, program) {
        ("container", _) | ("auto", Program::Magick) => container.or(local),
        ("local", _) => local,
        _ => local.or(container),
    };
    found.ok_or_else(|| missing(cfg, pref, program, image.is_some()))
}

/// Why a program cannot run: the line that installs it, and what keeps the container from it.
fn missing(cfg: &PreviewConfig, pref: &str, program: Program, image: bool) -> String {
    let install = match crate::tools::install(program.local()) {
        Some(line) => t!("searchable.install_line", "line" => line),
        None => t!(program.needs().0),
    };
    let container = if pref == "local" {
        t!("searchable.prefer_local")
    } else if cfg.container == "off" {
        t!("searchable.containers_off")
    } else if !image {
        t!("convert.no_image", "tool" => program.tool())
    } else {
        crate::tools::no_runtime()
    };
    format!("{} {install} {container}", t!(program.needs().1))
}

/// What a run does, for the question before it: which program, and the first pull with its
/// size when the image is not there yet. The bool: a pull it makes.
pub fn about(how: &How) -> (String, bool) {
    let name = how.program().name();
    match how {
        How::Local { .. } => (t!("media.runs_local", "program" => name), false),
        How::Container { rt, path, image, .. } => {
            // GDAL sees the file's folder (a shapefile is several files), the others the file alone.
            let key = if matches!(how.program(), Program::Gdal | Program::Openscad | Program::Kicad) { "geo.runs_container" } else { "media.runs_container" };
            let runs = t!(key, "program" => name, "runtime" => *rt, "image" => crate::images::short(image));
            if crate::tools::image_size(path, image).is_some() {
                return (runs, false);
            }
            let pull = match crate::images::download(image) {
                Some(size) => t!("convert.first_run_pulls_large", "image" => crate::images::short(image), "size" => size),
                None => t!("convert.first_run_pulls", "image" => crate::images::short(image)),
            };
            (format!("{runs} {pull}."), true)
        }
    }
}

// ---------------------------------------------------------------- what can be done

/// One thing ffmpeg or ImageMagick makes of a file.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// Previews: a frame of a video, a song's waveform, a picture no wider or taller than 2048.
    Frame,
    Waveform,
    Picture,
    /// *Convert video or audio*.
    Mp4,
    Mp3,
    Flac,
    /// The sound as it is, without the picture.
    Audio,
    /// `length` seconds from `start`, copied as they are.
    Clip { start: u64, length: u64 },
    /// *Convert or edit pictures*.
    Jpeg,
    Png,
    Webp,
    /// No side longer than this many pixels.
    Resize(u32),
    /// Without the location and the camera's other notes.
    NoLocation,
    /// Turned clockwise by 90, 180 or 270 degrees.
    Rotate(u16),
    /// GDAL: the preview's map, and *Convert geodata*.
    Map,
    Geo(crate::geo::To),
    /// OpenSCAD: the preview's picture, and *Export 3D model* as STL or 3MF.
    Model,
    Stl,
    ThreeMf,
    /// KiCad: the preview (a PDF of the sheets or layers), the thumbnail, and *Export from KiCad*.
    Board,
    BoardThumb,
    Kicad(crate::kicad::To),
}

impl Op {
    /// The program that does it.
    pub fn program(&self) -> Program {
        match self {
            Op::Frame | Op::Waveform | Op::Mp4 | Op::Mp3 | Op::Flac | Op::Audio | Op::Clip { .. } => Program::Ffmpeg,
            Op::Map | Op::Geo(_) => Program::Gdal,
            Op::Model | Op::Stl | Op::ThreeMf => Program::Openscad,
            Op::Board | Op::BoardThumb | Op::Kicad(_) => Program::Kicad,
            _ => Program::Magick,
        }
    }

    /// The ones the actions offer, in their menus' order; Clip and Resize ask for a value.
    pub fn menu(program: Program) -> Vec<Op> {
        match program {
            Program::Ffmpeg => vec![Op::Mp4, Op::Mp3, Op::Flac, Op::Audio, Op::Clip { start: 0, length: 30 }],
            Program::Magick => vec![Op::Jpeg, Op::Png, Op::Webp, Op::Resize(1600), Op::NoLocation, Op::Rotate(90), Op::Rotate(270), Op::Rotate(180)],
            Program::Gdal => crate::geo::To::ALL.map(Op::Geo).to_vec(),
            Program::Openscad => vec![Op::Stl, Op::ThreeMf],
            Program::Kicad => crate::kicad::To::ALL.map(Op::Kicad).to_vec(),
        }
    }

    /// What it is called in the menus.
    pub fn label(&self) -> String {
        match self {
            Op::Rotate(d) => t!("media.op.rotate", "degrees" => d),
            Op::Geo(to) => crate::i18n::tr(&format!("geo.to.{}", to.id()), &[]),
            Op::Stl => t!("scad.op.stl"),
            Op::ThreeMf => t!("scad.op.3mf"),
            Op::Kicad(to) => crate::i18n::tr(&format!("kicad.to.{}", to.id()), &[]),
            _ => crate::i18n::tr(&format!("media.op.{}", self.id().split(':').next().unwrap_or_default()), &[]),
        }
    }

    /// As the desktop app passes it: `mp4`, `clip:30:60`, `resize:1600`, `rotate:90`.
    pub fn id(&self) -> String {
        match self {
            Op::Frame => "frame".into(),
            Op::Waveform => "waveform".into(),
            Op::Picture => "picture".into(),
            Op::Mp4 => "mp4".into(),
            Op::Mp3 => "mp3".into(),
            Op::Flac => "flac".into(),
            Op::Audio => "audio".into(),
            Op::Clip { start, length } => format!("clip:{start}:{length}"),
            Op::Jpeg => "jpeg".into(),
            Op::Png => "png".into(),
            Op::Webp => "webp".into(),
            Op::Resize(n) => format!("resize:{n}"),
            Op::NoLocation => "no_location".into(),
            Op::Rotate(d) => format!("rotate:{d}"),
            Op::Map => "map".into(),
            Op::Geo(to) => format!("geo:{}", to.id()),
            Op::Model => "model".into(),
            Op::Stl => "stl".into(),
            Op::ThreeMf => "3mf".into(),
            Op::Board => "board".into(),
            Op::BoardThumb => "board_thumb".into(),
            Op::Kicad(to) => format!("kicad:{}", to.id()),
        }
    }

    /// `id` read back; values out of range are none (a clip of no length, a picture of 0
    /// pixels, a turn that is no quarter).
    pub fn parse(s: &str) -> Option<Op> {
        let mut p = s.split(':');
        let op = match p.next()? {
            "mp4" => Op::Mp4,
            "mp3" => Op::Mp3,
            "flac" => Op::Flac,
            "audio" => Op::Audio,
            "clip" => {
                let (start, length) = (p.next()?.parse().ok()?, p.next()?.parse().ok()?);
                (length > 0).then_some(Op::Clip { start, length })?
            }
            "jpeg" => Op::Jpeg,
            "png" => Op::Png,
            "webp" => Op::Webp,
            "resize" => Op::Resize(p.next()?.parse().ok().filter(|n| (16..=32768).contains(n))?),
            "no_location" => Op::NoLocation,
            "rotate" => Op::Rotate(p.next()?.parse().ok().filter(|d| [90, 180, 270].contains(d))?),
            "geo" => Op::Geo(crate::geo::To::parse(p.next()?)?),
            "stl" => Op::Stl,
            "3mf" => Op::ThreeMf,
            "kicad" => Op::Kicad(crate::kicad::To::parse(p.next()?)?),
            _ => return None,
        };
        p.next().is_none().then_some(op)
    }

    /// Whether `path` is a file it takes.
    pub fn takes(&self, path: &Path) -> bool {
        match self.program() {
            Program::Ffmpeg => is_media(path),
            Program::Magick => is_picture(path),
            Program::Gdal => match self {
                Op::Geo(to) => to.takes(path),
                _ => crate::geo::is_geo(path),
            },
            Program::Openscad => crate::scad::is_model(path),
            Program::Kicad => match self {
                Op::Kicad(to) => to.takes(path),
                _ => crate::kicad::is_kicad(path),
            },
        }
    }

    /// The word put in a result's name, after the original's (`photo.resized.jpg`); none for a
    /// change of format (`song.mp3`).
    fn tag(&self) -> Option<&'static str> {
        match self {
            Op::Clip { .. } => Some("clip"),
            Op::Resize(_) => Some("resized"),
            Op::NoLocation => Some("no-location"),
            Op::Rotate(_) => Some("rotated"),
            Op::Audio => Some("audio"),
            _ => None,
        }
    }

    /// The ending of what it makes of `path`; `audio` is the codec of its sound, for Audio.
    fn result_ending(&self, path: &Path, audio: Option<&str>) -> String {
        let own = ending(path);
        let keep = || if WRITES.contains(&own.as_str()) { own.clone() } else if among(path, &[HEIF, RAW, &["jpg", "jpeg"]]) { "jpg".into() } else { "png".into() };
        match self {
            Op::Frame | Op::Waveform | Op::Picture | Op::Png => "png".into(),
            Op::Map | Op::Model => "svg".into(),
            Op::Stl => "stl".into(),
            Op::ThreeMf => "3mf".into(),
            Op::Board => "pdf".into(),
            Op::BoardThumb => "svg".into(),
            Op::Kicad(to) => to.result_ending(path).into(),
            Op::Geo(to) => to.result_ending().into(),
            Op::Mp4 => "mp4".into(),
            Op::Mp3 => "mp3".into(),
            Op::Flac => "flac".into(),
            Op::Audio => match audio.unwrap_or_default() {
                "aac" => "m4a",
                "mp3" => "mp3",
                "opus" => "opus",
                "vorbis" => "ogg",
                "flac" => "flac",
                "ac3" => "ac3",
                _ => "mka",
            }
            .into(),
            // Copied as they are: the same kind of file.
            Op::Clip { .. } => if own.is_empty() { "mkv".into() } else { own },
            Op::Jpeg => "jpg".into(),
            Op::Webp => "webp".into(),
            Op::Resize(_) | Op::NoLocation | Op::Rotate(_) => keep(),
        }
    }
}

/// A clip as typed: its start and its length, each as seconds, `m:ss` or `h:mm:ss`
/// (`1:30 0:45`).
pub fn clip(text: &str) -> Option<Op> {
    let secs = |s: &str| s.split(':').try_fold(0u64, |acc, p| acc.checked_mul(60)?.checked_add(p.parse().ok()?));
    let mut w = text.split_whitespace();
    let (start, length) = (secs(w.next()?)?, secs(w.next()?)?);
    (w.next().is_none() && length > 0).then_some(Op::Clip { start, length })
}

/// The question before a run: what it writes, next to what; the originals stay.
pub fn confirm(op: &Op, files: &[PathBuf]) -> String {
    match files {
        [one] => {
            let name = one.file_name().unwrap_or_default().to_string_lossy();
            let to = target(one, op);
            t!("media.confirm_one", "name" => name, "target" => to.file_name().unwrap_or_default().to_string_lossy())
        }
        many => t!("media.confirm_many", "n" => many.len()),
    }
}

/// Where the result of `op` on `path` goes: `name.<ending>`, `name.<tag>.<ending>`, else with
/// ` 2`, ` 3` … before the ending, the first name that is free. Never the original itself.
pub fn target(path: &Path, op: &Op) -> PathBuf {
    target_for(path, op, None)
}

fn target_for(path: &Path, op: &Op, audio: Option<&str>) -> PathBuf {
    let dir = path.parent().unwrap_or(Path::new("."));
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let ext = op.result_ending(path, audio);
    let base = match op.tag() {
        Some(tag) => format!("{stem}.{tag}"),
        None => stem.into_owned(),
    };
    (1..)
        .map(|n| dir.join(if n == 1 { format!("{base}.{ext}") } else { format!("{base} {n}.{ext}") }))
        .find(|p| p != path && !p.exists())
        .expect("a free name")
}

// ---------------------------------------------------------------- the commands

/// The name the file has where the program reads it: `in.<ending>`, the ending only of
/// letters and digits (it tells ImageMagick the format), at most 8 of them.
fn inner_name(path: &Path) -> String {
    let e: String = ending(path).chars().filter(char::is_ascii_alphanumeric).take(8).collect();
    if e.is_empty() { "in".into() } else { format!("in.{e}") }
}

/// The arguments of `op`, reading `input` and writing `output` (both already safe: `file:…`
/// for ffmpeg, `in.<ending>` for ImageMagick).
fn args(program: Program, op: &Op, input: &str, output: &str, src: &Path) -> Vec<String> {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    match program {
        Program::Ffmpeg => {
            let mut a = s(&["-hide_banner", "-nostdin", "-loglevel", "error", "-nostats", "-progress", "pipe:2", "-protocol_whitelist", "file", "-format_whitelist", DEMUXERS]);
            if let Op::Clip { start, length } = op {
                a.extend(s(&["-ss", &start.to_string(), "-t", &length.to_string()]));
            }
            a.extend(s(&["-i", input]));
            a.extend(s(match op {
                // The most telling of the first 120 frames, no wider than 1280.
                Op::Frame => &["-map", "0:v:0", "-vf", "thumbnail=120,scale='min(1280,iw)':-2", "-frames:v", "1"],
                Op::Waveform => &["-filter_complex", "aformat=channel_layouts=mono,showwavespic=s=1200x240:colors=#4fa3d8", "-frames:v", "1"],
                Op::Picture => &["-frames:v", "1", "-vf", "scale='min(2048,iw)':'min(2048,ih)':force_original_aspect_ratio=decrease"],
                // H.264 and AAC, which every player plays; subtitles are left out.
                Op::Mp4 => &["-map", "0:v:0?", "-map", "0:a?", "-crf", "23", "-preset", "medium", "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "160k", "-movflags", "+faststart", "-sn"],
                Op::Mp3 => &["-map", "0:a:0", "-vn", "-q:a", "2"],
                Op::Flac => &["-map", "0:a:0", "-vn", "-c:a", "flac"],
                Op::Audio => &["-map", "0:a:0", "-vn", "-c:a", "copy"],
                Op::Clip { .. } => &["-map", "0", "-c", "copy", "-avoid_negative_ts", "make_zero"],
                _ => &[],
            }));
            a.extend(s(&["-y", output]));
            a
        }
        Program::Magick => {
            let mut a = s(&["-limit", "memory", "1GiB", "-limit", "map", "2GiB", "-limit", "disk", "4GiB", "-limit", "time", "300"]);
            // A GIMP file is its layers, merged; anything else its first page or frame.
            if ending(src) == "xcf" {
                a.extend(s(&[input, "-background", "none", "-layers", "merge"]));
            } else {
                a.push(format!("{input}[0]"));
            }
            a.push("-auto-orient".into());
            let jpeg = output.ends_with(".jpg") || output.ends_with(".jpeg");
            match op {
                Op::Picture => a.extend(s(&["-resize", "2048x2048>"])),
                Op::Resize(n) => a.extend(s(&["-resize", &format!("{n}x{n}>")])),
                Op::Rotate(d) => a.extend(s(&["-rotate", &d.to_string()])),
                // Every profile and note but the colour profile: the location, the camera,
                // the date. The picture is already turned upright (`-auto-orient`).
                Op::NoLocation => a.extend(s(&["+profile", "!icc,*"])),
                _ => {}
            }
            if jpeg || matches!(op, Op::Webp) {
                a.extend(s(&["-quality", if jpeg { "92" } else { "90" }]));
            }
            a.push(output.into());
            a
        }
        // GDAL's and OpenSCAD's runs are `geo`'s and `scad`'s.
        Program::Gdal | Program::Openscad | Program::Kicad => vec![],
    }
}

/// The policy an installed ImageMagick runs under: no delegate programs, no coder but those of
/// the pictures asked for and the results, no `@file` arguments, and bounded resources.
fn policy() -> String {
    // By ending and by the coders' own names (`JPG` writes, `JPEG` reads).
    let mut coders: Vec<String> = [PICTURES, HEIF, MAGICK_ONLY, RAW, &["fits", "sun", "pict"]].concat().iter().map(|e| e.to_ascii_uppercase()).collect();
    coders.sort();
    coders.dedup();
    format!(
        "<policymap>\n  <policy domain=\"delegate\" rights=\"none\" pattern=\"*\"/>\n  <policy domain=\"coder\" rights=\"none\" pattern=\"*\"/>\n  <policy domain=\"coder\" rights=\"read|write\" pattern=\"{{{}}}\"/>\n  <policy domain=\"path\" rights=\"none\" pattern=\"@*\"/>\n  <policy domain=\"resource\" name=\"memory\" value=\"1GiB\"/>\n  <policy domain=\"resource\" name=\"map\" value=\"2GiB\"/>\n  <policy domain=\"resource\" name=\"disk\" value=\"4GiB\"/>\n  <policy domain=\"resource\" name=\"area\" value=\"256MP\"/>\n  <policy domain=\"resource\" name=\"width\" value=\"32KP\"/>\n  <policy domain=\"resource\" name=\"height\" value=\"32KP\"/>\n  <policy domain=\"resource\" name=\"time\" value=\"300\"/>\n</policymap>\n",
        coders.join(",")
    )
}

/// The folder holding `policy.xml`, written when missing or changed.
fn policy_dir() -> Result<PathBuf, String> {
    let dir = crate::dirs::cache_dir().ok_or_else(|| t!("err.no_cache_folder"))?.join("coxswain").join("magick");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let file = dir.join("policy.xml");
    let want = policy();
    if std::fs::read_to_string(&file).ok().as_deref() != Some(want.as_str()) {
        std::fs::write(&file, want).map_err(|e| e.to_string())?;
    }
    Ok(dir)
}

/// What a run says while it goes.
pub enum Said<'a> {
    /// The image is being pulled: its progress line.
    Pulling(&'a str),
    /// How far ffmpeg is, in percent of the file's length.
    Percent(u8),
    /// A line the program wrote.
    Line(&'a str),
}

static N: AtomicUsize = AtomicUsize::new(0);

/// A name of its own for a container, so a run that is stopped can be killed.
fn container_name() -> String {
    format!("coxswain-media-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed))
}

/// The image of a container `how`, pulled when it is not there yet; `stop` ends the pull.
fn pulled(how: &How, stop: &AtomicBool, said: &mut dyn FnMut(Said)) -> Result<(), String> {
    let How::Container { path: rt_path, image, .. } = how else { return Ok(()) };
    if crate::tools::image_size(rt_path, image).is_some() {
        return Ok(());
    }
    // Stop ends the pull too: it runs on a scoped thread that watches `stop`.
    std::thread::scope(|scope| {
        let puller = scope.spawn(|| crate::images::pull_until(rt_path, image, stop));
        while !puller.is_finished() {
            if let Some(line) = crate::images::pulling().get(image.as_str()) {
                said(Said::Pulling(line));
            }
            std::thread::sleep(Duration::from_millis(300));
        }
        puller.join().map_err(|_| t!("convert.failed", "status" => "pull"))?
    })
}

/// Run `c`, a container named `kill` when it is one: what it writes on its standard error goes
/// to `said` as it comes (ffmpeg's `-progress` lines as a percent of `seconds`), and its
/// standard output is kept, at most `keep` bytes. Stopped by `stop`; past `timeout`, killed.
/// When it fails, the last lines it wrote say why.
fn watch(
    c: std::process::Command,
    kill: Option<(PathBuf, String)>,
    stop: &AtomicBool,
    timeout: Option<Duration>,
    seconds: Option<f64>,
    said: &mut dyn FnMut(Said),
    keep: u64,
) -> Result<Vec<u8>, String> {
    let mut c = crate::tools::hosted(c);
    let mut child = c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
    let lines = crate::searchable::follow(&mut child);
    let mut so = child.stdout.take().ok_or("stdout")?;
    let reader = std::thread::spawn(move || {
        let mut b = vec![];
        let _ = std::io::Read::read_to_end(&mut std::io::Read::take(&mut so, keep), &mut b);
        // What is past `keep` is read and dropped, so the program is never held up writing it.
        let _ = std::io::copy(&mut so, &mut std::io::sink());
        b
    });
    let (mut last, start) = (Vec::<String>::new(), Instant::now());
    let note = |l: String, said: &mut dyn FnMut(Said), last: &mut Vec<String>| {
        // ffmpeg's `-progress` lines are `key=value`; the rest is what it has to say.
        if let Some(us) = l.strip_prefix("out_time_us=").and_then(|v| v.parse::<f64>().ok()) {
            if let Some(total) = seconds.filter(|s| *s > 0.0) {
                said(Said::Percent((us / 1e6 / total * 100.0).clamp(0.0, 99.0) as u8));
            }
        } else if !(l.contains('=') && !l.contains(' ')) {
            said(Said::Line(&l));
            last.push(l);
            if last.len() > 12 {
                last.remove(0);
            }
        }
    };
    let status = loop {
        while let Ok(l) = lines.try_recv() {
            note(l, said, &mut last);
        }
        let late = timeout.is_some_and(|t| start.elapsed() > t);
        if stop.load(Ordering::Relaxed) || late {
            let _ = child.kill();
            if let Some((rt, name)) = &kill {
                let _ = crate::tools::command(rt).args(["kill", name]).stdout(Stdio::null()).stderr(Stdio::null()).status();
            }
            let _ = child.wait();
            return Err(if late { t!("convert.timed_out", "seconds" => timeout.unwrap_or_default().as_secs()) } else { t!("media.stopped") });
        }
        if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
            break s;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    while let Ok(l) = lines.try_recv() {
        note(l, said, &mut last);
    }
    let out = reader.join().unwrap_or_default();
    if status.success() {
        return Ok(out);
    }
    let tail = last.iter().rev().take(6).rev().cloned().collect::<Vec<_>>().join("\n");
    Err(if tail.is_empty() { t!("convert.failed", "status" => status) } else { tail })
}

/// Make `op` of `path` into `scratch` with `how`: the file it wrote there. Stopped by `stop`;
/// past `timeout` (previews), killed. `seconds`: the file's length, for ffmpeg's percent.
#[allow(clippy::too_many_arguments)]
fn make(
    how: &How,
    op: &Op,
    path: &Path,
    scratch: &Path,
    ext: &str,
    seconds: Option<f64>,
    stop: &AtomicBool,
    timeout: Option<Duration>,
    said: &mut dyn FnMut(Said),
) -> Result<PathBuf, String> {
    let name = inner_name(path);
    let out_name = format!("out.{ext}");
    let out = scratch.join(&out_name);
    if how.program() == Program::Gdal {
        in_folder(how, path, scratch, stop, timeout, said, crate::geo::ENV, |run| match op {
            Op::Geo(to) => crate::geo::convert(run, path, *to, &out_name),
            _ => std::fs::write(&out, crate::geo::map(run, path)?).map_err(|e| e.to_string()),
        })?;
        return if out.is_file() { Ok(out) } else { Err(t!("convert.no_result")) };
    }
    if how.program() == Program::Openscad {
        let manifold = crate::scad::manifold(how);
        in_folder(how, path, scratch, stop, timeout, said, &[], |run| match op {
            Op::Model => std::fs::write(&out, crate::scad::picture_of(run, manifold)?).map_err(|e| e.to_string()),
            _ => crate::scad::export(run, op == &Op::Stl, &out_name, manifold),
        })?;
        return if out.is_file() { Ok(out) } else { Err(t!("convert.no_result")) };
    }
    if how.program() == Program::Kicad {
        in_folder(how, path, scratch, stop, timeout, said, &[], |run| match op {
            Op::Kicad(to) => crate::kicad::export(run, path, *to, &out_name),
            Op::BoardThumb => std::fs::write(&out, crate::kicad::thumbnail(run, path)?).map_err(|e| e.to_string()),
            _ => crate::kicad::preview(run, path, &out_name),
        })?;
        return if out.is_file() { Ok(out) } else { Err(t!("convert.no_result")) };
    }
    pulled(how, stop, said)?;
    let mut kill = None;
    let c = match how {
        How::Local { program: Program::Ffmpeg, path: program } => {
            let mut c = crate::tools::low(program);
            let input = format!("file:{}", std::path::absolute(path).map_err(|e| e.to_string())?.display());
            c.args(args(Program::Ffmpeg, op, &input, &format!("file:{}", out.display()), path));
            c
        }
        How::Local { path: program, .. } => {
            // A link to the file (a copy where none can be made) named `in.<ending>`.
            let linked = scratch.join(&name);
            if std::fs::hard_link(path, &linked).is_err() {
                std::fs::copy(path, &linked).map_err(|e| format!("{}: {e}", path.display()))?;
            }
            let mut c = crate::tools::low(program);
            c.current_dir(scratch).env("MAGICK_CONFIGURE_PATH", policy_dir()?);
            c.args(args(Program::Magick, op, &name, &out_name, path));
            c
        }
        How::Container { program, rt, path: rt_path, image } => {
            let container = container_name();
            let at = format!("/src/{name}");
            let mut c = crate::images::sealed_file(rt, rt_path, &container, path, &at, Some(scratch));
            match program {
                Program::Ffmpeg => {
                    c.args(["--entrypoint", "/ffmpeg", image.as_str()]);
                    c.args(args(Program::Ffmpeg, op, &format!("file:{at}"), &format!("file:/out/{out_name}"), path));
                }
                _ => {
                    // The same policy as for an installed one, read-only beside the file.
                    c.arg("--mount").arg(crate::images::bind(&policy_dir()?, "/policy", true));
                    c.args(["-e", "MAGICK_CONFIGURE_PATH=/policy", "-w", "/out", "--entrypoint", "magick", image.as_str()]);
                    c.args(args(Program::Magick, op, &at, &format!("/out/{out_name}"), path));
                }
            }
            kill = Some((rt_path.clone(), container));
            c
        }
    };
    watch(c, kill, stop, timeout, seconds, said, 0).map_err(|tail| {
        if tail.contains("NoDecodeDelegate") || tail.contains("no decode delegate") {
            format!("{}\n{tail}", t!("media.cannot_read", "ending" => ending(path)))
        } else {
            tail
        }
    })?;
    if out.is_file() { Ok(out) } else { Err(t!("convert.no_result")) }
}

/// A PNG picture `w` × `h` as an SVG one, so a preview's result is one kind of file.
pub(crate) fn png_in_svg(png: &[u8], w: u32, h: u32) -> String {
    use base64::Engine;
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\"><image width=\"{w}\" height=\"{h}\" href=\"data:image/png;base64,{}\"/></svg>",
        base64::engine::general_purpose::STANDARD.encode(png)
    )
}

/// What `in_folder`'s programs may write on their standard output, kept.
const KEEP: u64 = 4 << 20;

pub(crate) type Exec<'a> = dyn FnMut(&str, &[String]) -> Result<Vec<u8>, String> + 'a;

/// How the steps of `in_folder` reach their program: `exec(program, args)` runs it (installed,
/// or in the image) and gives what it wrote on its standard output, at most `KEEP`; `input` is
/// the file as the program sees it, `out` the scratch folder as it sees it, and `scratch` that
/// folder here.
pub(crate) struct Run<'a> {
    pub exec: &'a mut Exec<'a>,
    pub input: String,
    pub out: String,
    pub scratch: &'a Path,
}

impl Run<'_> {
    pub fn go(&mut self, program: &str, args: &[&str]) -> Result<Vec<u8>, String> {
        (self.exec)(program, &args.iter().map(|a| a.to_string()).collect::<Vec<_>>())
    }

    pub fn out(&self, name: &str) -> String {
        format!("{}/{name}", self.out)
    }
}

/// `steps` on `path`, each program they name run by `how`: installed, with `env`; or in the
/// image, which sees the file's folder read-only (a shapefile is several files side by side, a
/// model includes others beside it) and writes only into `scratch`, held as tightly as
/// ffmpeg's and ImageMagick's, without network. For GDAL, OpenSCAD and KiCad.
#[allow(clippy::too_many_arguments)]
pub(crate) fn in_folder<R>(
    how: &How,
    path: &Path,
    scratch: &Path,
    stop: &AtomicBool,
    timeout: Option<Duration>,
    said: &mut dyn FnMut(Said),
    env: &[(&str, &str)],
    steps: impl FnOnce(&mut Run) -> Result<R, String>,
) -> Result<R, String> {
    pulled(how, stop, said)?;
    let abs = std::path::absolute(path).map_err(|e| e.to_string())?;
    let dir = abs.parent().ok_or_else(|| t!("err.no_folder"))?.to_path_buf();
    // The folder only for the kinds made of files side by side; the others see their one file,
    // under a name of letters, digits, `.`, `_` and `-` that starts with none of the signs.
    let whole = matches!(abs.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref(), Some("shp" | "scad" | "kicad_sch" | "kicad_pcb"));
    // ponytail: a name that is no UTF-8 reaches the container changed, and is not found there.
    let file = abs.file_name().ok_or_else(|| t!("err.no_file_name"))?.to_string_lossy().into_owned();
    let alone: String = file.chars().map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '_' }).collect();
    // A plain name stays as it is (GeoJSON names its layer after it); any other gets an `f`.
    let alone = if alone == file && !file.starts_with('-') { format!("/src/{alone}") } else { format!("/src/f{alone}") };
    let (input, out) = match how {
        How::Local { .. } => (abs.display().to_string(), scratch.display().to_string()),
        How::Container { .. } if whole => (format!("/src/{file}"), "/out".to_string()),
        How::Container { .. } => (alone.clone(), "/out".to_string()),
    };
    // One time limit for all the steps of a preview, not one for each.
    let deadline = timeout.map(|t| Instant::now() + t);
    let mut exec = |program: &str, args: &[String]| -> Result<Vec<u8>, String> {
        let left = deadline.map(|d| d.saturating_duration_since(Instant::now()));
        if left.is_some_and(|l| l.is_zero()) {
            return Err(t!("convert.timed_out", "seconds" => timeout.unwrap_or_default().as_secs()));
        }
        let (c, kill) = match how {
            How::Local { .. } => {
                let p = crate::tools::which(program).ok_or_else(|| t!("convert.not_installed", "program" => program))?;
                let mut c = crate::tools::low(&p);
                c.args(args).envs(env.iter().copied());
                (c, None)
            }
            How::Container { rt, path: rt_path, image, .. } => {
                let name = container_name();
                let mut c = if whole {
                    crate::images::sealed(rt, rt_path, &name, &dir, Some(scratch))
                } else {
                    crate::images::sealed_file(rt, rt_path, &name, &abs, &alone, Some(scratch))
                };
                for (k, v) in env {
                    c.arg("-e").arg(format!("{k}={v}"));
                }
                c.args(["--entrypoint", program, image.as_str()]).args(args);
                (c, Some((rt_path.clone(), name)))
            }
        };
        watch(c, kill, stop, left, None, &mut *said, KEEP)
    };
    steps(&mut Run { exec: &mut exec, input, out, scratch })
}

/// A preview of `path` (`Op::Frame`, `Waveform` or `Picture`, a PNG; `Map`, an SVG) made with
/// `how` into `file`, within `timeout`.
pub fn preview(how: &How, op: &Op, path: &Path, file: &Path, timeout: Duration) -> Result<(), String> {
    let scratch = crate::searchable::Scratch::named("media")?;
    let made = make(how, op, path, &scratch.0, &op.result_ending(path, None), None, &AtomicBool::new(false), Some(timeout), &mut |_| {})?;
    std::fs::copy(made, file).map(drop).map_err(|e| e.to_string())
}

/// Do `op` on each of `files`, one after the other: `at(i)` as each starts, `said` with what
/// it says. `stop` ends the one under way, and the rest are not started; what a stopped run
/// made is removed. One result per file started: the file written next to it, or why not.
pub fn run_all(
    cfg: &PreviewConfig,
    op: &Op,
    files: &[PathBuf],
    stop: &AtomicBool,
    mut at: impl FnMut(usize),
    mut said: impl FnMut(Said),
) -> Vec<Result<PathBuf, String>> {
    let how = match how(cfg, op.program()) {
        Ok(h) => h,
        Err(e) => return vec![Err(e)],
    };
    let mut done = vec![];
    for (i, file) in files.iter().enumerate() {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        at(i);
        done.push(run(cfg, &how, op, file, stop, &mut said));
    }
    done
}

/// One file. A folder that cannot be written to says so before the program starts (a probe
/// file, removed at once); the result's name is taken only when it is put there, with
/// `create_new` (a hard link, else a copy), so nothing is overwritten and an app that ends
/// mid-run leaves no empty file.
fn run(cfg: &PreviewConfig, how: &How, op: &Op, file: &Path, stop: &AtomicBool, said: &mut dyn FnMut(Said)) -> Result<PathBuf, String> {
    let probe = target(file, op);
    std::fs::OpenOptions::new().write(true).create_new(true).open(&probe).map_err(|e| format!("{}: {e}", probe.display()))?;
    let _ = std::fs::remove_file(&probe);
    // The length (for the percent) and the sound's codec (for Audio's ending).
    let facts = if op.program() == Program::Ffmpeg { probe_file(cfg, file) } else { None };
    let audio = facts.as_ref().and_then(|p| p.streams.iter().find(|s| s.kind == "audio")).map(|s| s.codec.clone());
    if matches!(op, Op::Audio | Op::Mp3 | Op::Flac) && facts.as_ref().is_some_and(|p| !p.streams.iter().any(|s| s.kind == "audio")) {
        return Err(t!("media.no_sound"));
    }
    let ext = op.result_ending(file, audio.as_deref());
    let scratch = crate::searchable::Scratch::named("media")?;
    let made = make(how, op, file, &scratch.0, &ext, facts.as_ref().map(|p| p.seconds), stop, None, said)?;
    put(&made, || target_for(file, op, audio.as_deref()))
}

/// `made` under the first free name `name` gives, with `create_new`: a hard link where it can
/// be one, else a copy; a copy cut short is removed.
fn put(made: &Path, name: impl Fn() -> PathBuf) -> Result<PathBuf, String> {
    for _ in 0..100 {
        let to = name();
        match std::fs::hard_link(made, &to) {
            Ok(()) => return Ok(to),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => {}
        }
        let Ok(mut f) = std::fs::OpenOptions::new().write(true).create_new(true).open(&to) else { continue };
        let copied = std::fs::File::open(made).and_then(|mut m| std::io::copy(&mut m, &mut f));
        return match copied {
            Ok(_) => Ok(to),
            Err(e) => {
                drop(f);
                let _ = std::fs::remove_file(&to);
                Err(format!("{}: {e}", to.display()))
            }
        };
    }
    Err(t!("err.no_file_name"))
}

// ---------------------------------------------------------------- facts

/// What ffprobe says of one stream.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stream {
    /// "video", "audio" or "subtitle".
    pub kind: String,
    pub codec: String,
    pub width: u64,
    pub height: u64,
    /// Frames a second.
    pub fps: f64,
    pub channels: u64,
    pub rate: u64,
    pub language: String,
}

/// What ffprobe says of a file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Probe {
    pub format: String,
    pub seconds: f64,
    /// Bits a second.
    pub bitrate: u64,
    pub streams: Vec<Stream>,
}

/// ffprobe's JSON read into a `Probe`.
fn parse_probe(json: &str) -> Option<Probe> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let num = |x: &serde_json::Value| x.as_str().and_then(|s| s.parse::<f64>().ok()).or_else(|| x.as_f64());
    let f = &v["format"];
    let streams = v["streams"]
        .as_array()?
        .iter()
        .filter(|s| s["disposition"]["attached_pic"].as_u64() != Some(1))
        .map(|s| {
            let fps = s["avg_frame_rate"].as_str().and_then(|r| {
                let (a, b) = r.split_once('/')?;
                let (a, b) = (a.parse::<f64>().ok()?, b.parse::<f64>().ok()?);
                (b > 0.0).then(|| a / b)
            });
            Stream {
                kind: s["codec_type"].as_str().unwrap_or_default().into(),
                codec: s["codec_name"].as_str().unwrap_or_default().into(),
                width: s["width"].as_u64().unwrap_or(0),
                height: s["height"].as_u64().unwrap_or(0),
                fps: fps.unwrap_or(0.0),
                channels: s["channels"].as_u64().unwrap_or(0),
                rate: num(&s["sample_rate"]).unwrap_or(0.0) as u64,
                language: s["tags"]["language"].as_str().filter(|l| *l != "und").unwrap_or_default().into(),
            }
        })
        .collect();
    Some(Probe {
        format: f["format_long_name"].as_str().unwrap_or_default().into(),
        seconds: num(&f["duration"]).unwrap_or(0.0),
        bitrate: num(&f["bit_rate"]).unwrap_or(0.0) as u64,
        streams,
    })
}

/// ffprobe on `path`: the installed one, else in ffmpeg's image when that is pulled (never
/// pulled for this). Twenty seconds at most; `None` when neither is there or it says nothing.
pub fn probe_file(cfg: &PreviewConfig, path: &Path) -> Option<Probe> {
    let base = ["-v", "error", "-print_format", "json", "-show_format", "-show_streams", "-protocol_whitelist", "file", "-format_whitelist", DEMUXERS];
    let local = crate::tools::which("ffprobe").filter(|_| crate::preview_tools::asked(cfg, FFMPEG) != "container");
    let (c, kill) = match local {
        Some(p) => {
            let mut c = crate::tools::command(p);
            c.args(base).arg(format!("file:{}", std::path::absolute(path).ok()?.display()));
            (c, None)
        }
        None => {
            if crate::preview_tools::asked(cfg, FFMPEG) == "local" {
                return None;
            }
            let (rt, rt_path) = crate::tools::container_runtime(cfg)?;
            let image = cfg.images.get(FFMPEG).filter(|i| !i.is_empty())?;
            crate::tools::image_size(&rt_path, image)?;
            let name = format!("coxswain-probe-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed));
            let at = format!("/src/{}", inner_name(path));
            let mut c = crate::images::sealed_file(rt, &rt_path, &name, path, &at, None);
            c.args(["--entrypoint", "/ffprobe", image.as_str()]).args(base).arg(format!("file:{at}"));
            (c, Some((rt_path, name)))
        }
    };
    let mut c = crate::tools::hosted(c);
    let mut child = c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut so = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = std::io::Read::read_to_string(&mut std::io::Read::take(&mut so, 4 << 20), &mut s);
        s
    });
    let status = crate::tools::wait(&mut child, Duration::from_secs(20));
    if status.is_err()
        && let Some((rt, name)) = kill
    {
        let _ = crate::tools::command(rt).args(["kill", &name]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    let json = reader.join().ok()?;
    status.ok()?.success().then(|| parse_probe(&json)).flatten()
}

/// "1:02:03" or "4:05".
fn length(secs: f64) -> String {
    let s = secs.round() as u64;
    if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) }
}

/// A probe as facts: format, length, size in pixels, the video, each sound track, subtitles.
pub fn facts_of(p: &Probe) -> Vec<(String, String)> {
    let mut out = vec![];
    if !p.format.is_empty() {
        out.push((t!("facts.format"), p.format.clone()));
    }
    if p.seconds > 0.0 {
        out.push((t!("facts.length"), length(p.seconds)));
    }
    if let Some(v) = p.streams.iter().find(|s| s.kind == "video") {
        if v.width > 0 {
            out.push((t!("facts.dimensions"), format!("{} × {}", v.width, v.height)));
        }
        let fps = if v.fps > 0.0 { format!(", {} fps", (v.fps * 100.0).round() / 100.0) } else { String::new() };
        out.push((t!("facts.video"), format!("{}{fps}", v.codec)));
    }
    let tracks: Vec<String> = p
        .streams
        .iter()
        .filter(|s| s.kind == "audio")
        .map(|a| {
            let ch = match a.channels {
                0 => String::new(),
                1 => format!(", {}", t!("facts.mono")),
                2 => format!(", {}", t!("facts.stereo")),
                n => format!(", {}", crate::tn!("facts.channels", n)),
            };
            let lang = if a.language.is_empty() { String::new() } else { format!(" ({})", a.language) };
            format!("{}{ch}{lang}", a.codec)
        })
        .collect();
    if !tracks.is_empty() {
        out.push((t!("facts.audio"), tracks.join("; ")));
    }
    let subs = p.streams.iter().filter(|s| s.kind == "subtitle").count();
    if subs > 0 {
        out.push((t!("facts.subtitles"), subs.to_string()));
    }
    if p.bitrate > 0 {
        out.push((t!("facts.bitrate"), format!("{} kbps", p.bitrate / 1000)));
    }
    out
}

/// `p` with a video's facts (`facts`) when it has none of its own, for Properties in both apps.
pub fn with_facts(mut p: crate::fs::Props, cfg: &PreviewConfig, path: &Path) -> crate::fs::Props {
    if p.facts.is_none() && is_media(path) {
        p.facts = Some(facts(cfg, path)).filter(|f| !f.is_empty());
    }
    // Geodata's, by GDAL (`geo`); a GeoTIFF's after the picture's own.
    if crate::geo::is_geo(path) {
        let geo = crate::geo::facts(cfg, path);
        if !geo.is_empty() {
            crate::geo::merge(p.facts.get_or_insert_with(Vec::new), geo);
        }
    }
    p
}

/// The facts of a video, or of a song the tags reader does not know, by ffprobe; empty for
/// anything else, without ffprobe, or for a file only in the cloud.
pub fn facts(cfg: &PreviewConfig, path: &Path) -> Vec<(String, String)> {
    // Video and sound *Off* in Preview tools: ffprobe does not run by itself either.
    if crate::cloud::unasked(path) || !among(path, &[VIDEO, VIDEO_OTHER, AUDIO_OTHER]) || crate::preview_tools::off(cfg, FFMPEG) {
        return vec![];
    }
    probe_file(cfg, path).map(|p| facts_of(&p)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Our ImageMagick image bakes in the policy an installed one runs under: the same file.
    /// Read from the repository, so a published crate, which has no `packaging/`, skips it.
    #[test]
    fn media_image_policy_is_the_installed_ones() {
        let baked = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/images/imagemagick/policy.xml");
        if let Ok(baked) = std::fs::read_to_string(&baked) {
            // A Windows checkout may have turned its line ends into CRLF.
            assert_eq!(baked.replace('\r', ""), policy(), "packaging/images/imagemagick/policy.xml: write policy() into it");
        }
    }

    #[test]
    fn media_ops_read_back_and_refuse_nonsense() {
        for op in Op::menu(Program::Ffmpeg).into_iter().chain(Op::menu(Program::Magick)).chain([Op::Clip { start: 90, length: 15 }, Op::Resize(800)]) {
            assert_eq!(Op::parse(&op.id()), Some(op.clone()), "{op:?}");
        }
        for bad in ["clip:10:0", "clip:x:1", "resize:0", "resize:999999", "rotate:45", "mp4:1", "frame", "", "rm"] {
            assert_eq!(Op::parse(bad), None, "{bad}");
        }
        assert_eq!(clip("1:30 0:45"), Some(Op::Clip { start: 90, length: 45 }));
        assert_eq!(clip(" 1:00:00  10 "), Some(Op::Clip { start: 3600, length: 10 }));
        for bad in ["", "10", "10 0", "a b", "1 2 3", "-5 10"] {
            assert_eq!(clip(bad), None, "{bad}");
        }
    }

    #[test]
    fn media_results_never_take_a_name_in_use_or_the_original() {
        let d = std::env::temp_dir().join(format!("coxswain-media-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let song = d.join("song.flac");
        std::fs::write(&song, "x").unwrap();
        assert_eq!(target(&song, &Op::Mp3), d.join("song.mp3"));
        assert_eq!(target(&song, &Op::Flac), d.join("song 2.flac"), "never the original itself");
        std::fs::write(d.join("song.mp3"), "").unwrap();
        assert_eq!(target(&song, &Op::Mp3), d.join("song 2.mp3"));
        assert_eq!(target(&d.join("a.mkv"), &Op::Clip { start: 0, length: 5 }), d.join("a.clip.mkv"));
        assert_eq!(target(&d.join("IMG_1.HEIC"), &Op::Resize(800)), d.join("IMG_1.resized.jpg"), "HEIC is not written: JPEG");
        assert_eq!(target(&d.join("art.xcf"), &Op::Rotate(90)), d.join("art.rotated.png"));
        assert_eq!(target(&d.join("p.png"), &Op::NoLocation), d.join("p.no-location.png"));
        assert_eq!(target_for(&d.join("v.mp4"), &Op::Audio, Some("aac")), d.join("v.audio.m4a"));
        assert_eq!(target_for(&d.join("v.avi"), &Op::Audio, Some("pcm_s16le")), d.join("v.audio.mka"));
        // put: a name taken meanwhile is skipped, never written over.
        let made = d.join("made");
        std::fs::write(&made, "new").unwrap();
        let taken = d.join("taken.mp3");
        std::fs::write(&taken, "old").unwrap();
        let names = std::cell::Cell::new(0);
        let to = put(&made, || {
            names.set(names.get() + 1);
            if names.get() == 1 { taken.clone() } else { d.join("free.mp3") }
        })
        .unwrap();
        assert_eq!(to, d.join("free.mp3"));
        assert_eq!(std::fs::read_to_string(&taken).unwrap(), "old");
        assert_eq!(std::fs::read_to_string(&to).unwrap(), "new");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn media_names_are_never_arguments() {
        assert_eq!(inner_name(Path::new("/x/-rf --help.HEIC")), "in.heic");
        assert_eq!(inner_name(Path::new("/x/a.j$(id)pg")), "in.jidpg");
        assert_eq!(inner_name(Path::new("/x/noending")), "in");
        let a = args(Program::Ffmpeg, &Op::Mp4, "file:/src/in.avi", "file:/out/out.mp4", Path::new("a.avi"));
        let i = a.iter().position(|x| x == "-i").unwrap();
        assert_eq!(a[i + 1], "file:/src/in.avi");
        assert!(a.windows(2).any(|w| w == ["-protocol_whitelist", "file"]));
        assert!(a.iter().any(|x| x == "-format_whitelist") && !DEMUXERS.split(',').any(|d| ["hls", "concat", "image2"].contains(&d)));
        let m = args(Program::Magick, &Op::Rotate(90), "in.jpg", "out.jpg", Path::new("x.jpg"));
        assert_eq!(&m[m.len() - 1], "out.jpg");
        assert!(m.contains(&"in.jpg[0]".to_string()));
        let p = policy();
        for want in ["domain=\"delegate\" rights=\"none\" pattern=\"*\"", "domain=\"coder\" rights=\"none\" pattern=\"*\"", "JPEG", "HEIC", "XCF", "DNG", "pattern=\"@*\""] {
            assert!(p.contains(want), "{want}");
        }
        assert!(!p.contains("SVG") && !p.contains("MSL") && !p.contains("TEXT"));
    }

    #[test]
    fn media_facts_from_ffprobe() {
        crate::i18n::set_language("en-GB");
        let json = r#"{"streams":[{"codec_type":"video","codec_name":"h264","width":1920,"height":1080,"avg_frame_rate":"30000/1001"},
            {"codec_type":"audio","codec_name":"aac","channels":2,"sample_rate":"48000","tags":{"language":"eng"}},
            {"codec_type":"audio","codec_name":"ac3","channels":6,"sample_rate":"48000","tags":{"language":"und"}},
            {"codec_type":"video","codec_name":"mjpeg","disposition":{"attached_pic":1}},
            {"codec_type":"subtitle","codec_name":"subrip"}],
            "format":{"format_long_name":"Matroska / WebM","duration":"3725.4","bit_rate":"4200000"}}"#;
        let p = parse_probe(json).unwrap();
        assert_eq!(p.streams.len(), 4, "a cover picture is no stream of the video");
        let f = facts_of(&p);
        let get = |k: &str| f.iter().find(|(l, _)| *l == t!(k)).map(|(_, v)| v.as_str());
        assert_eq!(get("facts.length"), Some("1:02:05"));
        assert_eq!(get("facts.dimensions"), Some("1920 × 1080"));
        assert_eq!(get("facts.video"), Some("h264, 29.97 fps"));
        assert_eq!(get("facts.audio"), Some("aac, stereo (eng); ac3, 6 channels"));
        assert_eq!(get("facts.subtitles"), Some("1"));
        assert_eq!(length(65.0), "1:05");
        assert!(parse_probe("not json").is_none());
    }

    #[test]
    fn media_imagemagick_prefers_its_container_ffmpeg_the_installed_one() {
        crate::i18n::set_language("en-GB");
        let cfg = PreviewConfig { container: "off".into(), ..Default::default() };
        for p in [Program::Ffmpeg, Program::Magick] {
            match how(&cfg, p) {
                Ok(How::Local { .. }) => {}
                Ok(other) => panic!("containers are off: {other:?}"),
                Err(e) => assert!(e.contains(&t!("searchable.containers_off")), "{e}"),
            }
        }
        assert!(is_media(Path::new("a.WMV")) && is_video(Path::new("a.mkv")) && !is_video(Path::new("a.flac")));
        assert!(is_picture(Path::new("IMG.HEIC")) && is_picture(Path::new("a.cr2")) && !is_picture(Path::new("a.svg")));
    }

    /// Needs ffmpeg and ImageMagick installed: `cargo test -p coxswain-core -- --ignored real_media_local`.
    /// With `COXSWAIN_MEDIA_CONTAINER=1` the same in their containers (podman, images pulled).
    #[test]
    #[ignore]
    fn real_media_local() {
        let d = std::env::temp_dir().join(format!("coxswain-real-media-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let ok = |c: &mut std::process::Command| assert!(c.status().unwrap().success());
        let video = d.join("-v clip.avi");
        ok(std::process::Command::new("ffmpeg").args(["-v", "error", "-f", "lavfi", "-i", "testsrc2=d=4:s=320x240", "-f", "lavfi", "-i", "sine=d=4", "-c:v", "mpeg4", "-c:a", "mp2"]).arg(&video));
        let container = std::env::var("COXSWAIN_MEDIA_CONTAINER").is_ok();
        let photo = d.join("--photo.heic");
        ok(std::process::Command::new("magick").args(["-size", "400x300", "plasma:"]).arg(&photo));
        // Installed ones only, unless asked: a test never pulls into the machine's own store.
        let mut cfg = PreviewConfig { prefer: "local".into(), ..Default::default() };
        if container {
            cfg.prefer = "container".into();
        }
        let stop = AtomicBool::new(false);
        for (op, file, want) in [
            (Op::Mp4, &video, "-v clip.mp4"),
            (Op::Mp3, &video, "-v clip.mp3"),
            (Op::Audio, &video, "-v clip.audio.mka"),
            (Op::Clip { start: 1, length: 2 }, &video, "-v clip.clip.avi"),
            (Op::Jpeg, &photo, "--photo.jpg"),
            (Op::Rotate(90), &photo, "--photo.rotated"),
        ] {
            let mut percent = 0;
            let r = run_all(&cfg, &op, std::slice::from_ref(file), &stop, |_| {}, |s| {
                if let Said::Percent(p) = s {
                    percent = p;
                }
            });
            let made = r.into_iter().next().unwrap().unwrap_or_else(|e| panic!("{op:?}: {e}"));
            eprintln!("{op:?}: {} ({} bytes, last {percent} %)", made.display(), std::fs::metadata(&made).unwrap().len());
            assert!(made.starts_with(&d) && made.is_file(), "{want}");
        }
        let p = probe_file(&cfg, &video).expect("ffprobe");
        assert!(p.seconds > 3.0 && p.streams.iter().any(|s| s.kind == "video"));
        for (how_of, op, file) in [(Program::Ffmpeg, Op::Frame, &video), (Program::Ffmpeg, Op::Waveform, &video), (Program::Magick, Op::Picture, &photo)] {
            let png = d.join(format!("{}.png", op.id()));
            preview(&how(&cfg, how_of).unwrap(), &op, file, &png, Duration::from_secs(60)).unwrap();
            assert!(std::fs::read(&png).unwrap().starts_with(b"\x89PNG"));
        }
        std::fs::remove_dir_all(d).unwrap();
    }
}
