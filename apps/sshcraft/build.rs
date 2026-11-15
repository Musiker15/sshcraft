//! The Windows program gets the app icon and its version information (Explorer, the taskbar and
//! the installer show them). Other systems take the icon from their packages.

fn main() {
    println!("cargo::rerun-if-changed=../../assets/app-icon/sshcraft.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../assets/app-icon/sshcraft.ico");
    res.set("ProductName", "SSHCraft");
    res.set("FileDescription", "SSHCraft");
    res.set("CompanyName", "MSK Scripts");
    res.set("LegalCopyright", "MIT OR Apache-2.0");
    if let Err(e) = res.compile() {
        // A release build without the icon is a mistake; a local build may lack the resource
        // compiler and still runs.
        if std::env::var("SSHCRAFT_REQUIRE_WINRES").as_deref() == Ok("1") {
            panic!("the Windows resources could not be built: {e}");
        }
        println!("cargo::warning=the Windows resources could not be built: {e}");
    }
}
