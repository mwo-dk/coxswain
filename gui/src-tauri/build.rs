fn main() {
    // NetBSD's ld.so searches only /usr/lib: pkgsrc's GTK and WebKitGTK in /usr/pkg/lib need an
    // rpath, as pkgsrc's own packages carry one, and so does the base system's X (libepoxy
    // dlopens libEGL.so from /usr/X11R7/lib and aborts without it).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("netbsd") {
        println!("cargo:rustc-link-arg=-Wl,-R/usr/pkg/lib:/usr/X11R7/lib");
    }
    tauri_build::build()
}
