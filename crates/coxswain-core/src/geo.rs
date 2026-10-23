//! Geodata, in both apps: GeoPackage, shapefiles, GeoJSON, KML, GPX and GeoTIFF. Their facts
//! (format, layers and how many features each has, the coordinate system, the extent; a GPX
//! file's tracks and routes with their length and time), the map the desktop app's preview and
//! thumbnails show of the file's own shapes (no background, nothing fetched), and *Convert
//! geodata* (to GeoPackage, GeoJSON, a shapefile, KML or GPX; a GeoTIFF to a cloud-optimised
//! GeoTIFF or PNG).
//!
//! GDAL does the reading and converting (`gdalinfo`, `ogrinfo`, `ogr2ogr`, `gdal_translate`),
//! installed first, else from its container image (`[preview.images] gdal`), run through
//! `media`'s pipeline. A GPX file needs no GDAL for its facts and map: Coxswain reads it itself.
//! Every GDAL run is held to the driver of the file's kind (`-if`), so a file can never point
//! GDAL at another one or at an address; `GDAL_PAM_ENABLED=NO` keeps it from writing a
//! `.aux.xml` beside your file, and PROJ is kept off the network.

use std::fmt::Write as _;
use std::path::Path;

use crate::media::Run;
use crate::t;

/// The name under `[preview.images]`.
pub const GDAL: &str = "gdal";

/// Vector kinds by their endings, with the GDAL driver that alone may read each.
const VECTOR: &[(&str, &str)] = &[("gpkg", "GPKG"), ("shp", "ESRI Shapefile"), ("geojson", "GeoJSON"), ("kml", "KML"), ("gpx", "GPX")];

/// Layers drawn and listed at most, features drawn per layer, points drawn in all.
const MAX_LAYERS: usize = 8;
const MAX_FEATURES: usize = 50_000;
const MAX_POINTS: usize = 400_000;
/// Each layer's GeoJSON read at most.
const MAX_LAYER: u64 = 64 << 20;
/// The map's width in pixels; its height follows the shapes, at most this much too.
const SIDE: f64 = 1024.0;

fn ending(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase()
}

/// A vector file by its name.
pub fn is_vector(path: &Path) -> bool {
    let e = ending(path);
    VECTOR.iter().any(|(x, _)| *x == e)
}

/// A TIFF that says where on Earth it is: its first page has GeoTIFF's tags (the model's
/// tie points, pixel scale or transformation, or its key directory). Only its header and first
/// directory are read.
pub fn is_geotiff(path: &Path) -> bool {
    if !matches!(ending(path).as_str(), "tif" | "tiff") || crate::cloud::unasked(path) {
        return false;
    }
    let mut head = vec![];
    if let Ok(f) = std::fs::File::open(path) {
        let _ = std::io::Read::read_to_end(&mut std::io::Read::take(f, 64 * 1024), &mut head);
    }
    geotiff_tags(&head)
}

fn geotiff_tags(b: &[u8]) -> bool {
    let le = match b.get(..4) {
        Some([b'I', b'I', 42, 0]) => true,
        Some([b'M', b'M', 0, 42]) => false,
        _ => return false,
    };
    let u16_at = |i: usize| b.get(i..i + 2).map(|x| if le { u16::from_le_bytes([x[0], x[1]]) } else { u16::from_be_bytes([x[0], x[1]]) });
    let u32_at = |i: usize| b.get(i..i + 4).map(|x| if le { u32::from_le_bytes([x[0], x[1], x[2], x[3]]) } else { u32::from_be_bytes([x[0], x[1], x[2], x[3]]) });
    let Some(ifd) = u32_at(4).map(|o| o as usize) else { return false };
    let n = u16_at(ifd).unwrap_or(0) as usize;
    // ModelPixelScale, ModelTiepoint, ModelTransformation, GeoKeyDirectory.
    (0..n).filter_map(|i| u16_at(ifd + 2 + i * 12)).any(|tag| matches!(tag, 33550 | 33922 | 34264 | 34735))
}

/// Geodata Coxswain shows a map and facts of: a vector file, or a GeoTIFF.
pub fn is_geo(path: &Path) -> bool {
    is_vector(path) || is_geotiff(path)
}

/// The GDAL driver that alone may read `path`.
pub(crate) fn driver(path: &Path) -> Option<&'static str> {
    let e = ending(path);
    VECTOR.iter().find(|(x, _)| *x == e).map(|(_, d)| *d).or_else(|| matches!(e.as_str(), "tif" | "tiff").then_some("GTiff"))
}

/// The endings by kind, for the desktop app: `vector` (a map by their shapes) and `raster`
/// (TIFF, a map only when GDAL finds it is a GeoTIFF).
pub fn endings() -> std::collections::BTreeMap<&'static str, Vec<&'static str>> {
    [("vector", VECTOR.iter().map(|(e, _)| *e).collect()), ("raster", vec!["tif", "tiff"])].into_iter().collect()
}

// ---------------------------------------------------------------- what can be made

/// What *Convert geodata* makes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum To {
    Gpkg,
    GeoJson,
    /// Every layer as a shapefile, the files of each zipped into one `.shp.zip`.
    Shapefile,
    Kml,
    Gpx,
    /// A cloud-optimised GeoTIFF (tiled, with overviews).
    Cog,
    /// A plain picture of a GeoTIFF, without its place on Earth.
    Png,
}

impl To {
    pub const ALL: [To; 7] = [To::Gpkg, To::GeoJson, To::Shapefile, To::Kml, To::Gpx, To::Cog, To::Png];

    pub fn id(self) -> &'static str {
        match self {
            To::Gpkg => "gpkg",
            To::GeoJson => "geojson",
            To::Shapefile => "shp",
            To::Kml => "kml",
            To::Gpx => "gpx",
            To::Cog => "cog",
            To::Png => "png",
        }
    }

    pub fn parse(s: &str) -> Option<To> {
        To::ALL.into_iter().find(|t| t.id() == s)
    }

    /// Whether it takes `path`: GeoPackage both kinds, the other vector formats vectors, COG
    /// and PNG GeoTIFFs.
    pub fn takes(self, path: &Path) -> bool {
        match self {
            To::Gpkg => is_geo(path),
            To::Cog | To::Png => is_geotiff(path) || ending(path) == "gpkg",
            _ => is_vector(path),
        }
    }

    /// The ending of what it makes, and the word put before it when that would be the
    /// original's own (`map.cog.tif`).
    pub(crate) fn result_ending(self) -> &'static str {
        match self {
            To::Gpkg => "gpkg",
            To::GeoJson => "geojson",
            To::Shapefile => "shp.zip",
            To::Kml => "kml",
            To::Gpx => "gpx",
            To::Cog => "cog.tif",
            To::Png => "png",
        }
    }

    /// GDAL's driver for it.
    fn driver(self) -> &'static str {
        match self {
            To::Gpkg => "GPKG",
            To::GeoJson => "GeoJSON",
            To::Shapefile => "ESRI Shapefile",
            To::Kml => "KML",
            To::Gpx => "GPX",
            To::Cog => "COG",
            To::Png => "PNG",
        }
    }
}

// ---------------------------------------------------------------- running GDAL

/// The environment of every GDAL run: no `.aux.xml` written beside a file, PROJ offline, no
/// address ever opened.
pub(crate) const ENV: &[(&str, &str)] = &[("GDAL_PAM_ENABLED", "NO"), ("PROJ_NETWORK", "OFF"), ("CPL_VSIL_CURL_ALLOWED_EXTENSIONS", "none"), ("GDAL_HTTP_TIMEOUT", "1")];

/// The oldest GDAL whose programs take every option used here: `ogrinfo -json` came in 3.7.
pub(crate) const GDAL_LEAST: (u32, u32) = (3, 7);

/// The installed GDAL's version (`gdalinfo --version`) when it is older than `GDAL_LEAST`;
/// asked once per program. None when it is new enough, or says no version.
pub(crate) fn too_old(gdalinfo: &Path) -> Option<String> {
    static KNOWN: std::sync::Mutex<Vec<(std::path::PathBuf, Option<String>)>> = std::sync::Mutex::new(Vec::new());
    if let Some((_, v)) = KNOWN.lock().ok()?.iter().find(|(p, _)| p == gdalinfo) {
        return v.clone();
    }
    let mut c = crate::tools::low(gdalinfo);
    c.arg("--version");
    let said = crate::tools::capture(c, std::time::Duration::from_secs(10), 4096).map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    let old = version(&said).filter(|v| (v.0, v.1) < GDAL_LEAST).map(|v| format!("{}.{}.{}", v.0, v.1, v.2));
    KNOWN.lock().ok()?.push((gdalinfo.to_path_buf(), old.clone()));
    old
}

/// The version in `gdalinfo --version`'s line: `GDAL 3.6.2, released 2023/01/02`.
fn version(said: &str) -> Option<(u32, u32, u32)> {
    let v = said.trim().strip_prefix("GDAL ")?.split([',', ' ']).next()?;
    let mut n = v.split('.').map(|x| x.chars().take_while(char::is_ascii_digit).collect::<String>().parse::<u32>());
    Some((n.next()?.ok()?, n.next()?.ok()?, n.next().and_then(Result::ok).unwrap_or(0)))
}

/// What GDAL says of `path`: `ogrinfo`'s summary of a vector file (a GeoPackage of pictures
/// only: `gdalinfo`'s), `gdalinfo`'s of a GeoTIFF.
pub(crate) fn info(run: &mut Run, path: &Path) -> Result<Info, String> {
    let drv = driver(path).ok_or_else(|| t!("geo.not_geodata"))?;
    let input = run.input.clone();
    if drv != "GTiff" {
        let json = run.go("ogrinfo", &["-json", "-ro", "-so", "-al", "-if", drv, &input])?;
        let v = parse_vector(&String::from_utf8_lossy(&json)).ok_or_else(|| t!("geo.unreadable"))?;
        if drv != "GPKG" || v.layers.iter().any(|l| l.geometry) {
            return Ok(Info::Vector(v));
        }
    }
    let json = run.go("gdalinfo", &["-json", "-if", drv, &input])?;
    parse_raster(&String::from_utf8_lossy(&json)).map(Info::Raster).ok_or_else(|| t!("geo.unreadable"))
}

/// The map of `path` as SVG: its shapes, or its picture (a GeoTIFF, a GeoPackage of pictures).
pub(crate) fn map(run: &mut Run, path: &Path) -> Result<String, String> {
    if ending(path) == "gpx" {
        return gpx_map(path);
    }
    match info(run, path)? {
        Info::Vector(v) => {
            let mut layers = vec![];
            for (i, l) in v.layers.iter().filter(|l| l.geometry && plain(l)).take(MAX_LAYERS).enumerate() {
                let file = format!("layer{i}.geojson");
                run.go("ogr2ogr", &layer_args(path, l, &run.out(&file), &run.input.clone()).iter().map(String::as_str).collect::<Vec<_>>())?;
                let json = made(&run.scratch.join(&file))?;
                layers.push(geojson(&json, l.crs.is_some()).ok_or_else(|| t!("geo.unreadable"))?);
            }
            Ok(svg(&layers))
        }
        Info::Raster(r) => {
            let input = run.input.clone();
            let png = run.out("map.png");
            run.go("gdal_translate", &picture_args(&r, driver(path).unwrap_or("GTiff"), &input, &png).iter().map(String::as_str).collect::<Vec<_>>())?;
            let bytes = crate::images::read_result(&run.scratch.join("map.png"), MAX_LAYER, || t!("geo.too_large"))?;
            let (w, h) = image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format().ok().and_then(|r| r.into_dimensions().ok()).ok_or_else(|| t!("geo.unreadable"))?;
            Ok(crate::media::png_in_svg(&bytes, w, h))
        }
    }
}

/// A GPX file read here, at most `MAX_LAYER` of it.
fn read_gpx(path: &Path) -> Result<Gpx, String> {
    Ok(gpx(&read_max(path, MAX_LAYER)?))
}

/// The map of a GPX file, drawn without GDAL: its tracks, routes and waypoints.
pub fn gpx_map(path: &Path) -> Result<String, String> {
    Ok(svg(&read_gpx(path)?.layers()))
}

/// The facts of geodata, for Properties in both apps and under the desktop app's preview: a GPX
/// file's read here; the others' by GDAL, when it runs without a pull (installed, or its image
/// there), within twenty seconds. Empty otherwise, and for a file only in the cloud.
pub fn facts(cfg: &crate::config::PreviewConfig, path: &Path) -> Vec<(String, String)> {
    use crate::media::{How, Program};
    if crate::cloud::unasked(path) || !is_geo(path) {
        return vec![];
    }
    if ending(path) == "gpx" {
        return read_gpx(path).map(|g| g.facts()).unwrap_or_default();
    }
    // Maps *Off* in Preview tools: GDAL does not run by itself.
    if crate::preview_tools::off(cfg, GDAL) {
        return vec![];
    }
    let Ok(how) = crate::media::how(cfg, Program::Gdal) else { return vec![] };
    if let How::Container { path: rt, image, .. } = &how
        && crate::tools::image_size(rt, image).is_none()
    {
        return vec![];
    }
    // Asked once per file as it is: the preview asks each time the file is looked at.
    type Key = (std::path::PathBuf, u64, Option<std::time::SystemTime>);
    static KNOWN: std::sync::Mutex<Vec<(Key, Info)>> = std::sync::Mutex::new(Vec::new());
    let Ok(meta) = std::fs::metadata(path) else { return vec![] };
    let key = (path.to_path_buf(), meta.len(), meta.modified().ok());
    if let Some((_, i)) = KNOWN.lock().unwrap().iter().find(|(k, _)| *k == key) {
        return facts_of(i);
    }
    let Ok(scratch) = crate::searchable::Scratch::named("geo") else { return vec![] };
    let stop = std::sync::atomic::AtomicBool::new(false);
    let Ok(i) = crate::media::in_folder(&how, path, &scratch.0, &stop, Some(std::time::Duration::from_secs(20)), &mut |_| {}, ENV, |run| info(run, path)) else { return vec![] };
    let mut known = KNOWN.lock().unwrap();
    // ponytail: the last 64 files, a scan of a short list.
    if known.len() >= 64 {
        known.remove(0);
    }
    known.push((key, i.clone()));
    facts_of(&i)
}

/// `more` facts after `out`, leaving out a line `out` has already (a GeoTIFF's format and size
/// are its picture's too).
pub fn merge(out: &mut Vec<(String, String)>, more: Vec<(String, String)>) {
    for (k, v) in more {
        if !out.iter().any(|(l, _)| *l == k) {
            out.push((k, v));
        }
    }
}

/// A layer whose name ogr2ogr can be given: one starting with `-` would be read as an option.
// ponytail: such a layer is left out of the map and of a merged GeoJSON; GeoPackage, KML,
// GPX and the shapefile zip take every layer, as they name none.
fn plain(l: &Layer) -> bool {
    !l.name.starts_with('-')
}

/// `ogr2ogr`'s arguments for one layer as GeoJSON in degrees (a layer without a coordinate
/// system as it is), its shapes simplified to about a pixel and bounded in number.
fn layer_args(path: &Path, l: &Layer, out: &str, input: &str) -> Vec<String> {
    let mut a: Vec<String> = ["-f", "GeoJSON", "-if", driver(path).unwrap_or_default(), "-lco", "COORDINATE_PRECISION=7", "-limit"].map(String::from).to_vec();
    a.push(MAX_FEATURES.to_string());
    if l.crs.is_some() {
        a.extend(["-t_srs", "EPSG:4326"].map(String::from));
    }
    if let Some([x0, y0, x1, y1]) = l.extent {
        let tolerance = (x1 - x0).max(y1 - y0) / SIDE / 2.0;
        if tolerance > 0.0 && tolerance.is_finite() {
            a.extend(["-simplify".into(), format!("{tolerance}")]);
        }
    }
    a.extend([out.to_string(), input.to_string(), l.name.clone()]);
    a
}

/// `gdal_translate`'s arguments for a picture of a raster: no wider or taller than `SIDE`, its
/// colours as they are (red, green, blue; a palette spread out), anything else stretched over
/// grey.
fn picture_args(r: &Raster, drv: &str, input: &str, png: &str) -> Vec<String> {
    let mut a: Vec<String> = ["-q", "-if", drv, "-of", "PNG"].map(String::from).to_vec();
    let (w, h) = (r.width as f64, r.height as f64);
    if w > SIDE || h > SIDE {
        let (ow, oh) = if w >= h { (SIDE as u64, 0) } else { (0, SIDE as u64) };
        a.extend(["-outsize".into(), ow.to_string(), oh.to_string()]);
    }
    let byte = r.bands.iter().all(|b| b.kind == "Byte");
    match r.bands.as_slice() {
        [one] if one.palette => a.extend(["-expand", "rgba"].map(String::from)),
        [_, _, _, ..] if byte => {
            let n = if r.bands.get(3).is_some_and(|b| b.colour == "Alpha") { 4 } else { 3 };
            for i in 1..=n {
                a.extend(["-b".into(), i.to_string()]);
            }
        }
        _ => a.extend(["-b", "1", "-ot", "Byte", "-scale"].map(String::from)),
    }
    a.extend([input.to_string(), png.to_string()]);
    a
}

/// `run` converts `path` to `to`, into `name` in the scratch folder.
pub(crate) fn convert(run: &mut Run, path: &Path, to: To, name: &str) -> Result<(), String> {
    let drv = driver(path).ok_or_else(|| t!("geo.not_geodata"))?;
    let (input, out) = (run.input.clone(), run.out(name));
    let info = info(run, path)?;
    match (to, matches!(info, Info::Raster(_))) {
        (To::Cog | To::Png, false) => Err(t!("geo.needs_raster")),
        (To::Gpkg | To::Cog | To::Png, true) => {
            let mut a = vec!["-q", "-if", drv, "-of", to.driver()];
            if to == To::Cog {
                a.extend(["-co", "COMPRESS=DEFLATE"]);
            }
            a.extend([input.as_str(), out.as_str()]);
            run.go("gdal_translate", &a).map(drop)
        }
        (_, true) => Err(t!("geo.needs_vector")),
        (To::GeoJson, false) => {
            // GeoJSON holds one layer: each is made on its own, and their features are put
            // together, each with its layer's name when there are several.
            let Info::Vector(v) = info else { return Err(t!("geo.needs_vector")) };
            let filled: Vec<&Layer> = v.layers.iter().filter(|l| l.features > 0).collect();
            if filled.len() <= 1 {
                // One layer: GDAL writes it straight, however large; named, as the file may
                // have others without features (a GPX file has five, a GeoPackage empty tables).
                let one = filled.first().copied().or(v.layers.first()).map(|l| l.name.as_str());
                let a = ["-f", "GeoJSON", "-if", drv, "-lco", "RFC7946=YES", out.as_str(), input.as_str()];
                return run.go("ogr2ogr", &a.into_iter().chain(one).collect::<Vec<_>>()).map(drop);
            }
            let mut all = vec![];
            for (i, l) in filled.into_iter().filter(|l| plain(l)).enumerate() {
                let file = format!("part{i}.geojson");
                let mut a = vec!["-f", "GeoJSON", "-if", drv, "-lco", "RFC7946=YES"];
                let at = run.out(&file);
                a.extend([at.as_str(), input.as_str(), l.name.as_str()]);
                run.go("ogr2ogr", &a)?;
                let v: serde_json::Value = serde_json::from_str(&made(&run.scratch.join(&file))?).map_err(|e| e.to_string())?;
                for mut f in v["features"].as_array().cloned().unwrap_or_default() {
                    f["properties"]["layer"] = l.name.clone().into();
                    all.push(f);
                }
            }
            let fc = serde_json::json!({ "type": "FeatureCollection", "features": all });
            std::io::Write::write_all(&mut crate::images::fresh_result(&run.scratch.join(name))?, fc.to_string().as_bytes()).map_err(|e| e.to_string())
        }
        (_, false) => {
            let mut a = vec!["-f", to.driver(), "-if", drv];
            // A file's one layer is named after the file, which GeoPackage refuses when it starts
            // with a sign (`-roads`): letters, digits and `_` only, then.
            let Info::Vector(v) = &info else { unreachable!() };
            let plain = match v.layers.as_slice() {
                [one] if !one.name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ' ') => {
                    let n: String = one.name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
                    Some(n.trim_matches('_').to_string()).filter(|n| !n.is_empty()).or(Some("layer".into()))
                }
                _ => None,
            };
            if let Some(n) = &plain {
                a.extend(["-nln", n.as_str()]);
            }
            // KML and GPX are in degrees; GPX keeps a feature's other fields as its extensions.
            if matches!(to, To::Kml | To::Gpx) {
                a.extend(["-t_srs", "EPSG:4326"]);
            }
            if to == To::Gpx {
                a.extend(["-dsco", "GPX_USE_EXTENSIONS=YES"]);
            }
            a.extend([out.as_str(), input.as_str()]);
            run.go("ogr2ogr", &a).map(drop)
        }
    }
}

/// A layer GDAL wrote into the scratch folder, at most `MAX_LAYER` (`images::read_result`).
fn made(path: &Path) -> Result<String, String> {
    crate::images::read_result(path, MAX_LAYER, || t!("geo.too_large")).map(|b| String::from_utf8_lossy(&b).into_owned())
}

fn read_max(path: &Path, max: u64) -> Result<String, String> {
    let f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    if f.metadata().map_err(|e| e.to_string())?.len() > max {
        return Err(t!("geo.too_large"));
    }
    let mut s = String::new();
    std::io::Read::read_to_string(&mut std::io::Read::take(f, max), &mut s).map_err(|e| e.to_string())?;
    Ok(s)
}

// ---------------------------------------------------------------- what GDAL says

/// What GDAL said of a file.
#[derive(Clone, Debug, PartialEq)]
pub enum Info {
    Vector(Vector),
    Raster(Raster),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Vector {
    pub format: String,
    pub layers: Vec<Layer>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layer {
    pub name: String,
    /// "Point", "LineString", "Polygon" …; empty for a table without shapes.
    pub kind: String,
    pub geometry: bool,
    pub features: u64,
    /// Its coordinate system's name and EPSG code.
    pub crs: Option<(String, Option<u32>)>,
    /// West, south, east, north, in its coordinate system.
    pub extent: Option<[f64; 4]>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Raster {
    pub format: String,
    pub width: u64,
    pub height: u64,
    pub bands: Vec<Band>,
    pub crs: Option<(String, Option<u32>)>,
    /// West, south, east, north, in degrees.
    pub extent: Option<[f64; 4]>,
    /// A pixel's width and height in the coordinate system's units.
    pub pixel: Option<(f64, f64)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Band {
    pub kind: String,
    pub colour: String,
    pub palette: bool,
}

/// A coordinate system's name and EPSG code from its WKT: the first quoted name, and the last
/// `ID["EPSG",…]`, the system's own (those before it are its parts').
fn crs_of(wkt: &str) -> Option<(String, Option<u32>)> {
    let name = wkt.split_once("[\"")?.1.split('"').next()?.to_string();
    let epsg = wkt.rfind("ID[\"EPSG\",").and_then(|i| wkt[i + 10..].split(']').next()?.trim().parse().ok());
    (!name.is_empty()).then_some((name, epsg))
}

fn parse_vector(json: &str) -> Option<Vector> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let layers = v["layers"]
        .as_array()?
        .iter()
        .map(|l| {
            let g = &l["geometryFields"][0];
            let extent = g["extent"].as_array().and_then(|e| {
                let n: Vec<f64> = e.iter().filter_map(|x| x.as_f64()).collect();
                <[f64; 4]>::try_from(n).ok()
            });
            Layer {
                name: l["name"].as_str().unwrap_or_default().into(),
                kind: g["type"].as_str().unwrap_or_default().into(),
                geometry: g.is_object(),
                features: l["featureCount"].as_u64().unwrap_or(0),
                crs: g["coordinateSystem"]["wkt"].as_str().and_then(crs_of),
                extent,
            }
        })
        .collect();
    Some(Vector { format: v["driverLongName"].as_str().unwrap_or_default().into(), layers })
}

fn parse_raster(json: &str) -> Option<Raster> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let size = v["size"].as_array()?;
    let ring = v["wgs84Extent"]["coordinates"][0].as_array();
    let extent = ring.and_then(|r| {
        let pts: Vec<(f64, f64)> = r.iter().filter_map(|p| Some((p[0].as_f64()?, p[1].as_f64()?))).collect();
        let f = |g: fn(f64, f64) -> f64, s: f64, k: fn(&(f64, f64)) -> f64| pts.iter().map(k).fold(s, g);
        (!pts.is_empty()).then(|| [f(f64::min, f64::MAX, |p| p.0), f(f64::min, f64::MAX, |p| p.1), f(f64::max, f64::MIN, |p| p.0), f(f64::max, f64::MIN, |p| p.1)])
    });
    let gt = v["geoTransform"].as_array();
    Some(Raster {
        format: v["driverLongName"].as_str().unwrap_or_default().into(),
        width: size.first()?.as_u64()?,
        height: size.get(1)?.as_u64()?,
        bands: v["bands"]
            .as_array()?
            .iter()
            .map(|b| Band { kind: b["type"].as_str().unwrap_or_default().into(), colour: b["colorInterpretation"].as_str().unwrap_or_default().into(), palette: b["colorTable"].is_object() })
            .collect(),
        crs: v["coordinateSystem"]["wkt"].as_str().and_then(crs_of),
        extent,
        pixel: gt.and_then(|g| Some((g.get(1)?.as_f64()?.abs(), g.get(5)?.as_f64()?.abs()))),
    })
}

/// A number with no more decimals than it needs, at most `places`.
fn num(x: f64, places: usize) -> String {
    let s = format!("{x:.places$}");
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
    crate::i18n::digits(s)
}

fn crs_line(crs: &(String, Option<u32>)) -> String {
    match crs.1 {
        Some(code) => format!("{} (EPSG:{code})", crs.0),
        None => crs.0.clone(),
    }
}

fn extent_line(e: &[f64; 4], degrees: bool) -> String {
    let p = if degrees { 5 } else { 2 };
    format!("{}, {} – {}, {}", num(e[0], p), num(e[1], p), num(e[2], p), num(e[3], p))
}

/// GDAL's word for a kind of shape, said in the language.
fn shape_kind(kind: &str) -> String {
    let k = kind.trim_start_matches("Multi").trim_start_matches("3D ").trim_end_matches(" Z").trim_end_matches(" M").trim_end_matches(" ZM");
    let key = match k.trim_start_matches("Multi") {
        "Point" => "geo.points",
        "LineString" | "CircularString" | "CompoundCurve" | "MultiCurve" => "geo.lines",
        "Polygon" | "CurvePolygon" | "MultiSurface" => "geo.areas",
        "" | "None" => return String::new(),
        _ => "geo.mixed",
    };
    t!(key)
}

/// What GDAL said as facts: the format, each layer with its kind of shapes and number of
/// features, the coordinate system, the extent; a raster's size, bands and pixel size.
pub fn facts_of(info: &Info) -> Vec<(String, String)> {
    let mut out = vec![];
    match info {
        Info::Vector(v) => {
            if !v.format.is_empty() {
                out.push((t!("facts.format"), v.format.clone()));
            }
            let shown: Vec<String> = v
                .layers
                .iter()
                .take(MAX_LAYERS)
                .map(|l| {
                    let kind = shape_kind(&l.kind);
                    let n = crate::tn!("geo.features", l.features, "n" => crate::i18n::digits(l.features.to_string()));
                    if kind.is_empty() { format!("{} ({n})", l.name) } else { format!("{} ({kind}, {n})", l.name) }
                })
                .collect();
            if !shown.is_empty() {
                let more = if v.layers.len() > MAX_LAYERS { format!("; {}", t!("geo.more_layers", "n" => crate::i18n::digits((v.layers.len() - MAX_LAYERS).to_string()))) } else { String::new() };
                out.push((t!("facts.layers"), format!("{}{more}", shown.join("; "))));
            }
            if let Some(l) = v.layers.iter().find(|l| l.crs.is_some()) {
                out.push((t!("geo.crs"), crs_line(l.crs.as_ref().unwrap())));
                if let Some(e) = &l.extent {
                    let degrees = l.crs.as_ref().is_some_and(|c| c.1 == Some(4326));
                    out.push((t!("geo.extent"), extent_line(e, degrees)));
                }
            }
        }
        Info::Raster(r) => {
            if !r.format.is_empty() {
                out.push((t!("facts.format"), r.format.clone()));
            }
            out.push((t!("facts.dimensions"), t!("facts.dimensions_value", "width" => crate::i18n::digits(r.width.to_string()), "height" => crate::i18n::digits(r.height.to_string()))));
            if let Some(b) = r.bands.first() {
                out.push((t!("geo.bands"), format!("{} ({})", crate::i18n::digits(r.bands.len().to_string()), b.kind)));
            }
            if let Some(c) = &r.crs {
                out.push((t!("geo.crs"), crs_line(c)));
            }
            if let Some((w, h)) = r.pixel {
                out.push((t!("geo.pixel"), format!("{} × {}", num(w, 6), num(h, 6))));
            }
            if let Some(e) = &r.extent {
                out.push((t!("geo.extent_degrees"), extent_line(e, true)));
            }
        }
    }
    out
}

// ---------------------------------------------------------------- GPX, read here

/// A track's point: longitude, latitude, and its time (seconds since 1970) when it has one.
pub type Point = (f64, f64, Option<i64>);

/// What a GPX file holds, read without GDAL.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Gpx {
    pub waypoints: Vec<(f64, f64)>,
    /// Each route's points; each track's segments' points with their times (seconds since
    /// 1970, when the point has one).
    pub routes: Vec<Vec<(f64, f64)>>,
    pub tracks: Vec<Vec<Vec<Point>>>,
}

/// A GPX file read: its waypoints, routes and tracks. What is no GPX reads as nothing; the
/// points read are bounded.
pub fn gpx(xml: &str) -> Gpx {
    use quick_xml::events::Event;
    let mut r = quick_xml::Reader::from_str(xml);
    r.config_mut().check_end_names = false;
    let mut g = Gpx::default();
    let (mut n, mut in_time, mut last_point) = (0usize, false, None::<&'static str>);
    let attr = |e: &quick_xml::events::BytesStart, name: &str| -> Option<f64> {
        let a = e.attributes().with_checks(false).flatten().find(|a| a.key.local_name().as_ref() == name)?;
        a.value.trim().parse().ok().filter(|x: &f64| x.is_finite())
    };
    while n < MAX_POINTS {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if matches!(e.local_name().as_ref(), "wpt" | "rtept" | "trkpt") => {
                let point = attr(&e, "lat").zip(attr(&e, "lon"));
                let Some((lat, lon)) = point else { continue };
                n += 1;
                match e.local_name().as_ref() {
                    "wpt" => (g.waypoints.push((lon, lat)), last_point = None).1,
                    "rtept" => {
                        if let Some(r) = g.routes.last_mut() {
                            r.push((lon, lat));
                        }
                        last_point = None;
                    }
                    _ => {
                        if let Some(s) = g.tracks.last_mut().and_then(|t| t.last_mut()) {
                            s.push((lon, lat, None));
                            last_point = Some("trkpt");
                        }
                    }
                }
            }
            Ok(Event::Start(e)) => match e.local_name().as_ref() {
                "rte" => g.routes.push(vec![]),
                "trk" => g.tracks.push(vec![]),
                "trkseg" => {
                    if g.tracks.is_empty() {
                        g.tracks.push(vec![]);
                    }
                    g.tracks.last_mut().unwrap().push(vec![]);
                }
                "time" => in_time = last_point.is_some(),
                _ => {}
            },
            Ok(Event::End(e)) => match e.local_name().as_ref() {
                "time" => in_time = false,
                "trkpt" => last_point = None,
                _ => {}
            },
            Ok(Event::Text(t)) if in_time => {
                let s = t.into_inner().trim().to_string();
                if let (Ok(when), Some(p)) = (chrono::DateTime::parse_from_rfc3339(&s), g.tracks.last_mut().and_then(|t| t.last_mut()).and_then(|s| s.last_mut())) {
                    p.2 = Some(when.timestamp());
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    g
}

/// Metres between two points in degrees, on a sphere the Earth's size.
fn metres(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (la1, la2) = (a.1.to_radians(), b.1.to_radians());
    let (dla, dlo) = (la2 - la1, (b.0 - a.0).to_radians());
    let h = (dla / 2.0).sin().powi(2) + la1.cos() * la2.cos() * (dlo / 2.0).sin().powi(2);
    2.0 * 6_371_008.8 * h.sqrt().min(1.0).asin()
}

fn along(points: impl Iterator<Item = (f64, f64)>) -> f64 {
    let mut prev = None;
    points.fold(0.0, |sum, p| {
        let d = prev.map_or(0.0, |q| metres(q, p));
        prev = Some(p);
        sum + d
    })
}

/// "6.3 km" or "850 m".
fn distance(m: f64) -> String {
    if m >= 1000.0 { format!("{} {}", num(m / 1000.0, if m >= 100_000.0 { 0 } else { 1 }), t!("unit.km")) } else { format!("{} {}", num(m, 0), t!("unit.m")) }
}

/// "1:02:03" or "4:05".
fn hms(secs: i64) -> String {
    let s = secs.max(0);
    crate::i18n::digits(if s >= 3600 { format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60) } else { format!("{}:{:02}", s / 60, s % 60) })
}

impl Gpx {
    /// Its facts: how many tracks, their length, when they began and how long they took; the
    /// routes and their length; the waypoints.
    pub fn facts(&self) -> Vec<(String, String)> {
        let mut out = vec![(t!("facts.format"), "GPX".to_string())];
        let segs = || self.tracks.iter().flatten();
        if !self.tracks.is_empty() {
            out.push((t!("geo.tracks"), crate::i18n::digits(self.tracks.len().to_string())));
            out.push((t!("geo.track_length"), distance(segs().map(|s| along(s.iter().map(|p| (p.0, p.1)))).sum())));
            let times: Vec<i64> = segs().flatten().filter_map(|p| p.2).collect();
            if let (Some(first), Some(last)) = (times.iter().min(), times.iter().max()) {
                out.push((t!("geo.started"), crate::i18n::date_time(*first as u64)));
                out.push((t!("geo.took"), hms(last - first)));
            }
        }
        if !self.routes.is_empty() {
            out.push((t!("geo.routes"), crate::i18n::digits(self.routes.len().to_string())));
            out.push((t!("geo.route_length"), distance(self.routes.iter().map(|r| along(r.iter().copied())).sum())));
        }
        if !self.waypoints.is_empty() {
            out.push((t!("geo.waypoints"), crate::i18n::digits(self.waypoints.len().to_string())));
        }
        out
    }

    /// As layers for the map: tracks, routes, waypoints.
    fn layers(&self) -> Vec<Shapes> {
        let tracks = self.tracks.iter().flatten().map(|s| Shape::Line(s.iter().map(|p| (p.0, p.1)).collect())).collect();
        let routes = self.routes.iter().map(|r| Shape::Line(r.clone())).collect();
        let points = self.waypoints.iter().map(|p| Shape::Point(*p)).collect();
        vec![Shapes { shapes: tracks, degrees: true }, Shapes { shapes: routes, degrees: true }, Shapes { shapes: points, degrees: true }]
    }
}

// ---------------------------------------------------------------- the map

#[derive(Clone, Debug, PartialEq)]
enum Shape {
    Point((f64, f64)),
    Line(Vec<(f64, f64)>),
    /// Its outer ring and its holes.
    Area(Vec<Vec<(f64, f64)>>),
}

/// One layer's shapes; `degrees` when they are longitude and latitude.
#[derive(Clone, Debug, PartialEq)]
struct Shapes {
    shapes: Vec<Shape>,
    degrees: bool,
}

/// A layer's GeoJSON as shapes; `degrees` when it was made in them (`layer_args`: a layer with
/// a coordinate system is moved into WGS 84, one without stays as it is).
fn geojson(json: &str, degrees: bool) -> Option<Shapes> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let mut shapes = vec![];
    for f in v["features"].as_array()?.iter().take(MAX_FEATURES) {
        geometry(&f["geometry"], &mut shapes);
    }
    Some(Shapes { shapes, degrees })
}

fn geometry(g: &serde_json::Value, out: &mut Vec<Shape>) {
    let pt = |p: &serde_json::Value| Some((p.get(0)?.as_f64()?, p.get(1)?.as_f64()?));
    let line = |l: &serde_json::Value| l.as_array().map(|a| a.iter().filter_map(pt).collect::<Vec<_>>()).unwrap_or_default();
    let area = |a: &serde_json::Value| a.as_array().map(|r| r.iter().map(line).collect::<Vec<_>>()).unwrap_or_default();
    let c = &g["coordinates"];
    let each = || c.as_array().into_iter().flatten();
    match g["type"].as_str().unwrap_or_default() {
        "Point" => out.extend(pt(c).map(Shape::Point)),
        "MultiPoint" => out.extend(each().filter_map(pt).map(Shape::Point)),
        "LineString" => out.push(Shape::Line(line(c))),
        "MultiLineString" => out.extend(each().map(|l| Shape::Line(line(l)))),
        "Polygon" => out.push(Shape::Area(area(c))),
        "MultiPolygon" => out.extend(each().map(|a| Shape::Area(area(a)))),
        "GeometryCollection" => {
            for g in g["geometries"].as_array().into_iter().flatten() {
                geometry(g, out);
            }
        }
        _ => {}
    }
}

/// Colours of the layers, in turn: they read on light and dark panes.
const COLOURS: &[&str] = &["#2f7fd0", "#e0732c", "#3aa55d", "#a35bd1", "#d0475a", "#1fa3a3", "#b8962e", "#6d7a8c"];

/// The layers drawn as SVG: areas filled lightly, lines, points as dots, north up. Longitude
/// is drawn narrower away from the equator (by the cosine of the middle latitude), so a map of
/// Denmark is not stretched wide. Nothing of the file's text goes into it.
fn svg(layers: &[Shapes]) -> String {
    let degrees = layers.iter().all(|l| l.degrees);
    let mut n = 0usize;
    let mut all = |s: &Shape| -> Vec<(f64, f64)> {
        let pts: Vec<(f64, f64)> = match s {
            Shape::Point(p) => vec![*p],
            Shape::Line(l) => l.clone(),
            Shape::Area(rings) => rings.iter().flatten().copied().collect(),
        };
        n += pts.len();
        if n > MAX_POINTS { vec![] } else { pts }
    };
    let pts: Vec<(f64, f64)> = layers.iter().flat_map(|l| &l.shapes).flat_map(&mut all).collect();
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for (x, y) in &pts {
        (x0, y0, x1, y1) = (x0.min(*x), y0.min(*y), x1.max(*x), y1.max(*y));
    }
    if pts.is_empty() {
        (x0, y0, x1, y1) = (0.0, 0.0, 1.0, 1.0);
    }
    let kx = if degrees { ((y0 + y1) / 2.0).to_radians().cos().max(0.01) } else { 1.0 };
    // One point, or all on one line: drawn in the middle.
    let (w, h) = (((x1 - x0) * kx).max(1e-9), (y1 - y0).max(1e-9));
    let scale = (SIDE - 32.0) / w.max(h);
    let (pw, ph) = ((w * scale + 32.0).round(), (h * scale + 32.0).round());
    let at = |(x, y): (f64, f64)| ((x - x0) * kx * scale + 16.0, (y1 - y) * scale + 16.0);
    let mut s = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{pw}\" height=\"{ph}\" viewBox=\"0 0 {pw} {ph}\">");
    let path = |pts: &[(f64, f64)], close: bool, d: &mut String| {
        for (i, p) in pts.iter().enumerate() {
            let (x, y) = at(*p);
            let _ = write!(d, "{}{x:.1} {y:.1}", if i == 0 { 'M' } else { 'L' });
        }
        if close {
            d.push('Z');
        }
    };
    let mut left = MAX_POINTS;
    for (i, l) in layers.iter().enumerate() {
        // Each layer's areas, lines and points in colours of their own: a road shows on its area.
        let c = |k: usize| COLOURS[(i * 3 + k) % COLOURS.len()];
        let (mut areas, mut lines, mut dots) = (String::new(), String::new(), String::new());
        for shape in &l.shapes {
            match shape {
                Shape::Area(rings) => {
                    for r in rings {
                        if r.len() > left {
                            break;
                        }
                        left -= r.len();
                        path(r, true, &mut areas);
                    }
                }
                Shape::Line(p) if p.len() <= left => {
                    left -= p.len();
                    path(p, false, &mut lines);
                }
                Shape::Point(p) if left > 0 => {
                    left -= 1;
                    let (x, y) = at(*p);
                    let _ = write!(dots, "<circle cx=\"{x:.1}\" cy=\"{y:.1}\" r=\"12\"/>");
                }
                _ => {}
            }
        }
        if !areas.is_empty() {
            let _ = write!(s, "<path d=\"{areas}\" fill=\"{0}\" fill-opacity=\"0.3\" fill-rule=\"evenodd\" stroke=\"{0}\" stroke-width=\"3\"/>", c(0));
        }
        if !lines.is_empty() {
            let _ = write!(s, "<path d=\"{lines}\" fill=\"none\" stroke=\"{}\" stroke-width=\"6\" stroke-linejoin=\"round\" stroke-linecap=\"round\"/>", c(1));
        }
        if !dots.is_empty() {
            let _ = write!(s, "<g fill=\"{}\" stroke=\"#ffffff\" stroke-width=\"3\">{dots}</g>", c(2));
        }
    }
    s.push_str("</svg>");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const GPX: &str = r#"<?xml version="1.0"?><gpx version="1.1" xmlns="http://www.topografix.com/GPX/1/1">
        <wpt lat="55.676" lon="12.568"><name>Start</name></wpt>
        <rte><name>Plan</name><rtept lat="55.676" lon="12.568"/><rtept lat="55.680" lon="12.575"/></rte>
        <trk><name>Morning</name><trkseg>
        <trkpt lat="55.676" lon="12.568"><time>2026-10-01T07:00:00Z</time></trkpt>
        <trkpt lat="55.678" lon="12.571"><ele>6</ele><time>2026-10-01T07:04:10Z</time></trkpt>
        <trkpt lat="55.680" lon="12.575"><time>2026-10-01T08:09:30+00:00</time></trkpt>
        </trkseg></trk></gpx>"#;

    #[test]
    fn geo_gpx_tracks_have_their_length_and_time() {
        crate::i18n::set_language("en-GB");
        let g = gpx(GPX);
        assert_eq!((g.waypoints.len(), g.routes.len(), g.tracks.len()), (1, 1, 1));
        assert_eq!(g.tracks[0][0].len(), 3);
        assert_eq!(g.tracks[0][0][2].2, Some(1_790_842_170));
        let f = g.facts();
        let get = |k: &str| f.iter().find(|(l, _)| *l == t!(k)).map(|(_, v)| v.clone());
        assert_eq!(get("geo.took").as_deref(), Some("1:09:30"));
        // On a sphere: GDAL's length on the ellipsoid is 627.9 m, 0.3 % more.
        assert_eq!(get("geo.track_length").as_deref(), Some("626 m"));
        assert_eq!(get("geo.routes").as_deref(), Some("1"));
        assert!(gpx("not xml <<<").tracks.is_empty());
        assert_eq!(distance(6340.0), "6.3 km");
        assert_eq!(distance(123_456.0), "123 km");
    }

    #[test]
    fn geo_tiff_tags_tell_a_geotiff() {
        // Little-endian, one directory at 8 with one entry: GeoKeyDirectory.
        let mut b = b"II\x2a\x00\x08\x00\x00\x00\x01\x00".to_vec();
        b.extend(34735u16.to_le_bytes());
        b.extend([0u8; 10]);
        assert!(geotiff_tags(&b));
        b[10..12].copy_from_slice(&256u16.to_le_bytes());
        assert!(!geotiff_tags(&b), "a plain TIFF");
        assert!(!geotiff_tags(b"MM\x00\x2a\x00\x00\x00\xff"), "a directory past the end");
        assert!(!geotiff_tags(b"GIF89a"));
        assert!(is_vector(Path::new("a.GPKG")) && !is_vector(Path::new("a.json")));
        assert_eq!(driver(Path::new("x.shp")), Some("ESRI Shapefile"));
    }

    #[test]
    fn geo_facts_from_gdal() {
        crate::i18n::set_language("en-GB");
        let wkt = "PROJCRS[\"ETRS89 / UTM zone 32N\",BASEGEOGCRS[\"ETRS89\",ID[\"EPSG\",4258]],ID[\"EPSG\",25832]]";
        assert_eq!(crs_of(wkt), Some(("ETRS89 / UTM zone 32N".into(), Some(25832))));
        let vector = format!(
            r#"{{"driverLongName":"GeoPackage","layers":[{{"name":"roads","featureCount":3,"geometryFields":[{{"type":"LineString","extent":[720000,6178000,724000,6180000],"coordinateSystem":{{"wkt":{}}}}}]}},{{"name":"notes","featureCount":1,"geometryFields":[]}}]}}"#,
            serde_json::to_string(wkt).unwrap()
        );
        let v = parse_vector(&vector).unwrap();
        assert!(v.layers[0].geometry && !v.layers[1].geometry);
        let f = facts_of(&Info::Vector(v));
        let get = |k: &str| f.iter().find(|(l, _)| *l == t!(k)).map(|(_, v)| v.clone());
        assert_eq!(get("facts.layers").as_deref(), Some("roads (lines, 3 features); notes (1 feature)"));
        assert_eq!(get("geo.crs").as_deref(), Some("ETRS89 / UTM zone 32N (EPSG:25832)"));
        assert_eq!(get("geo.extent").as_deref(), Some("720000, 6178000 – 724000, 6180000"));
        let raster = r#"{"driverLongName":"GeoTIFF","size":[200,100],"geoTransform":[720000,20,0,6180000,0,-20],
            "coordinateSystem":{"wkt":"GEOGCRS[\"WGS 84\",ID[\"EPSG\",4326]]"},
            "wgs84Extent":{"type":"Polygon","coordinates":[[[12.50,55.71],[12.50,55.69],[12.56,55.69],[12.56,55.71],[12.50,55.71]]]},
            "bands":[{"type":"Byte","colorInterpretation":"Red"},{"type":"Byte","colorInterpretation":"Green"},{"type":"Byte","colorInterpretation":"Blue"}]}"#;
        let r = parse_raster(raster).unwrap();
        assert_eq!(r.extent, Some([12.50, 55.69, 12.56, 55.71]));
        let a = picture_args(&r, "GTiff", "/src/a.tif", "/out/map.png");
        assert!(a.windows(2).any(|w| w == ["-b", "3"]) && !a.contains(&"-outsize".to_string()) && a.windows(2).any(|w| w == ["-if", "GTiff"]));
        let f = facts_of(&Info::Raster(r));
        assert!(f.iter().any(|(_, v)| v == "3 (Byte)"), "{f:?}");
        let grey = Raster { width: 4000, height: 1000, bands: vec![Band { kind: "Float32".into(), ..Default::default() }], ..Default::default() };
        let a = picture_args(&grey, "GTiff", "i", "o");
        assert!(a.windows(3).any(|w| w == ["-outsize", "1024", "0"]) && a.contains(&"-scale".to_string()));
    }

    /// `steps` on a GDAL that is a stand-in: `ogrinfo` says `layers`; `ogr2ogr` hands its
    /// arguments to `made`, which writes what it likes into the scratch folder.
    fn fake<R>(name: &str, layers: &str, mut made: impl FnMut(&Path, &[String]), steps: impl FnOnce(&mut Run) -> Result<R, String>) -> (Result<R, String>, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("coxswain-geo-fake-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let info = format!(r#"{{"driverLongName":"x","layers":[{layers}]}}"#);
        let scratch = dir.clone();
        let mut exec = |program: &str, args: &[String]| -> Result<Vec<u8>, String> {
            match program {
                "ogrinfo" => Ok(info.clone().into_bytes()),
                _ => {
                    made(&scratch, args);
                    Ok(vec![])
                }
            }
        };
        let mut run = Run { exec: &mut exec, input: "/src/in".into(), out: dir.display().to_string(), scratch: &dir };
        (steps(&mut run), dir)
    }

    const WGS84: &str = r#"{"wkt":"GEOGCRS[\"WGS 84\",ID[\"EPSG\",4326]]"}"#;
    /// What ogr2ogr writes when it moves a layer into WGS 84 without RFC 7946: a `crs` member.
    const CRS84: &str = r#"{"type":"FeatureCollection","crs":{"type":"name","properties":{"name":"urn:ogc:def:crs:OGC:1.3:CRS84"}},"features":[
        {"type":"Feature","geometry":{"type":"LineString","coordinates":[[12.0,55.0],[13.0,56.0]]}}]}"#;

    fn write_to_geojson_arg(dir: &Path, args: &[String], body: &str) {
        let out = args.iter().find(|a| a.ends_with(".geojson")).unwrap();
        assert!(Path::new(out).starts_with(dir));
        std::fs::write(out, body).unwrap();
    }

    fn width_and_height(svg: &str) -> (f64, f64) {
        let get = |k: &str| svg.split(k).nth(1).unwrap().split('"').next().unwrap().parse().unwrap();
        (get("width=\""), get("height=\""))
    }

    #[test]
    fn geo_a_reprojected_layer_is_drawn_in_degrees_a_layer_without_a_crs_as_it_is() {
        let with = format!(r#"{{"name":"roads","featureCount":1,"geometryFields":[{{"type":"LineString","coordinateSystem":{WGS84}}}]}}"#);
        let (svg, d) = fake("crs", &with, |dir, a| write_to_geojson_arg(dir, a, CRS84), |run| map(run, Path::new("a.gpkg")));
        let (w, h) = width_and_height(&svg.unwrap());
        assert!(h == SIDE && w < 0.65 * SIDE, "a layer moved into WGS 84 is narrower at 55° N: {w} × {h}");
        std::fs::remove_dir_all(d).unwrap();
        let without = r#"{"name":"plan","featureCount":1,"geometryFields":[{"type":"LineString"}]}"#;
        let (svg, d) = fake("nocrs", without, |dir, a| write_to_geojson_arg(dir, a, &CRS84.replace(r#""crs":{"type":"name","properties":{"name":"urn:ogc:def:crs:OGC:1.3:CRS84"}},"#, "")), |run| map(run, Path::new("a.gpkg")));
        let (w, h) = width_and_height(&svg.unwrap());
        assert_eq!(w, h, "a layer without a coordinate system is drawn as it is");
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn geo_a_gpx_file_of_waypoints_alone_converts_to_geojson_by_its_layer() {
        let layers = ["waypoints", "routes", "tracks", "route_points", "track_points"]
            .iter()
            .map(|n| format!(r#"{{"name":"{n}","featureCount":{},"geometryFields":[{{"type":"Point","coordinateSystem":{WGS84}}}]}}"#, u8::from(*n == "waypoints")))
            .collect::<Vec<_>>()
            .join(",");
        let mut seen = vec![];
        let (r, d) = fake("gpx", &layers, |_, a| seen.push(a.to_vec()), |run| convert(run, Path::new("walk.gpx"), To::GeoJson, "out.geojson"));
        r.unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].last().map(String::as_str), Some("waypoints"), "{:?}", seen[0]);
        std::fs::remove_dir_all(d).unwrap();
    }

    /// A GDAL that was made to write a link to a secret (or a FIFO) where its layer goes: the
    /// map is refused, nothing of the secret is read, and a FIFO does not hang it.
    #[cfg(unix)]
    #[test]
    fn geo_results_that_are_links_or_fifos_are_refused() {
        let secret = std::env::temp_dir().join(format!("coxswain-geo-secret-{}", std::process::id()));
        std::fs::write(&secret, CRS84).unwrap();
        let layer = format!(r#"{{"name":"roads","featureCount":1,"geometryFields":[{{"type":"LineString","coordinateSystem":{WGS84}}}]}}"#);
        let link = |_: &Path, a: &[String]| std::os::unix::fs::symlink(&secret, a.iter().find(|a| a.ends_with(".geojson")).unwrap()).unwrap();
        let (r, d) = fake("link", &layer, link, |run| map(run, Path::new("a.gpkg")));
        assert!(r.is_err(), "a link is never followed");
        std::fs::remove_dir_all(d).unwrap();
        let fifo = |_: &Path, a: &[String]| {
            let p = std::ffi::CString::new(a.iter().find(|a| a.ends_with(".geojson")).unwrap().as_str()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(p.as_ptr(), 0o600) }, 0);
        };
        let (r, d) = fake("fifo", &layer, fifo, |run| map(run, Path::new("a.gpkg")));
        assert!(r.is_err(), "a FIFO is refused, not waited on");
        std::fs::remove_dir_all(d).unwrap();
        std::fs::remove_file(secret).unwrap();
    }

    #[test]
    fn geo_gdal_versions_are_read_from_gdalinfo() {
        assert_eq!(version("GDAL 3.6.2, released 2023/01/02\n"), Some((3, 6, 2)));
        assert_eq!(version("GDAL 3.10.0dev-abc, released 2024/11/01"), Some((3, 10, 0)));
        assert_eq!(version("GDAL 3.9.1 \"Ilheus\", released 2024/06/22"), Some((3, 9, 1)));
        assert_eq!(version("not GDAL"), None);
        assert!((3, 6) < GDAL_LEAST && (3, 7) >= GDAL_LEAST && (3, 10) > GDAL_LEAST);
    }

    #[test]
    fn geo_names_are_never_options_and_layers_say_their_driver() {
        let l = Layer { name: "-roads".into(), crs: Some(("WGS 84".into(), Some(4326))), extent: Some([0.0, 0.0, 2048.0, 1.0]), ..Default::default() };
        let a = layer_args(Path::new("/x/-a.gpkg"), &l, "/out/layer0.geojson", "/src/-a.gpkg");
        assert_eq!(&a[a.len() - 3..], ["/out/layer0.geojson", "/src/-a.gpkg", "-roads"], "a layer's name comes after the source, where ogr2ogr reads names only");
        assert!(a.windows(2).any(|w| w == ["-if", "GPKG"]) && a.windows(2).any(|w| w == ["-simplify", "1"]) && a.windows(2).any(|w| w == ["-t_srs", "EPSG:4326"]));
        assert_eq!(To::ALL.map(|t| To::parse(t.id())), To::ALL.map(Some));
        assert!(To::Gpkg.takes(Path::new("a.kml")) && !To::Cog.takes(Path::new("a.kml")) && !To::Kml.takes(Path::new("a.tif")));
    }

    /// Needs GDAL installed, or with `COXSWAIN_GDAL_CONTAINER=1` podman with GDAL's image pulled:
    /// `cargo test -p coxswain-core -- --ignored real_gdal`. Makes its own files with ogr2ogr.
    #[test]
    #[ignore]
    fn real_gdal() {
        use crate::media::{Op, Program};
        crate::i18n::set_language("en-GB");
        let d = std::env::temp_dir().join(format!("coxswain-real-gdal-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let container = std::env::var("COXSWAIN_GDAL_CONTAINER").is_ok();
        let cfg = crate::config::PreviewConfig { prefer: if container { "container" } else { "local" }.into(), ..Default::default() };
        let json = d.join("-roads.geojson");
        std::fs::write(&json, r#"{"type":"FeatureCollection","features":[
            {"type":"Feature","properties":{"name":"A"},"geometry":{"type":"LineString","coordinates":[[12.57,55.70],[12.60,55.78]]}},
            {"type":"Feature","properties":{"name":"B"},"geometry":{"type":"LineString","coordinates":[[12.50,55.60],[12.52,55.65]]}}]}"#).unwrap();
        std::fs::write(d.join("walk.gpx"), GPX).unwrap();
        let stop = std::sync::atomic::AtomicBool::new(false);
        let run = |op: Op, file: &Path| crate::media::run_all(&cfg, &op, &[file.to_path_buf()], &stop, |_| {}, |_| {}).remove(0).unwrap_or_else(|e| panic!("{op:?} of {}: {e}", file.display()));
        let gpkg = run(Op::Geo(To::Gpkg), &json);
        assert_eq!(gpkg, d.join("-roads.gpkg"));
        for (to, want) in [(To::Shapefile, "-roads.shp.zip"), (To::Kml, "-roads.kml"), (To::Gpx, "-roads.gpx"), (To::GeoJson, "-roads 2.geojson")] {
            assert_eq!(run(Op::Geo(to), &gpkg), d.join(want), "{to:?}");
        }
        let gpx = run(Op::Geo(To::GeoJson), &d.join("walk.gpx"));
        let merged = std::fs::read_to_string(&gpx).unwrap();
        assert!(merged.contains("\"layer\":\"tracks\"") && merged.contains("\"layer\":\"waypoints\""), "{merged}");
        // Waypoints alone: one filled layer of five, converted by its name.
        std::fs::write(d.join("points.gpx"), r#"<?xml version="1.0"?><gpx version="1.1" xmlns="http://www.topografix.com/GPX/1/1"><wpt lat="55.676" lon="12.568"><name>Start</name></wpt></gpx>"#).unwrap();
        let points = std::fs::read_to_string(run(Op::Geo(To::GeoJson), &d.join("points.gpx"))).unwrap();
        assert!(points.contains("Start") && points.contains("Point"), "{points}");
        let facts = facts(&cfg, &gpkg);
        assert!(facts.iter().any(|(_, v)| v.contains("2 features")), "{facts:?}");
        let how = crate::media::how(&cfg, Program::Gdal).unwrap();
        let svg = d.join("map.svg");
        crate::media::preview(&how, &Op::Map, &d.join("-roads.shp.zip"), &svg, std::time::Duration::from_secs(60)).ok();
        crate::media::preview(&how, &Op::Map, &gpkg, &svg, std::time::Duration::from_secs(60)).unwrap();
        assert!(std::fs::read_to_string(&svg).unwrap().contains("<path"));
        // A raster: the lines burnt into a GeoTIFF by gdal_rasterize.
        let tif = d.join("lines.tif");
        let made = crate::searchable::Scratch::named("geo-test").unwrap();
        crate::media::in_folder(&how, &gpkg, &made.0, &stop, None, &mut |_| {}, ENV, |r| {
            let out = r.out("lines.tif");
            let input = r.input.clone();
            (r.exec)("gdal_rasterize", &["-q", "-burn", "200", "-ts", "300", "200", "-ot", "Byte", "-of", "GTiff", &input, &out].map(String::from))
        })
        .unwrap();
        std::fs::copy(made.0.join("lines.tif"), &tif).unwrap();
        assert!(is_geotiff(&tif));
        assert_eq!(run(Op::Geo(To::Cog), &tif), d.join("lines.cog.tif"));
        assert_eq!(run(Op::Geo(To::Png), &tif), d.join("lines.png"));
        let facts = super::facts(&cfg, &tif);
        assert!(facts.iter().any(|(_, v)| v == "300 × 200"), "{facts:?}");
        crate::media::preview(&how, &Op::Map, &tif, &svg, std::time::Duration::from_secs(60)).unwrap();
        assert!(std::fs::read_to_string(&svg).unwrap().contains("data:image/png;base64,"));
        let wrong = crate::media::run_all(&cfg, &Op::Geo(To::Png), std::slice::from_ref(&gpkg), &stop, |_| {}, |_| {}).remove(0);
        assert_eq!(wrong, Err(t!("geo.needs_raster")));
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn geo_maps_are_drawn_north_up_and_narrower_away_from_the_equator() {
        let json = r#"{"type":"FeatureCollection","features":[
            {"type":"Feature","geometry":{"type":"LineString","coordinates":[[12.0,55.0],[13.0,56.0]]}},
            {"type":"Feature","geometry":{"type":"Polygon","coordinates":[[[12,55],[12.5,55],[12.5,55.5],[12,55]]]}},
            {"type":"Feature","geometry":{"type":"MultiPoint","coordinates":[[12.2,55.2],[12.8,55.8]]}},
            {"type":"Feature","geometry":null}]}"#;
        let shapes = geojson(json, true).unwrap();
        assert_eq!(shapes.shapes.len(), 4);
        let s = svg(&[shapes]);
        assert!(s.starts_with("<svg") && s.ends_with("</svg>") && s.contains("<circle") && s.contains("fill-opacity"));
        // One degree of longitude at 55.5° N is 0.57 of a degree of latitude: taller than wide.
        let w: f64 = s.split("width=\"").nth(1).unwrap().split('"').next().unwrap().parse().unwrap();
        let h: f64 = s.split("height=\"").nth(1).unwrap().split('"').next().unwrap().parse().unwrap();
        assert!(h == SIDE && w < 0.65 * SIDE, "{w} × {h}");
        assert!(svg(&[]).ends_with("</svg>"), "nothing to draw draws nothing");
        assert!(!svg(&gpx(GPX).layers()).contains("Morning"), "no text of the file in the picture");
    }
}
