//! Embeds the app icon and version details in harshpaint.exe on Windows, so
//! Explorer, the taskbar and the Properties dialog show them.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=../../assets/harshpaint.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/harshpaint.ico");
        res.set("ProductName", "HarshPaint");
        res.set("FileDescription", "HarshPaint");
        res.set("LegalCopyright", "MIT License, HarshPaint contributors");
        res.compile()?;
    }
    Ok(())
}
