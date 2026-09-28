//! Zip and tar archives: list what is inside, and extract. Both crates refuse entries that
//! would land outside the destination (`..`, absolute paths).

use serde::Serialize;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ArchiveEntry {
    /// Path inside the archive, `/`-separated.
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Zip,
    Tar,
    TarGz,
}

fn kind(path: &Path) -> Option<Kind> {
    let n = path.file_name()?.to_string_lossy().to_lowercase();
    if n.ends_with(".tar.gz") || n.ends_with(".tgz") {
        Some(Kind::TarGz)
    } else if n.ends_with(".tar") {
        Some(Kind::Tar)
    } else if [".zip", ".jar", ".apk", ".nupkg", ".whl", ".vsix"].iter().any(|e| n.ends_with(e)) {
        Some(Kind::Zip)
    } else {
        None
    }
}

pub fn is_archive(path: &Path) -> bool {
    kind(path).is_some()
}

fn not_archive(path: &Path) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, format!("{} is not a zip or tar archive", path.display()))
}

fn tar_reader(path: &Path, k: Kind) -> io::Result<tar::Archive<Box<dyn Read>>> {
    let f = BufReader::new(File::open(path)?);
    let r: Box<dyn Read> = if k == Kind::TarGz { Box::new(flate2::read::GzDecoder::new(f)) } else { Box::new(f) };
    Ok(tar::Archive::new(r))
}

/// The first `max` entries, and whether there were more.
pub fn list(path: &Path, max: usize) -> io::Result<(Vec<ArchiveEntry>, bool)> {
    let k = kind(path).ok_or_else(|| not_archive(path))?;
    let mut out = vec![];
    if k == Kind::Zip {
        let mut z = zip::ZipArchive::new(BufReader::new(File::open(path)?)).map_err(io::Error::other)?;
        for i in 0..z.len().min(max) {
            let e = z.by_index_raw(i).map_err(io::Error::other)?;
            out.push(ArchiveEntry { name: e.name().trim_end_matches('/').into(), size: e.size(), is_dir: e.is_dir() });
        }
        return Ok((out, z.len() > max));
    }
    let mut a = tar_reader(path, k)?;
    for e in a.entries()? {
        if out.len() == max {
            return Ok((out, true));
        }
        let e = e?;
        let name = e.path()?.to_string_lossy().trim_end_matches('/').to_string();
        out.push(ArchiveEntry { name, size: e.size(), is_dir: e.header().entry_type().is_dir() });
    }
    Ok((out, false))
}

/// Extract into a new folder named after the archive inside `dest_dir`. Returns that folder.
pub fn extract(path: &Path, dest_dir: &Path) -> io::Result<PathBuf> {
    let k = kind(path).ok_or_else(|| not_archive(path))?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let lower = name.to_lowercase();
    let stem_len = [".tar.gz", ".tgz", ".tar"].iter().find(|e| lower.ends_with(*e)).map_or_else(
        || name.rfind('.').unwrap_or(name.len()),
        |e| name.len() - e.len(),
    );
    let to = dest_dir.join(&name[..stem_len]);
    if to.exists() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{} exists", to.display())));
    }
    std::fs::create_dir_all(&to)?;
    let r = if k == Kind::Zip {
        zip::ZipArchive::new(BufReader::new(File::open(path)?)).and_then(|mut z| z.extract(&to)).map_err(io::Error::other)
    } else {
        tar_reader(path, k)?.unpack(&to)
    };
    r.map(|_| to)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_zip_and_tar_roundtrip() {
        let d = std::env::temp_dir().join(format!("bosum-test-archive-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();

        let zp = d.join("pack.zip");
        let mut w = zip::ZipWriter::new(File::create(&zp).unwrap());
        w.add_directory("sub/", zip::write::SimpleFileOptions::default()).unwrap();
        w.start_file("sub/a.txt", zip::write::SimpleFileOptions::default()).unwrap();
        io::Write::write_all(&mut w, b"hello").unwrap();
        w.finish().unwrap();
        let (entries, more) = list(&zp, 10).unwrap();
        assert!(!more);
        assert_eq!(entries[1], ArchiveEntry { name: "sub/a.txt".into(), size: 5, is_dir: false });
        assert!(entries[0].is_dir);
        assert_eq!(list(&zp, 1).unwrap(), (vec![entries[0].clone()], true));
        let out = extract(&zp, &d).unwrap();
        assert_eq!(out, d.join("pack"));
        assert_eq!(std::fs::read_to_string(out.join("sub/a.txt")).unwrap(), "hello");
        assert!(extract(&zp, &d).is_err(), "never extracts over an existing folder");

        let tp = d.join("pack2.tar.gz");
        let gz = flate2::write::GzEncoder::new(File::create(&tp).unwrap(), flate2::Compression::fast());
        let mut t = tar::Builder::new(gz);
        t.append_path_with_name(d.join("pack/sub/a.txt"), "x/a.txt").unwrap();
        t.into_inner().unwrap().finish().unwrap();
        assert_eq!(list(&tp, 10).unwrap().0, [ArchiveEntry { name: "x/a.txt".into(), size: 5, is_dir: false }]);
        assert_eq!(std::fs::read_to_string(extract(&tp, &d).unwrap().join("x/a.txt")).unwrap(), "hello");

        assert!(list(&d.join("pack/sub/a.txt"), 10).is_err());
        std::fs::remove_dir_all(d).unwrap();
    }
}
