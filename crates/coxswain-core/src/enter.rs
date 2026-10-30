//! What Enter and F3 do on an entry (docs/files/opening-things.md), one rule for both apps:
//! Enter goes into anything that holds things (a folder, an archive, a disk or container image,
//! a mailbox) and opens a games file on its games (a single game on its board); F3 shows the
//! thing itself, and a games file's games as well. The key line says what Enter does (`label`).

use serde::Serialize;
use std::path::Path;

/// Where an entry is: on the disk, or inside an archive (at any depth). Inside one only names
/// count: nothing is read to say what Enter does there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Disk,
    Archive,
}

/// What Enter does on an entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Enter {
    /// Into it: a folder.
    Folder,
    /// Into it, like a folder: an archive (a comic too).
    Archive,
    /// Into it, read-only: a disk image (while those open), a container image, a mailbox.
    DiskImage,
    Image,
    Mailbox,
    /// Its games in their table, a file of one game on its board: PGN, SCID, ChessBase, SGF,
    /// PDN, PBN, KIF, CSA, xiangqi.
    Games,
    /// An EPD or FEN file's positions.
    Positions,
    /// A Polyglot opening book, on its board.
    Book,
    /// None of these: what the app does with a file (its program, Join, a check of checksums).
    Other,
}

/// What F3 does on a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum View {
    /// Its games (or positions, or book), as Enter.
    Games,
    /// The file itself: text, hex or its preview.
    Itself,
}

/// What Enter does on the entry at `path` in `place`. On the disk a plain tar is read once to
/// tell a container image, and a `.bin` to tell an opening book; inside an archive, names only.
/// A file only in the cloud is told by its name too: the apps ask this of every file the cursor
/// comes to, and reading it would download it.
pub fn enter(path: &Path, is_dir: bool, place: Place) -> Enter {
    if is_dir {
        return Enter::Folder;
    }
    let read = place == Place::Disk && !crate::cloud::unasked(path);
    let opens = match place {
        Place::Disk if read => crate::archive::opens_as(path),
        Place::Disk => crate::archive::named_opens_as(path),
        Place::Archive => crate::archive::opens_inside_as(path),
    };
    if let Some(k) = opens {
        return k;
    }
    let games = if read { crate::chess::sniff(path) } else { crate::chess::format(path) };
    use crate::chess::Format as F;
    match games {
        Some(F::Epd) => Enter::Positions,
        Some(F::Book) => Enter::Book,
        Some(_) => Enter::Games,
        None => Enter::Other,
    }
}

/// What F3 does on the file at `path` in `place`.
pub fn view(path: &Path, place: Place) -> View {
    match enter(path, false, place) {
        Enter::Games | Enter::Positions | Enter::Book => View::Games,
        _ => View::Itself,
    }
}

/// Inside an archive, a file whose name does not say whether it is games but whose bytes may: a
/// `.bin` opening book, a DhtmlXQ xiangqi game (`.che`, `.dhtmlxq`). Enter looks at a copy.
pub fn may_be_games(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| ["bin", "che", "dhtmlxq"].iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// What the key line says Enter does: "Enter: Open games". `None` for a folder and for a file
/// Enter hands to its program: nothing to say there.
pub fn label(e: Enter) -> Option<String> {
    let id = match e {
        Enter::Folder | Enter::Other => return None,
        Enter::Archive => "archive",
        Enter::DiskImage => "disk_image",
        Enter::Image => "image",
        Enter::Mailbox => "mailbox",
        Enter::Games => "games",
        Enter::Positions => "positions",
        Enter::Book => "book",
    };
    Some(crate::t!(&format!("enter.{id}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Every kind × every place: on the disk, inside a zip, inside a 7z, and two levels down
    /// (zip in tar.gz in zip). Inside an archive only the name counts, so a name is enough.
    #[test]
    fn enter_and_f3_across_the_matrix() {
        let d = crate::fs::Scratch::named("enter");
        // On the disk, files that are what they say (a container tar and a book are read).
        let book = d.join("openings.bin");
        // One Polyglot entry: a key, a move, a weight, a learn value; 16 bytes.
        std::fs::write(&book, [0x46, 0x3b, 0x96, 0x18, 0x16, 0x91, 0xfc, 0x9c, 0x01, 0x95, 0x00, 0x0c, 0, 0, 0, 0]).unwrap();
        let noise = d.join("noise.bin");
        std::fs::write(&noise, b"not a book").unwrap();
        let places: Vec<(&str, Place, PathBuf)> = vec![
            ("disk", Place::Disk, d.to_path_buf()),
            ("zip", Place::Archive, PathBuf::from("/x/all.zip")),
            ("7z", Place::Archive, PathBuf::from("/x/all.7z")),
            ("nested", Place::Archive, PathBuf::from("/x/nested.zip/mid.tar.gz/inner.zip")),
        ];
        use Enter::*;
        let cases: &[(&str, Enter)] = &[
            ("plain.zip", Archive),
            ("plain.7z", Archive),
            ("parts.7z.001", Archive),
            ("split.zip.001", Archive),
            ("plain.tar", Archive),
            ("plain.tar.gz", Archive),
            ("plain.tgz", Archive),
            ("plain.tar.bz2", Archive),
            ("plain.tar.xz", Archive),
            ("plain.tar.zst", Archive),
            ("plain.jar", Archive),
            ("comic.cbz", Archive),
            ("mail.mbox", Mailbox),
            ("multi.pgn", Games),
            ("single.pgn", Games),
            ("multi.pgn.gz", Games),
            ("club.si4", Games),
            ("Classics.cbh", Games),
            ("twic1667.cbv", Games),
            ("multi.sgf", Games),
            ("club.pdn", Games),
            ("pairs.pbn", Games),
            ("afternoon.kifu", Games),
            ("afternoon.csa", Games),
            ("xiangqi.pgn", Games),
            ("mates.epd", Positions),
            ("notes.txt", Other),
            ("club.sg4", Other),
        ];
        for (place_name, place, base) in &places {
            for (name, want) in cases {
                let p = base.join(name);
                // On the disk the archives are looked at by their names, as files that are there.
                if *place == Place::Disk {
                    std::fs::write(&p, b"").unwrap();
                }
                assert_eq!(enter(&p, false, *place), *want, "Enter on {name} ({place_name})");
                let f3 = if matches!(want, Games | Positions | Book) { View::Games } else { View::Itself };
                assert_eq!(view(&p, *place), f3, "F3 on {name} ({place_name})");
            }
            assert_eq!(enter(&base.join("folder"), true, *place), Folder);
        }
        // A book is one by its bytes on the disk; inside an archive Enter looks at a copy.
        assert_eq!(enter(&book, false, Place::Disk), Book);
        assert_eq!(view(&book, Place::Disk), View::Games);
        assert_eq!(enter(&noise, false, Place::Disk), Other);
        assert_eq!(enter(Path::new("/x/all.zip/openings.bin"), false, Place::Archive), Other);
        assert!(may_be_games(Path::new("/x/all.zip/openings.bin")));
        assert!(!may_be_games(Path::new("/x/all.zip/notes.txt")));
    }

    /// A file only in the cloud is told by its name: nothing is read (which would download it)
    /// to tell a container image from a tar or a book from a `.bin`, until the user asks for it.
    #[test]
    fn a_file_only_in_the_cloud_is_not_read_to_tell_what_enter_does() {
        let d = crate::fs::Scratch::named("enter-cloud");
        let image = d.join("demo.tar");
        std::fs::write(&image, crate::container_image::tests::docker_tar_of(&[crate::container_image::tests::tar_of(&[("a.txt", b"a")])])).unwrap();
        let book = d.join("openings.bin");
        std::fs::write(&book, [0x46, 0x3b, 0x96, 0x18, 0x16, 0x91, 0xfc, 0x9c, 0x01, 0x95, 0x00, 0x0c, 0, 0, 0, 0]).unwrap();
        for p in [&image, &book] {
            crate::cloud::pretend(p, true);
        }
        assert_eq!(enter(&image, false, Place::Disk), Enter::Archive, "a tar by its name, not read");
        assert_eq!(enter(&book, false, Place::Disk), Enter::Other, "a .bin by its name, not read");
        for p in [&image, &book] {
            crate::cloud::pretend(p, false);
        }
        assert_eq!(enter(&image, false, Place::Disk), Enter::Image);
        assert_eq!(enter(&book, false, Place::Disk), Enter::Book);
    }

    #[test]
    fn disk_images_open_inside_archives_while_that_option_is_on() {
        crate::disk_image::FORCE.with(|f| f.set(true));
        let d = crate::fs::Scratch::named("enter-disk");
        let iso = d.join("setup.iso");
        std::fs::write(&iso, b"").unwrap();
        assert_eq!(enter(&iso, false, Place::Disk), Enter::DiskImage);
        assert_eq!(enter(Path::new("/x/all.zip/setup.iso"), false, Place::Archive), Enter::DiskImage);
        assert_eq!(view(Path::new("/x/all.zip/setup.iso"), Place::Archive), View::Itself);
        crate::disk_image::FORCE.with(|f| f.set(false));
        assert_eq!(enter(Path::new("/x/all.zip/setup.iso"), false, Place::Archive), Enter::Other);
    }

    #[test]
    fn the_key_line_names_what_enter_does() {
        assert_eq!(label(Enter::Folder), None);
        assert_eq!(label(Enter::Other), None);
        for e in [Enter::Archive, Enter::DiskImage, Enter::Image, Enter::Mailbox, Enter::Games, Enter::Positions, Enter::Book] {
            assert!(label(e).is_some_and(|l| !l.starts_with("enter.")), "{e:?}");
        }
    }
}
