//! Puts the icon and the version into sozluk.exe (Explorer, shortcuts, Alt+Tab).
fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    #[cfg(windows)]
    {
        let mut r = winresource::WindowsResource::new();
        r.set_icon("assets/icon.ico");
        r.set("FileDescription", "Sözlük");
        r.set("ProductName", "Sözlük");
        if let Err(e) = r.compile() {
            println!("cargo:warning=icon not embedded: {e}");
        }
    }
}
