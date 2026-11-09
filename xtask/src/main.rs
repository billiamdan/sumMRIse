//! Build automation for SumMRIse — `cargo xtask <command>`.
//!
//! Commands:
//!   icons                         Regenerate the app icon (PNG set, .icns, .ico)
//!   bundle-macos [options]        Build SumMRIse.app (and optionally a .dmg), with JPEG-LS
//!       --universal               arm64 + x86_64 universal binary (default: host arch)
//!       --sign <identity>         codesign identity (default: "-" = ad-hoc)
//!       --dmg                     also create a compressed disk image
//!       --notarize <profile>      notarize + staple the DMG (xcrun notarytool keychain profile)
//!       --notary-key <file.p8> --notary-key-id <id> --notary-issuer <uuid>
//!                                 notarize + staple with an App Store Connect API key (CI)
//!   package-windows [--msi] [--sign <file.pfx>]   Windows zip (and per-user MSI with WiX 5)
//!   package-linux [--appimagetool <tool> --appimage-runtime <file>]   Linux tarball (and AppImage)
//!   release-notes <version>       Release notes from CHANGELOG.md (--signed-macos, --signed-windows)
//!   fixtures <dir> [--small]      Write a synthetic, PHI-free DICOM test CD to <dir>
//!   ci                            fmt check, clippy (deny warnings), tests, notices check
//!   i18n                          Bring the translation catalogues up to date with the source code
//!   notices [--check]             Write THIRD-PARTY-NOTICES.txt (--check: fail if out of date)

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail, ensure};
use image::{Rgba, RgbaImage};

mod notices;
mod package;

const APP_NAME: &str = "SumMRIse";
const BUNDLE_ID: &str = "com.summrise.Viewer";
const BIN: &str = "summrise";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first().map(String::as_str) else {
        print_help();
        return Ok(());
    };
    let rest = &args[1..];
    match command {
        "icons" => icons(),
        "bundle-macos" => bundle_macos(rest),
        "fixtures" => fixtures(rest),
        "ci" => ci(),
        "i18n" => i18n(),
        "notices" => notices::run(&root(), rest),
        "package-windows" => package::package_windows(rest),
        "package-linux" => package::package_linux(rest),
        "release-notes" => package::release_notes(rest),
        "-h" | "--help" | "help" => {
            print_help();
            Ok(())
        }
        other => bail!("unknown command `{other}` (try `cargo xtask help`)"),
    }
}

fn print_help() {
    println!(
        "{}",
        include_str!("main.rs")
            .lines()
            .skip(2)
            .take(17)
            .map(|l| l.trim_start_matches("//!"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("workspace root").to_path_buf()
}

fn run(cmd: &mut Command) -> Result<()> {
    eprintln!("$ {cmd:?}");
    let status = cmd.status().with_context(|| format!("failed to run {cmd:?}"))?;
    ensure!(status.success(), "command failed ({status}): {cmd:?}");
    Ok(())
}

fn flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn option<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).map(String::as_str)
}

fn version() -> Result<String> {
    let manifest = fs::read_to_string(root().join("Cargo.toml"))?;
    manifest
        .lines()
        .skip_while(|l| l.trim() != "[workspace.package]")
        .find_map(|l| l.trim().strip_prefix("version = ").map(|v| v.trim_matches('"').to_owned()))
        .context("workspace version not found")
}

// ---------------------------------------------------------------------------
// Icons

/// Renders the icon with analytic anti-aliasing (signed distance fields), in
/// the macOS Big Sur grid: an 824 pt rounded square centred in 1024 pt.
fn render_icon(size: u32) -> RgbaImage {
    let scale = size as f32 / 1024.0;
    let aa = 1.0 / scale;
    let mut img = RgbaImage::new(size, size);
    for (x, y, px) in img.enumerate_pixels_mut() {
        let p = ((x as f32 + 0.5) / scale, (y as f32 + 0.5) / scale);
        let mut c = [0.0f32; 4];
        let mut over = |rgb: [f32; 3], alpha: f32| {
            let a = alpha.clamp(0.0, 1.0);
            for i in 0..3 {
                c[i] = rgb[i] * a + c[i] * (1.0 - a);
            }
            c[3] = a + c[3] * (1.0 - a);
        };
        let coverage = |d: f32| (0.5 - d / aa).clamp(0.0, 1.0);
        let ellipse = |cx: f32, cy: f32, rx: f32, ry: f32| {
            let (dx, dy) = ((p.0 - cx) / rx, (p.1 - cy) / ry);
            (dx.hypot(dy) - 1.0) * rx.min(ry)
        };
        // Rounded square.
        let (qx, qy) = ((p.0 - 512.0).abs() - 412.0 + 185.0, (p.1 - 512.0).abs() - 412.0 + 185.0);
        let d_sq = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - 185.0;
        let t = (p.1 - 100.0) / 824.0;
        over([0.102 - 0.05 * t, 0.145 - 0.06 * t, 0.251 - 0.11 * t], coverage(d_sq));
        // Axial head slice: skull ring, brain, lateral ventricles, midline.
        let rotated = |cx: f32, cy: f32, rx: f32, ry: f32, deg: f32| {
            let (s, c) = deg.to_radians().sin_cos();
            let (dx, dy) = (p.0 - cx, p.1 - cy);
            let (u, v) = ((dx * c + dy * s) / rx, (-dx * s + dy * c) / ry);
            (u.hypot(v) - 1.0) * rx.min(ry)
        };
        over([0.90, 0.93, 0.97], coverage(ellipse(512.0, 522.0, 262.0, 320.0)));
        let g = ((p.1 - 230.0) / 600.0).clamp(0.0, 1.0);
        over([0.52 - 0.16 * g, 0.68 - 0.18 * g, 0.88 - 0.14 * g], coverage(ellipse(512.0, 522.0, 236.0, 294.0)));
        let dark = [0.075, 0.125, 0.227];
        over(dark, coverage(rotated(466.0, 505.0, 30.0, 96.0, 15.0)));
        over(dark, coverage(rotated(558.0, 505.0, 30.0, 96.0, -15.0)));
        let fissure =
            |y0: f32, y1: f32| ((p.0 - 512.0).abs() - 4.0).max((p.1 - (y0 + y1) / 2.0).abs() - (y1 - y0) / 2.0);
        over(dark, coverage(fissure(236.0, 372.0)) * 0.8);
        over(dark, coverage(fissure(668.0, 812.0)) * 0.8);
        // Red scan line (axial plane colour), extending past the head.
        let d_line = ((p.1 - 522.0).abs() - 7.0).max((p.0 - 512.0).abs() - 352.0);
        over([0.949, 0.361, 0.329], coverage(d_line) * 0.95);
        // Keep everything inside the rounded square.
        let mask = coverage(d_sq);
        *px = Rgba([
            (c[0] / c[3].max(1e-6) * 255.0).round() as u8,
            (c[1] / c[3].max(1e-6) * 255.0).round() as u8,
            (c[2] / c[3].max(1e-6) * 255.0).round() as u8,
            (c[3] * mask * 255.0).round() as u8,
        ]);
    }
    img
}

fn icons() -> Result<()> {
    let dir = root().join("packaging/icons");
    fs::create_dir_all(&dir)?;
    for size in [16u32, 32, 48, 64, 128, 256, 512, 1024] {
        render_icon(size).save(dir.join(format!("icon-{size}.png")))?;
    }
    // Windows .ico
    let ico_dir = root().join("packaging/windows");
    fs::create_dir_all(&ico_dir)?;
    let frames: Vec<image::codecs::ico::IcoFrame<'_>> = [16u32, 24, 32, 48, 64, 256]
        .iter()
        .map(|&s| {
            let img = render_icon(s);
            image::codecs::ico::IcoFrame::as_png(img.as_raw(), s, s, image::ExtendedColorType::Rgba8)
        })
        .collect::<Result<_, _>>()?;
    image::codecs::ico::IcoEncoder::new(fs::File::create(ico_dir.join("summrise.ico"))?).encode_images(&frames)?;
    // macOS .icns via iconutil
    if cfg!(target_os = "macos") {
        let iconset = std::env::temp_dir().join("SumMRIse-AppIcon.iconset");
        let _ = fs::remove_dir_all(&iconset);
        fs::create_dir_all(&iconset)?;
        for (base, scale) in
            [(16u32, 1u32), (16, 2), (32, 1), (32, 2), (128, 1), (128, 2), (256, 1), (256, 2), (512, 1), (512, 2)]
        {
            let name =
                if scale == 1 { format!("icon_{base}x{base}.png") } else { format!("icon_{base}x{base}@2x.png") };
            render_icon(base * scale).save(iconset.join(name))?;
        }
        let icns = root().join("packaging/macos/AppIcon.icns");
        run(Command::new("iconutil").args(["-c", "icns"]).arg(&iconset).arg("-o").arg(&icns))?;
        fs::remove_dir_all(&iconset)?;
    }
    println!("icons written to packaging/");
    Ok(())
}

// ---------------------------------------------------------------------------
// macOS bundle

fn bundle_macos(args: &[String]) -> Result<()> {
    ensure!(cfg!(target_os = "macos"), "bundle-macos must run on macOS (it uses lipo, codesign and hdiutil)");
    let root = root();
    let version = version()?;
    let universal = flag(args, "--universal");
    let identity = option(args, "--sign").unwrap_or("-");
    let host_arch = if cfg!(target_arch = "aarch64") { "aarch64" } else { "x86_64" };
    let targets: Vec<String> = if universal {
        vec!["aarch64-apple-darwin".into(), "x86_64-apple-darwin".into()]
    } else {
        vec![format!("{host_arch}-apple-darwin")]
    };

    for target in &targets {
        // rust-toolchain.toml installs only the host target; add others on demand.
        if let Err(e) = run(Command::new("rustup").current_dir(&root).args(["target", "add", target])) {
            eprintln!("warning: could not add target {target} with rustup ({e}); trying the build anyway");
        }
        run(Command::new(env!("CARGO")).current_dir(&root).env("MACOSX_DEPLOYMENT_TARGET", "11.0").args([
            "build",
            "--release",
            "--locked",
            "-p",
            BIN,
            "--features",
            notices::RELEASE_FEATURES,
            "--target",
            target,
        ]))?;
    }

    let dist = root.join("dist/macos");
    let app = dist.join(format!("{APP_NAME}.app"));
    let _ = fs::remove_dir_all(&app);
    let contents = app.join("Contents");
    fs::create_dir_all(contents.join("MacOS"))?;
    fs::create_dir_all(contents.join("Resources"))?;

    let exe = contents.join("MacOS").join(BIN);
    let built: Vec<PathBuf> = targets.iter().map(|t| root.join("target").join(t).join("release").join(BIN)).collect();
    if built.len() > 1 {
        run(Command::new("lipo").arg("-create").args(&built).arg("-output").arg(&exe))?;
    } else {
        fs::copy(&built[0], &exe)?;
    }

    let icns = root.join("packaging/macos/AppIcon.icns");
    ensure!(icns.exists(), "missing {} — run `cargo xtask icons` first", icns.display());
    fs::copy(&icns, contents.join("Resources/AppIcon.icns"))?;
    let plist = fs::read_to_string(root.join("packaging/macos/Info.plist"))?
        .replace("${VERSION}", &version)
        .replace("${BUNDLE_ID}", BUNDLE_ID)
        .replace("${EXECUTABLE}", BIN)
        .replace("${NAME}", APP_NAME);
    fs::write(contents.join("Info.plist"), plist)?;
    fs::write(contents.join("PkgInfo"), "APPL????")?;
    for (from, name) in package::licence_files(&root) {
        fs::copy(&from, contents.join("Resources").join(name))?;
    }

    let mut sign = Command::new("codesign");
    sign.args(["--force", "--options", "runtime", "--entitlements"])
        .arg(root.join("packaging/macos/entitlements.plist"))
        .args(["--sign", identity]);
    if identity != "-" {
        sign.arg("--timestamp");
    }
    run(sign.arg(&app))?;
    run(Command::new("codesign").args(["--verify", "--strict", "--verbose=2"]).arg(&app))?;
    println!("built {}", app.display());

    let notary_key = option(args, "--notary-key");
    if flag(args, "--dmg") || option(args, "--notarize").is_some() || notary_key.is_some() {
        let arch = if universal { "universal" } else { host_arch };
        let dmg = dist.join(format!("{APP_NAME}-{version}-macos-{arch}.dmg"));
        let staging = dist.join("dmg-staging");
        let _ = fs::remove_dir_all(&staging);
        fs::create_dir_all(&staging)?;
        run(Command::new("ditto").arg(&app).arg(staging.join(format!("{APP_NAME}.app"))))?;
        // The licence and the notices also next to the app, where they can be read.
        for (from, name) in package::licence_files(&root) {
            fs::copy(&from, staging.join(name))?;
        }
        // bundle-macos only runs on macOS (checked above); the cfg keeps the
        // workspace compiling on Windows.
        #[cfg(unix)]
        std::os::unix::fs::symlink("/Applications", staging.join("Applications"))?;
        let _ = fs::remove_file(&dmg);
        // `diskutil image` replaces the deprecated `hdiutil create` on recent
        // macOS; fall back to hdiutil where it is not available yet.
        let created = run(Command::new("diskutil")
            .args(["image", "create", "from", "--format", "ULFO", "--volumeName", APP_NAME])
            .arg(&staging)
            .arg(&dmg));
        if created.is_err() {
            eprintln!("`diskutil image` unavailable, using hdiutil");
            run(Command::new("hdiutil")
                .args(["create", "-volname", APP_NAME, "-format", "ULFO", "-ov", "-srcfolder"])
                .arg(&staging)
                .arg(&dmg))?;
        }
        fs::remove_dir_all(&staging)?;
        if identity != "-" {
            run(Command::new("codesign").args(["--force", "--timestamp", "--sign", identity]).arg(&dmg))?;
        }
        let mut notarize = Command::new("xcrun");
        notarize.args(["notarytool", "submit"]).arg(&dmg).arg("--wait");
        let notarizing = if let Some(profile) = option(args, "--notarize") {
            notarize.args(["--keychain-profile", profile]);
            true
        } else if let Some(key) = notary_key {
            let key_id = option(args, "--notary-key-id").context("--notary-key needs --notary-key-id")?;
            let issuer = option(args, "--notary-issuer").context("--notary-key needs --notary-issuer")?;
            notarize.args(["--key", key, "--key-id", key_id, "--issuer", issuer]);
            true
        } else {
            false
        };
        if notarizing {
            ensure!(identity != "-", "notarization requires a Developer ID identity (--sign)");
            run(&mut notarize)?;
            run(Command::new("xcrun").args(["stapler", "staple"]).arg(&dmg))?;
            run(Command::new("xcrun").args(["stapler", "validate"]).arg(&dmg))?;
        }
        println!("built {}", dmg.display());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Fixtures & CI

fn fixtures(args: &[String]) -> Result<()> {
    let dir = args.iter().find(|a| !a.starts_with("--")).context("usage: cargo xtask fixtures <dir> [--small]")?;
    let opts = if flag(args, "--small") { summrise_testkit::PhantomOptions::small() } else { Default::default() };
    let study = summrise_testkit::write_phantom_study(Path::new(dir), &opts)?;
    println!(
        "wrote {} series ({} files) to {dir}",
        study.series.len(),
        study.series.iter().map(|s| s.files.len()).sum::<usize>()
    );
    Ok(())
}

/// The checks of CI, with the features of the release packages (JPEG-LS needs
/// CMake and a C++ compiler; plain `cargo test` works without them).
fn ci() -> Result<()> {
    let cargo = env!("CARGO");
    let root = root();
    let features = format!("{BIN}/{}", notices::RELEASE_FEATURES);
    run(Command::new(cargo).current_dir(&root).args(["fmt", "--all", "--check"]))?;
    run(Command::new(cargo).current_dir(&root).args([
        "clippy",
        "--workspace",
        "--all-targets",
        "--locked",
        "--features",
        &features,
        "--",
        "-D",
        "warnings",
    ]))?;
    run(Command::new(cargo).current_dir(&root).args(["test", "--workspace", "--locked", "--features", &features]))?;
    notices::run(&root, &["--check".to_owned()])?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Translations

/// Messages listed in full per kind; longer lists are cut.
const LISTED: usize = 25;

/// Writes the template `messages.pot` and brings the PO file of every
/// language up to date with the messages of the source code (new messages
/// added untranslated, translations kept, entries of removed messages
/// dropped), then says per language what was added, removed, is
/// untranslated or wrong.
fn i18n() -> Result<()> {
    use summrise_i18n::{po, tooling};

    let root = root();
    let messages = match tooling::scan_workspace(&root) {
        Ok(messages) => messages,
        Err(errors) => {
            for error in &errors {
                eprintln!("error: {error}");
            }
            bail!("{} uses of tr!, tr_n! or tr_ctx! need fixing", errors.len());
        }
    };
    let plural = messages.iter().filter(|m| m.plural.is_some()).count();
    let context = messages.iter().filter(|m| m.context.is_some()).count();
    println!(
        "{} messages in the source code: {} singular ({context} of them with a context), {plural} with a plural",
        messages.len(),
        messages.len() - plural,
    );
    let locales = root.join("crates/i18n/locales");
    if write_if_changed(&locales.join("messages.pot"), &po::write(&tooling::template(&messages)))? {
        println!("updated crates/i18n/locales/messages.pot");
    }
    let mut wrong = 0;
    for language in summrise_i18n::translated_languages() {
        let path = locales.join(format!("{}.po", language.code));
        let text = fs::read_to_string(&path).unwrap_or_default();
        let existing = po::parse(&text).with_context(|| format!("{} cannot be read", path.display()))?;
        let before = tooling::check(&messages, &existing, language);
        let updated = tooling::update(&messages, &existing, language);
        let after = tooling::check(&messages, &updated, language);
        let changed = write_if_changed(&path, &po::write(&updated))?;
        let translated = after.entries - after.untranslated.len();
        println!(
            "{} ({}): {translated} of {} translated, {} untranslated; {} added, {} removed{}",
            language.code,
            language.english_name,
            after.entries,
            after.untranslated.len(),
            before.missing.len(),
            before.obsolete.len(),
            if changed { "; file updated" } else { "" },
        );
        list("added", &before.missing);
        list("removed (no longer in the source code)", &before.obsolete);
        list("was wrong, translation dropped", &before.errors);
        list("error", &after.errors);
        // The list of what is left is for a translation in progress.
        if translated > 0 {
            list("untranslated", &after.untranslated);
        }
        wrong += after.errors.len();
    }
    ensure!(wrong == 0, "{wrong} entries are wrong");
    Ok(())
}

fn list(kind: &str, items: &[String]) {
    for item in items.iter().take(LISTED) {
        println!("  {kind}: {item}");
    }
    if items.len() > LISTED {
        println!("  {kind}: … and {} more", items.len() - LISTED);
    }
}

/// Writes `text` to `path` unless it holds that already; says whether it wrote.
fn write_if_changed(path: &Path, text: &str) -> Result<bool> {
    if fs::read_to_string(path).is_ok_and(|old| old == text) {
        return Ok(false);
    }
    fs::write(path, text).with_context(|| format!("cannot write {}", path.display()))?;
    Ok(true)
}
