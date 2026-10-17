// Embeds the Grapht icon into the Windows .exe (Explorer, taskbar, Alt-Tab).
// Needs the Windows SDK's rc.exe (installed with Visual Studio Build Tools). If it is missing the build
// still succeeds, the exe just gets the default icon; the window icon is set at runtime regardless.
fn main() {
    println!("cargo:rerun-if-changed=assets/icon/grapht.ico");
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon/grapht.ico");
        res.set("ProductName", "Grapht");
        res.set("FileDescription", "Grapht graph viewer");
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed the exe icon: {e}");
        }
    }
}
