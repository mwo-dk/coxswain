//! File-type icons: a Nerd Font glyph and a color per name or extension.

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Icon {
    pub glyph: &'static str,
    /// `#rrggbb`, or empty to use the theme's color for the entry.
    pub color: &'static str,
}

const fn i(glyph: &'static str, color: &'static str) -> Icon {
    Icon { glyph, color }
}

const FOLDER: Icon = i("\u{f07b}", "");
const FILE: Icon = i("\u{f15b}", "");

/// Exact names first (lowercase), then extensions.
const NAMES: &[(&str, Icon)] = &[
    (".git", i("\u{e5fb}", "#f14e32")),
    ("node_modules", i("\u{e5fa}", "#8bc34a")),
    (".github", i("\u{e5fd}", "")),
    (".vscode", i("\u{e70c}", "#3b9ae1")),
    ("src", i("\u{f07c}", "")),
    ("target", i("\u{f487}", "#6d8086")),
    ("dist", i("\u{f487}", "#6d8086")),
    ("build", i("\u{f487}", "#6d8086")),
    ("docs", i("\u{f02d}", "")),
    ("cargo.toml", i("\u{e7a8}", "#dea584")),
    ("cargo.lock", i("\u{e7a8}", "#6d8086")),
    ("package.json", i("\u{e71e}", "#e8274b")),
    ("package-lock.json", i("\u{e71e}", "#6d8086")),
    ("dockerfile", i("\u{f308}", "#0db7ed")),
    ("docker-compose.yml", i("\u{f308}", "#0db7ed")),
    ("makefile", i("\u{e779}", "#6d8086")),
    ("license", i("\u{f0219}", "#d0bf41")),
    ("readme.md", i("\u{f00ba}", "#42a5f5")),
    (".gitignore", i("\u{f1d3}", "#f14e32")),
    (".gitattributes", i("\u{f1d3}", "#f14e32")),
    (".env", i("\u{f462}", "#faf743")),
];

const EXTS: &[(&str, Icon)] = &[
    ("rs", i("\u{e7a8}", "#dea584")),
    ("toml", i("\u{e6b2}", "#9c4221")),
    ("md", i("\u{e73e}", "#42a5f5")),
    ("js", i("\u{e74e}", "#f1e05a")),
    ("mjs", i("\u{e74e}", "#f1e05a")),
    ("ts", i("\u{e628}", "#3178c6")),
    ("tsx", i("\u{e7ba}", "#3178c6")),
    ("jsx", i("\u{e7ba}", "#61dafb")),
    ("svelte", i("\u{e697}", "#ff3e00")),
    ("vue", i("\u{e6a0}", "#41b883")),
    ("json", i("\u{e60b}", "#cbcb41")),
    ("html", i("\u{e736}", "#e34c26")),
    ("css", i("\u{e749}", "#42a5f5")),
    ("scss", i("\u{e603}", "#cd6799")),
    ("py", i("\u{e73c}", "#3572a5")),
    ("go", i("\u{e627}", "#00add8")),
    ("c", i("\u{e61e}", "#599eff")),
    ("h", i("\u{f0fd}", "#a074c4")),
    ("cpp", i("\u{e61d}", "#f34b7d")),
    ("hpp", i("\u{f0fd}", "#a074c4")),
    ("cs", i("\u{f031b}", "#178600")),
    ("csproj", i("\u{f031b}", "#512bd4")),
    ("sln", i("\u{e70c}", "#854cc7")),
    ("java", i("\u{e738}", "#b07219")),
    ("kt", i("\u{e634}", "#7f52ff")),
    ("swift", i("\u{e755}", "#f05138")),
    ("rb", i("\u{e739}", "#cc342d")),
    ("php", i("\u{e73d}", "#777bb3")),
    ("lua", i("\u{e620}", "#51a0cf")),
    ("sh", i("\u{f489}", "#89e051")),
    ("bash", i("\u{f489}", "#89e051")),
    ("zsh", i("\u{f489}", "#89e051")),
    ("fish", i("\u{f489}", "#89e051")),
    ("ps1", i("\u{e683}", "#4273ca")),
    ("bat", i("\u{e629}", "#c1f12e")),
    ("yml", i("\u{e6a8}", "#cb171e")),
    ("yaml", i("\u{e6a8}", "#cb171e")),
    ("xml", i("\u{f05c0}", "#e37933")),
    ("sql", i("\u{f1c0}", "#dad8d8")),
    ("db", i("\u{f1c0}", "#dad8d8")),
    ("sqlite", i("\u{f1c0}", "#dad8d8")),
    ("lock", i("\u{f023}", "#6d8086")),
    ("nix", i("\u{f313}", "#7ebae4")),
    ("tf", i("\u{e69a}", "#844fba")),
    ("bicep", i("\u{e70f}", "#519aba")),
    ("txt", i("\u{f15c}", "#89a3b8")),
    ("log", i("\u{f18d}", "#89a3b8")),
    ("csv", i("\u{f1c3}", "#89e051")),
    ("xlsx", i("\u{f1c3}", "#207245")),
    ("docx", i("\u{f1c2}", "#185abd")),
    ("pptx", i("\u{f1c4}", "#c43e1c")),
    ("pdf", i("\u{f1c1}", "#e53935")),
    ("png", i("\u{f1c5}", "#a074c4")),
    ("jpg", i("\u{f1c5}", "#a074c4")),
    ("jpeg", i("\u{f1c5}", "#a074c4")),
    ("gif", i("\u{f1c5}", "#a074c4")),
    ("webp", i("\u{f1c5}", "#a074c4")),
    ("bmp", i("\u{f1c5}", "#a074c4")),
    ("ico", i("\u{f1c5}", "#cbcb41")),
    ("svg", i("\u{f0721}", "#ffb13b")),
    ("psd", i("\u{e7b8}", "#31a8ff")),
    ("mp4", i("\u{f03d}", "#fd971f")),
    ("mkv", i("\u{f03d}", "#fd971f")),
    ("mov", i("\u{f03d}", "#fd971f")),
    ("webm", i("\u{f03d}", "#fd971f")),
    ("avi", i("\u{f03d}", "#fd971f")),
    ("mp3", i("\u{f001}", "#e8a33d")),
    ("flac", i("\u{f001}", "#e8a33d")),
    ("wav", i("\u{f001}", "#e8a33d")),
    ("ogg", i("\u{f001}", "#e8a33d")),
    ("m4a", i("\u{f001}", "#e8a33d")),
    ("zip", i("\u{f410}", "#eca517")),
    ("gz", i("\u{f410}", "#eca517")),
    ("tgz", i("\u{f410}", "#eca517")),
    ("xz", i("\u{f410}", "#eca517")),
    ("zst", i("\u{f410}", "#eca517")),
    ("7z", i("\u{f410}", "#eca517")),
    ("rar", i("\u{f410}", "#eca517")),
    ("tar", i("\u{f410}", "#eca517")),
    ("iso", i("\u{e271}", "#d0bf41")),
    ("exe", i("\u{f17a}", "#00a4ef")),
    ("msi", i("\u{f17a}", "#00a4ef")),
    ("dll", i("\u{f107c}", "#6d8086")),
    ("so", i("\u{f107c}", "#6d8086")),
    ("deb", i("\u{e77d}", "#a80030")),
    ("rpm", i("\u{e7bb}", "#ee0000")),
    ("appimage", i("\u{f17c}", "#fcc624")),
    ("ttf", i("\u{f031}", "#ececec")),
    ("otf", i("\u{f031}", "#ececec")),
    ("woff2", i("\u{f031}", "#ececec")),
    ("key", i("\u{f084}", "#e0af68")),
    ("pem", i("\u{f084}", "#e0af68")),
    ("crt", i("\u{f0a3}", "#e0af68")),
];

pub fn icon(name: &str, is_dir: bool) -> Icon {
    let lower = name.to_lowercase();
    if let Some((_, ic)) = NAMES.iter().find(|(n, _)| *n == lower) {
        return *ic;
    }
    if is_dir {
        return FOLDER;
    }
    match lower.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => EXTS.iter().find(|(e, _)| *e == ext).map_or(FILE, |(_, ic)| *ic),
        _ => FILE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_lookup() {
        assert_eq!(icon("main.RS", false).color, "#dea584");
        assert_eq!(icon("Cargo.toml", false), icon("cargo.toml", false));
        assert_eq!(icon("x", true), FOLDER);
        assert_eq!(icon(".git", true).color, "#f14e32");
        assert_eq!(icon(".bashrc", false), FILE);
        assert_eq!(icon("noext", false), FILE);
    }
}
