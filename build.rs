// Embeds the Graphsight icon into the Windows .exe (Explorer, taskbar, Alt-Tab).
// Needs the Windows SDK's rc.exe (installed with Visual Studio Build Tools). If it is missing the build
// still succeeds, the exe just gets the default icon; the window icon is set at runtime regardless.
fn main() {
    println!("cargo:rerun-if-changed=assets/icon/graphsight.ico");
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon/graphsight.ico");
        res.set("ProductName", "Graphsight");
        res.set("FileDescription", "Graphsight graph viewer");
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed the exe icon: {e}");
        }
    }
}
