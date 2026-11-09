//! Windows only: embed the app icon and version info into `iridium_cad.exe`. Elsewhere a no-op.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/app-icon/iridium-cad.ico");
    println!("cargo:rerun-if-env-changed=IRIDIUM_CAD_REQUIRE_WINRES");
    println!("cargo:rerun-if-env-changed=IRIDIUM_CAD_BUILD_SHA");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    if std::path::Path::new("../../assets/app-icon/iridium-cad.ico").exists() {
        res.set_icon("../../assets/app-icon/iridium-cad.ico");
    }
    res.set("ProductName", "Cad")
        .set("FileDescription", "Cad computer-aided design")
        .set("CompanyName", "Iridium Technologies, Inc.")
        .set("LegalCopyright", "Copyright (c) 2026 the upstream project Team and the Cad contributors. MIT OR Apache-2.0.")
        .set("OriginalFilename", "iridium_cad.exe")
        .set("InternalName", "iridium_cad");
    if let Err(e) = res.compile() {
        if std::env::var_os("IRIDIUM_CAD_REQUIRE_WINRES").is_some() {
            eprintln!("embedding Windows resources failed: {e}");
            std::process::exit(1);
        }
        println!("cargo:warning=iridium_cad.exe built without icon/version resources: {e}");
    }
}
