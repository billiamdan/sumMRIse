//! The icon and version information of `SumMRIse.exe` (OpenSpec change
//! `windows-distribution`), compiled with the resource compiler of the
//! Windows SDK. Only when building on Windows for Windows: elsewhere there is
//! no resource compiler, and nothing to do.

fn main() {
    println!("cargo:rerun-if-changed=../../packaging/windows/summrise.ico");
    println!("cargo:rerun-if-changed=build.rs");
    let windows_target = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    if !(windows_target && cfg!(windows)) {
        return;
    }
    let mut resources = winresource::WindowsResource::new();
    resources
        .set_icon("../../packaging/windows/summrise.ico")
        .set("ProductName", "SumMRIse")
        .set("FileDescription", "SumMRIse")
        .set("InternalName", "summrise")
        .set("OriginalFilename", "SumMRIse.exe")
        .set("CompanyName", "SumMRIse")
        .set("LegalCopyright", "Copyright (c) billiamdan. MIT licence.")
        .set("Comments", "Viewer for the MRI, CT and PET scans on patient discs. Not a medical device.");
    if let Err(e) = resources.compile() {
        panic!("the Windows resources (icon, version) could not be compiled: {e}");
    }
}
