//! Packages for Windows and Linux, and what every package carries.
//!
//!   package-windows [--msi] [--sign <file.pfx>]
//!       dist/SumMRIse-<version>-windows-x64.zip (and .msi with WiX 5 on PATH as `wix`);
//!       with --sign, the executable and the installer are Authenticode-signed with
//!       signtool (password in WINDOWS_CERT_PASSWORD, timestamp server in
//!       WINDOWS_TIMESTAMP_URL or DigiCert's)
//!   package-linux [--appimagetool <tool> --appimage-runtime <runtime>]
//!       dist/SumMRIse-<version>-linux-x64.tar.gz (and the AppImage)
//!   release-notes <version> [--signed-macos] [--signed-windows]
//!       the CHANGELOG.md section of <version> and what the downloads are

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail, ensure};

use crate::{APP_NAME, BIN, BUNDLE_ID, flag, notices, option, root, run, version};

/// The files every package carries besides the program: its licence and the
/// notices of the third-party software in it, under these names.
pub(crate) fn licence_files(root: &Path) -> [(PathBuf, &'static str); 2] {
    [(root.join("LICENSE"), "LICENSE.txt"), (root.join(notices::FILE), notices::FILE)]
}

/// `cargo build --release` of the application with the release features.
fn build_release(root: &Path) -> Result<PathBuf> {
    run(Command::new(env!("CARGO")).current_dir(root).args([
        "build",
        "--release",
        "--locked",
        "-p",
        BIN,
        "--features",
        notices::RELEASE_FEATURES,
    ]))?;
    let exe = root.join("target/release").join(if cfg!(windows) { format!("{BIN}.exe") } else { BIN.to_owned() });
    ensure!(exe.is_file(), "{} was not built", exe.display());
    Ok(exe)
}

/// An empty folder at `path`.
fn fresh_dir(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_dir_all(path).with_context(|| format!("cannot remove {}", path.display()))?;
    }
    fs::create_dir_all(path)?;
    Ok(())
}

fn copy(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(from, to).with_context(|| format!("cannot copy {} to {}", from.display(), to.display()))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Windows

pub(crate) fn package_windows(args: &[String]) -> Result<()> {
    ensure!(cfg!(windows), "package-windows must run on Windows (MSVC, signtool, WiX)");
    let root = root();
    let version = version()?;
    let exe = build_release(&root)?;
    let dist = root.join("dist");
    let stage = dist.join("windows").join(APP_NAME);
    fresh_dir(&stage)?;
    let staged_exe = stage.join(format!("{APP_NAME}.exe"));
    copy(&exe, &staged_exe)?;
    for (from, name) in licence_files(&root) {
        copy(&from, &stage.join(name))?;
    }
    let pfx = option(args, "--sign");
    if let Some(pfx) = pfx {
        sign_windows(Path::new(pfx), &staged_exe)?;
    }
    let zip = dist.join(format!("{APP_NAME}-{version}-windows-x64.zip"));
    write_zip(&stage, APP_NAME, &zip)?;
    println!("built {}", zip.display());

    if flag(args, "--msi") {
        let msi = dist.join(format!("{APP_NAME}-{version}-windows-x64.msi"));
        let _ = fs::remove_file(&msi);
        run(Command::new("wix")
            .current_dir(&root)
            .args(["build", "-arch", "x64", "-d"])
            .arg(format!("Version={version}"))
            .arg("-d")
            .arg(format!("Stage={}", stage.display()))
            .arg("-d")
            .arg(format!("Icon={}", root.join("packaging/windows/summrise.ico").display()))
            .arg("-o")
            .arg(&msi)
            .arg(root.join("packaging/windows/SumMRIse.wxs")))?;
        if let Some(pfx) = pfx {
            sign_windows(Path::new(pfx), &msi)?;
        }
        println!("built {}", msi.display());
    }
    Ok(())
}

/// Signs `file` with the certificate in `pfx` and a timestamp (SHA-256).
fn sign_windows(pfx: &Path, file: &Path) -> Result<()> {
    let password = std::env::var("WINDOWS_CERT_PASSWORD").context("WINDOWS_CERT_PASSWORD is not set")?;
    let timestamp =
        std::env::var("WINDOWS_TIMESTAMP_URL").unwrap_or_else(|_| "http://timestamp.digicert.com".to_owned());
    let signtool = signtool()?;
    // The password goes to signtool only; `run` would print the command line.
    eprintln!("$ signtool sign … {}", file.display());
    let status = Command::new(&signtool)
        .args(["sign", "/fd", "SHA256", "/td", "SHA256", "/tr", &timestamp, "/f"])
        .arg(pfx)
        .args(["/p", &password])
        .arg(file)
        .status()
        .context("cannot run signtool")?;
    ensure!(status.success(), "signtool failed ({status}) for {}", file.display());
    run(Command::new(&signtool).args(["verify", "/pa", "/v"]).arg(file))
}

/// The newest x64 signtool of the Windows SDK.
fn signtool() -> Result<PathBuf> {
    let kits = Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    let mut found: Vec<PathBuf> = fs::read_dir(kits)
        .with_context(|| format!("no Windows SDK in {}", kits.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("x64").join("signtool.exe"))
        .filter(|path| path.is_file())
        .collect();
    found.sort();
    found.pop().context("no signtool.exe in the Windows SDK")
}

/// Writes the files below `folder` into the ZIP archive `zip`, under the
/// folder `name` (deflate, forward slashes, sorted).
pub(crate) fn write_zip(folder: &Path, name: &str, zip: &Path) -> Result<()> {
    let mut files = Vec::new();
    collect_files(folder, &mut files)?;
    files.sort();
    let mut writer = zip::ZipWriter::new(fs::File::create(zip)?);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o755);
    for file in files {
        let relative = file.strip_prefix(folder)?.to_string_lossy().replace('\\', "/");
        writer.start_file(format!("{name}/{relative}"), options)?;
        writer.write_all(&fs::read(&file)?)?;
    }
    writer.finish()?;
    Ok(())
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_files(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Linux

pub(crate) fn package_linux(args: &[String]) -> Result<()> {
    ensure!(cfg!(target_os = "linux"), "package-linux must run on Linux");
    let root = root();
    let version = version()?;
    let exe = build_release(&root)?;
    let dist = root.join("dist");
    let linux = dist.join("linux");
    let stage = linux.join(BIN);
    fresh_dir(&stage)?;
    let desktop = root.join(format!("packaging/linux/{BUNDLE_ID}.desktop"));
    let metainfo = root.join(format!("packaging/linux/{BUNDLE_ID}.metainfo.xml"));
    let icon = root.join("packaging/icons/icon-256.png");
    copy(&exe, &stage.join(BIN))?;
    for (from, name) in licence_files(&root) {
        copy(&from, &stage.join(name))?;
    }
    copy(&desktop, &stage.join(format!("{BUNDLE_ID}.desktop")))?;
    copy(&metainfo, &stage.join(format!("{BUNDLE_ID}.metainfo.xml")))?;
    copy(&icon, &stage.join(format!("{BUNDLE_ID}.png")))?;
    let tarball = dist.join(format!("{APP_NAME}-{version}-linux-x64.tar.gz"));
    run(Command::new("tar").arg("-C").arg(&linux).arg("-czf").arg(&tarball).arg(BIN))?;
    println!("built {}", tarball.display());

    if let Some(tool) = option(args, "--appimagetool") {
        let runtime = option(args, "--appimage-runtime").context("--appimagetool needs --appimage-runtime")?;
        let appdir = linux.join(format!("{APP_NAME}.AppDir"));
        fresh_dir(&appdir)?;
        copy(&exe, &appdir.join("usr/bin").join(BIN))?;
        copy(&desktop, &appdir.join(format!("usr/share/applications/{BUNDLE_ID}.desktop")))?;
        copy(&metainfo, &appdir.join(format!("usr/share/metainfo/{BUNDLE_ID}.appdata.xml")))?;
        copy(&icon, &appdir.join(format!("usr/share/icons/hicolor/256x256/apps/{BUNDLE_ID}.png")))?;
        for (from, name) in licence_files(&root) {
            copy(&from, &appdir.join("usr/share/doc").join(BIN).join(name))?;
        }
        // The top of an AppDir: the desktop entry, the icon and the program to start.
        copy(&desktop, &appdir.join(format!("{BUNDLE_ID}.desktop")))?;
        copy(&icon, &appdir.join(format!("{BUNDLE_ID}.png")))?;
        copy(&icon, &appdir.join(".DirIcon"))?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(format!("usr/bin/{BIN}"), appdir.join("AppRun"))?;
        let appimage = dist.join(format!("{APP_NAME}-{version}-linux-x64.AppImage"));
        let _ = fs::remove_file(&appimage);
        // Without FUSE, as on CI runners; the runtime is given, so nothing is downloaded.
        run(Command::new(tool)
            .env("ARCH", "x86_64")
            .env("APPIMAGE_EXTRACT_AND_RUN", "1")
            .arg("--runtime-file")
            .arg(runtime)
            .arg(&appdir)
            .arg(&appimage))?;
        println!("built {}", appimage.display());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Release notes

pub(crate) fn release_notes(args: &[String]) -> Result<()> {
    let version = args.iter().find(|a| !a.starts_with("--")).context("usage: cargo xtask release-notes <version>")?;
    let version = version.trim_start_matches('v');
    let changelog = fs::read_to_string(root().join("CHANGELOG.md"))?;
    let section = changelog_section(&changelog, version)?;
    println!("{}", notes(&section, version, flag(args, "--signed-macos"), flag(args, "--signed-windows")));
    Ok(())
}

/// The text under the heading of `version` in CHANGELOG.md, up to the next
/// version's heading.
pub(crate) fn changelog_section(changelog: &str, version: &str) -> Result<String> {
    let mut lines = changelog.lines().skip_while(|line| !is_version_heading(line, version));
    if lines.next().is_none() {
        bail!("CHANGELOG.md has no section for {version}: add it before tagging");
    }
    let body: Vec<&str> = lines.take_while(|line| !line.starts_with("## ")).collect();
    let text = body.join("\n").trim().to_owned();
    ensure!(!text.is_empty(), "the CHANGELOG.md section for {version} is empty");
    Ok(text)
}

fn is_version_heading(line: &str, version: &str) -> bool {
    line.strip_prefix("## ").is_some_and(|rest| {
        let rest = rest.trim_start_matches('[');
        rest.strip_prefix(version)
            .is_some_and(|after| !after.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '.'))
    })
}

fn notes(section: &str, version: &str, signed_macos: bool, signed_windows: bool) -> String {
    let mut out = format!("{section}\n\n## Downloads\n\n");
    out.push_str(&format!(
        "| File | For |\n| --- | --- |\n\
         | `{APP_NAME}-{version}-macos-universal.dmg` | macOS 11 or later, Apple silicon and Intel |\n\
         | `{APP_NAME}-{version}-windows-x64.msi` | Windows 10 or later, installs for you only (no administrator rights) |\n\
         | `{APP_NAME}-{version}-windows-x64.zip` | Windows 10 or later, without installing |\n\
         | `{APP_NAME}-{version}-linux-x64.AppImage` | Linux (x86-64): make it executable and start it |\n\
         | `{APP_NAME}-{version}-linux-x64.tar.gz` | Linux (x86-64): the program with its desktop entry |\n\
         | `SHA256SUMS` | the SHA-256 checksum of every file (`sha256sum -c SHA256SUMS`) |\n\n"
    ));
    if !signed_macos {
        out.push_str(
            "The macOS app is not signed with a Developer ID or notarized yet, so macOS refuses to open it \
             at first. After the first attempt, choose Open Anyway in System Settings > Privacy & Security \
             (on macOS 14 and earlier, Control-click the app and choose Open).\n\n",
        );
    }
    if !signed_windows {
        out.push_str(
            "The Windows files are not code-signed yet: Microsoft Defender SmartScreen may warn about an \
             unknown publisher. Choose More info > Run anyway.\n\n",
        );
    }
    out.push_str(
        "SumMRIse is not a medical device. It is intended for personal and educational viewing and must not \
         be used for diagnosis.",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHANGELOG: &str = "# Changelog\n\n## [Unreleased]\n\nNothing.\n\n## [0.3.0] — 2026-11-09\n\n\
        ### Added\n\n- JPEG-LS.\n\n## [0.3.0-rc.1]\n\n- Candidate.\n\n## [0.2.0] — 2026-11-05\n\n- First.\n";

    #[test]
    fn the_section_of_a_version_is_found() {
        assert_eq!(changelog_section(CHANGELOG, "0.3.0").expect("found"), "### Added\n\n- JPEG-LS.");
        assert_eq!(changelog_section(CHANGELOG, "0.3.0-rc.1").expect("found"), "- Candidate.");
        assert_eq!(changelog_section(CHANGELOG, "0.2.0").expect("found"), "- First.");
        assert!(changelog_section(CHANGELOG, "0.3").is_err(), "not a prefix of 0.3.0");
        assert!(changelog_section(CHANGELOG, "0.4.0").is_err());
    }

    #[test]
    fn notes_say_what_is_not_signed() {
        let unsigned = notes("- JPEG-LS.", "0.3.0", false, false);
        assert!(unsigned.starts_with("- JPEG-LS.\n\n## Downloads"));
        assert!(unsigned.contains("SumMRIse-0.3.0-windows-x64.msi"));
        assert!(unsigned.contains("not signed with a Developer ID"));
        assert!(unsigned.contains("not code-signed"));
        assert!(unsigned.ends_with("must not be used for diagnosis."));
        let signed = notes("- JPEG-LS.", "0.3.0", true, true);
        assert!(!signed.contains("not signed") && !signed.contains("not code-signed"));
    }

    #[test]
    fn a_zip_holds_the_folder_under_its_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        let stage = dir.path().join("stage");
        fs::create_dir_all(stage.join("doc")).expect("folders");
        fs::write(stage.join("SumMRIse.exe"), b"MZ program").expect("file");
        fs::write(stage.join("doc").join("LICENSE.txt"), b"MIT").expect("file");
        let zip = dir.path().join("out.zip");
        write_zip(&stage, "SumMRIse", &zip).expect("written");
        let mut archive = zip::ZipArchive::new(fs::File::open(&zip).expect("open")).expect("archive");
        let mut names: Vec<String> = archive.file_names().map(|name| name.expect("name").into_owned()).collect();
        names.sort();
        assert_eq!(names, ["SumMRIse/SumMRIse.exe", "SumMRIse/doc/LICENSE.txt"]);
        let mut text = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("SumMRIse/doc/LICENSE.txt").expect("entry"), &mut text)
            .expect("read");
        assert_eq!(text, "MIT");
    }

    #[test]
    fn every_package_carries_the_licence_and_the_notices() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("root");
        for (path, name) in licence_files(root) {
            assert!(path.is_file(), "{} exists", path.display());
            assert!(name.ends_with(".txt"));
        }
    }
}
