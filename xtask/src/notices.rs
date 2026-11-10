//! Third-party notices: `cargo xtask notices [--check]`.
//!
//! Lists every crate compiled into the release application, with the licence
//! SumMRIse uses it under, followed by the licence texts that apply, each text
//! once. "Compiled into the release application" means the normal (not build,
//! not dev) dependencies of `summrise` with the release features, on any of the
//! four targets of the release packages. The list is a superset for each single
//! package, so one file serves all of them.
//!
//! Licence texts come from the published crates: the licence files at the top of
//! the crate (for a choice of licences, those of the licence chosen) and licence
//! files further down, which belong to code or fonts the crate bundles. A crate
//! that publishes no file for its licence gets the standard text from
//! `xtask/licenses/`, with a note saying so. Versions are left out on purpose: a
//! version bump changes the notices only when it changes a licence text.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;

/// The file the notices are written to, at the root of the repository.
pub(crate) const FILE: &str = "THIRD-PARTY-NOTICES.txt";

/// The targets of the release packages.
const TARGETS: &[&str] =
    &["aarch64-apple-darwin", "x86_64-apple-darwin", "x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu"];

/// The features the release packages are built with.
pub(crate) const RELEASE_FEATURES: &str = "jpeg-ls";

/// Licences in the order SumMRIse prefers them where a crate offers a choice.
const PREFERENCE: &[&str] = &[
    "MIT",
    "Apache-2.0",
    "Zlib",
    "BSD-3-Clause",
    "BSD-2-Clause",
    "ISC",
    "0BSD",
    "Unlicense",
    "CC0-1.0",
    "BSL-1.0",
    "Unicode-3.0",
    "Apache-2.0 WITH LLVM-exception",
];

/// Standard texts for crates that publish no licence file.
const STANDARD: &[(&str, &str)] = &[
    ("Apache-2.0", include_str!("../licenses/Apache-2.0.txt")),
    ("BSD-3-Clause", include_str!("../licenses/BSD-3-Clause.txt")),
    ("BSL-1.0", include_str!("../licenses/BSL-1.0.txt")),
    ("CC0-1.0", include_str!("../licenses/CC0-1.0.txt")),
    ("IJG", include_str!("../licenses/IJG.txt")),
    ("MIT", include_str!("../licenses/MIT.txt")),
    ("OFL-1.1", include_str!("../licenses/OFL-1.1.txt")),
    ("Zlib", include_str!("../licenses/Zlib.txt")),
];

/// What a crate contains that its own licence does not say.
struct Clarification {
    krate: &'static str,
    /// Said in the list, under the crate.
    note: &'static str,
    /// The licences of what it contains, named in the list after its own.
    contains: &'static [&'static str],
    /// Those of them whose standard text is given, because the crate publishes none.
    standard_texts: &'static [&'static str],
}

const CLARIFICATIONS: &[Clarification] = &[
    Clarification {
        krate: "charls-sys",
        note: "builds CharLS, the C++ JPEG-LS library by Jan de Vaan and Victor Derks, into the \
               application; CharLS is licensed under the BSD 3-Clause licence (charls/LICENSE.md)",
        contains: &["BSD-3-Clause"],
        standard_texts: &[],
    },
    Clarification {
        krate: "egui_material_icons",
        note: "contains the Material Symbols font by Google, licensed under the Apache License 2.0",
        contains: &["Apache-2.0"],
        standard_texts: &["Apache-2.0"],
    },
    Clarification {
        krate: "epaint_default_fonts",
        note: "contains the fonts Hack, Noto Emoji, Ubuntu Light and emoji-icon-font, whose licences are in \
               its folder fonts/",
        contains: &[],
        standard_texts: &[],
    },
    Clarification {
        krate: "jpeg-encoder",
        note: "contains code ported from the IJG JPEG library (src/fdct.rs): this software is based in part on \
               the work of the Independent JPEG Group",
        contains: &[],
        standard_texts: &[],
    },
    Clarification {
        krate: "sctk-adwaita",
        note: "contains the font Cantarell (Copyright (c) 2009-2011 Understanding Limited, Copyright (c) \
               2010-2011 Jakub Steiner), licensed under the SIL Open Font License 1.1; it draws the title \
               bars of windows on Wayland (Linux)",
        contains: &["OFL-1.1"],
        standard_texts: &["OFL-1.1"],
    },
    Clarification {
        krate: "wayland-protocols-plasma",
        note: "contains the Wayland protocol definitions of KDE Plasma, from which code is generated for \
               Linux; some of them, among them the one winit uses for blurred window backgrounds (blur.xml, \
               Copyright 2015 Martin Gräßlin and Marco Martin), are licensed under LGPL-2.1-or-later, whose \
               text is given; the source code of SumMRIse is public, so the application can be rebuilt with \
               a changed version of them",
        contains: &["LGPL-2.1-or-later"],
        standard_texts: &[],
    },
];

/// Files in a crate other than at its top that hold licences of code or fonts it
/// bundles, besides those whose names say "licence" or "copying".
const BUNDLED_EXTRA: &[(&str, &str)] =
    &[("epaint_default_fonts", "fonts/Hack-Regular.txt"), ("epaint_default_fonts", "fonts/egui-icons.txt")];

/// Folders whose licence files are not about anything compiled in.
const SKIPPED_FOLDERS: &[&str] = &[
    "tests",
    "test",
    "testdata",
    "test-data",
    "test_data",
    "examples",
    "example",
    "benches",
    "bench",
    "fuzz",
    "images",
    "target",
    ".github",
];

/// One crate of the release application (all its versions together).
#[derive(Debug, Default)]
struct Crate {
    /// The licence expressions of its versions.
    expressions: BTreeSet<String>,
    authors: BTreeSet<String>,
    /// The folders of its versions.
    dirs: BTreeSet<PathBuf>,
}

/// One line of the list: a component, the licences it is used under and of
/// what it contains, the texts that apply and what else there is to say.
#[derive(Debug)]
struct Component {
    name: String,
    licences: Vec<String>,
    texts: Vec<Text>,
    note: Option<String>,
}

/// A licence text and the licences found in it.
#[derive(Debug, Clone)]
struct Text {
    licences: BTreeSet<&'static str>,
    body: String,
}

/// Writes the notices, or with `--check` fails when the file differs from what
/// would be written.
pub(crate) fn run(root: &Path, args: &[String]) -> Result<()> {
    let notices = generate(root)?;
    let path = root.join(FILE);
    if args.iter().any(|a| a == "--check") {
        let on_disk = fs::read_to_string(&path).unwrap_or_default();
        ensure!(on_disk == notices, "{FILE} is out of date: run `cargo xtask notices` and commit the file");
        println!("{FILE} is up to date");
    } else {
        fs::write(&path, &notices).with_context(|| format!("cannot write {}", path.display()))?;
        println!("wrote {FILE} ({} KB)", notices.len() / 1024);
    }
    Ok(())
}

fn generate(root: &Path) -> Result<String> {
    let crates = release_crates(root)?;
    let mut texts: Vec<Text> = Vec::new();
    let mut users: Vec<BTreeSet<String>> = Vec::new();
    let mut list = String::new();
    // The standard library first: it is in every program, and in no Cargo.lock.
    let mut components = vec![standard_library()?];
    for (name, krate) in &crates {
        components.push(texts_of(name, krate)?);
    }
    let count = components.len();
    for Component { name, licences, texts: mut found, note } in components {
        let mut numbers = BTreeSet::new();
        for text in found.drain(..) {
            let key = collapse(&text.body);
            let index = match texts.iter().position(|t| collapse(&t.body) == key) {
                Some(index) => index,
                None => {
                    texts.push(text);
                    users.push(BTreeSet::new());
                    texts.len() - 1
                }
            };
            users[index].insert(name.clone());
            numbers.insert(index + 1);
        }
        let numbers: Vec<String> = numbers.iter().map(|n| format!("[{n}]")).collect();
        list.push_str(&format!("{name} — {} — {}\n", licences.join(" AND "), numbers.join(" ")));
        if let Some(note) = note {
            list.push_str(&wrap(&format!("Note: {note}."), 4));
        }
    }

    let mut out = String::new();
    out.push_str("SumMRIse third-party notices\n============================\n\n");
    out.push_str(&wrap(
        "SumMRIse is licensed under the MIT licence (the file LICENSE). Its release packages for macOS, \
         Windows and Linux contain the third-party software listed below. Each component is named with \
         the licence under which SumMRIse uses it (where a component offers a choice of licences, the one \
         chosen, followed by the licences of what it contains) and the numbers of the licence texts that \
         apply to it, which follow the list. Copyright belongs to the authors named in those texts and in \
         each component.",
        0,
    ));
    out.push('\n');
    // The licence of the IJG code in jpeg-encoder asks for this statement.
    out.push_str("This software is based in part on the work of the Independent JPEG Group.\n\n");
    out.push_str(&wrap(
        "The Linux AppImage begins with the AppImage type 2 runtime (github.com/AppImage/type2-runtime, \
         Copyright (c) 2004-23 probonopd, MIT licence), a separate program that starts SumMRIse; it is not \
         part of SumMRIse and comes with its own licences.",
        0,
    ));
    out.push('\n');
    out.push_str(&wrap("This file is generated by `cargo xtask notices` from Cargo.lock; do not edit it by hand.", 0));
    out.push_str(&format!("\n\nComponents ({count})\n{}\n\n", "-".repeat(13 + count.to_string().len())));
    out.push_str(&list);
    out.push_str("\n\nLicence texts\n-------------\n");
    for (index, (text, users)) in texts.iter().zip(&users).enumerate() {
        let kind = if text.licences.is_empty() {
            "Notice".to_owned()
        } else {
            text.licences.iter().copied().collect::<Vec<_>>().join(", ")
        };
        out.push_str(&format!("\n[{}] {kind}\n", index + 1));
        out.push_str(&wrap(&format!("Used by: {}.", users.iter().cloned().collect::<Vec<_>>().join(", ")), 0));
        out.push_str(&format!("{}\n\n{}\n", "-".repeat(78), text.body));
    }
    Ok(out)
}

/// The Rust standard library, which is linked into the application and is in no
/// Cargo.lock: its licence and where its own notices are.
fn standard_library() -> Result<Component> {
    let mit = STANDARD.iter().find(|(id, _)| *id == "MIT").context("no standard text for MIT")?;
    let note = "linked into every program written in Rust; copyright belongs to its contributors (the Rust \
                Project contributors); it is offered under the MIT licence or the Apache License 2.0 and is \
                used here under MIT, whose standard text is given; the parts it takes from other projects \
                (under MIT, Apache-2.0, in part with the LLVM exception, BSD-2-Clause, ISC, Zlib, Unlicense, \
                BSL-1.0 and Unicode-3.0 terms) are listed with their copyright notices in the file \
                share/doc/rust/COPYRIGHT-library.html of the Rust toolchain the packages are built with";
    Ok(Component {
        name: "Rust standard library (std, core, alloc)".to_owned(),
        licences: vec!["MIT".to_owned()],
        texts: vec![Text { licences: BTreeSet::from([mit.0]), body: normalise(mit.1) }],
        note: Some(note.to_owned()),
    })
}

/// The crates of the release application by name: the normal dependencies of
/// `summrise` on each release target, with the release features.
fn release_crates(root: &Path) -> Result<BTreeMap<String, Crate>> {
    let mut crates: BTreeMap<String, Crate> = BTreeMap::new();
    for target in TARGETS {
        let output = Command::new(env!("CARGO"))
            .current_dir(root)
            .args(["metadata", "--format-version", "1", "--locked", "--filter-platform", target, "--features"])
            .arg(format!("summrise/{RELEASE_FEATURES}"))
            .output()
            .context("cannot run cargo metadata")?;
        ensure!(output.status.success(), "cargo metadata failed: {}", String::from_utf8_lossy(&output.stderr));
        let metadata: Value = serde_json::from_slice(&output.stdout).context("cargo metadata output")?;
        let packages: BTreeMap<&str, &Value> =
            array(&metadata["packages"])?.iter().filter_map(|p| Some((p["id"].as_str()?, p))).collect();
        let nodes: BTreeMap<&str, &Value> =
            array(&metadata["resolve"]["nodes"])?.iter().filter_map(|n| Some((n["id"].as_str()?, n))).collect();
        let start = packages
            .values()
            .find(|p| p["name"] == "summrise" && p["source"].is_null())
            .and_then(|p| p["id"].as_str())
            .context("the package summrise is not in the workspace")?;
        let mut seen = BTreeSet::new();
        let mut stack = vec![start];
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            let node = nodes.get(id).with_context(|| format!("{id} is not resolved"))?;
            for dep in array(&node["deps"])? {
                let normal = array(&dep["dep_kinds"])?.iter().any(|kind| kind["kind"].is_null());
                if normal && let Some(pkg) = dep["pkg"].as_str() {
                    stack.push(pkg);
                }
            }
        }
        for id in seen {
            let package = packages.get(id).with_context(|| format!("{id} has no package"))?;
            // The crates of this repository are SumMRIse itself.
            if package["source"].is_null() {
                continue;
            }
            let name = package["name"].as_str().context("package name")?.to_owned();
            let manifest = PathBuf::from(package["manifest_path"].as_str().context("manifest path")?);
            let krate = crates.entry(name.clone()).or_default();
            let expression = package["license"].as_str().with_context(|| format!("{name} declares no licence"))?;
            krate.expressions.insert(expression.to_owned());
            for author in package["authors"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                // Names only: the addresses are not needed to credit anyone.
                let name = author.split('<').next().unwrap_or(author).trim();
                if !name.is_empty() {
                    krate.authors.insert(name.to_owned());
                }
            }
            krate.dirs.insert(manifest.parent().context("crate folder")?.to_path_buf());
        }
    }
    Ok(crates)
}

fn array(value: &Value) -> Result<&Vec<Value>> {
    value.as_array().context("unexpected cargo metadata output")
}

/// The licences SumMRIse uses a crate under, the texts that apply and a note.
fn texts_of(name: &str, krate: &Crate) -> Result<Component> {
    let mut top: Vec<(String, Text)> = Vec::new();
    let mut nested: Vec<(String, Text)> = Vec::new();
    for dir in &krate.dirs {
        for path in licence_files(dir, name)? {
            let relative = path.strip_prefix(dir).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            let body = normalise(&String::from_utf8_lossy(&fs::read(&path)?));
            if body.is_empty() {
                continue;
            }
            let text = Text { licences: classify(&relative, &body), body };
            // A REUSE folder `LICENSES/` holds the crate's own licences, like files at its top.
            let own = !relative.contains('/') || relative.to_lowercase().starts_with("licenses/");
            if own { top.push((relative, text)) } else { nested.push((relative, text)) }
        }
    }
    let with_file: BTreeSet<&str> = top.iter().flat_map(|(_, t)| t.licences.iter().copied()).collect();
    let mut chosen: Vec<String> = Vec::new();
    for expression in &krate.expressions {
        for licence in choose(&parse(expression)?, &with_file) {
            if !chosen.contains(&licence) {
                chosen.push(licence);
            }
        }
    }
    let clarification = CLARIFICATIONS.iter().find(|c| c.krate == name);
    let mut texts: Vec<Text> = Vec::new();
    // Top-level files of the licences chosen, and those that name no licence (notices).
    for (_, text) in &top {
        if text.licences.is_empty() || text.licences.iter().any(|l| chosen.iter().any(|c| c == l)) {
            texts.push(text.clone());
        }
    }
    // A licence is covered by a file of the chosen ones or by one further down
    // (the fonts of a crate keep their licences in their folder).
    let mut missing: Vec<&str> = Vec::new();
    for licence in &chosen {
        let covered = texts.iter().chain(nested.iter().map(|(_, t)| t)).any(|t| t.licences.contains(licence.as_str()));
        if !covered {
            missing.push(licence);
        }
    }
    let extra = clarification.map_or(&[][..], |c| c.standard_texts);
    let mut notes: Vec<String> = clarification.iter().map(|c| c.note.to_owned()).collect();
    if !missing.is_empty() {
        let authors = if krate.authors.is_empty() {
            String::new()
        } else {
            format!("; its authors are {}", krate.authors.iter().cloned().collect::<Vec<_>>().join(", "))
        };
        notes.push(format!(
            "the published crate contains no licence file for {}, so the standard text is given{authors}",
            missing.join(" and ")
        ));
    }
    for licence in missing.into_iter().chain(extra.iter().copied()) {
        let standard = STANDARD
            .iter()
            .find(|(id, _)| *id == licence)
            .with_context(|| format!("{name}: no standard text for {licence} in xtask/licenses (see its README)"))?;
        texts.push(Text { licences: BTreeSet::from([standard.0]), body: normalise(standard.1) });
    }
    texts.extend(nested.into_iter().map(|(_, text)| text));
    // What the crate contains is named after its own licence.
    for licence in clarification.map_or(&[][..], |c| c.contains) {
        if !chosen.iter().any(|c| c == licence) {
            chosen.push((*licence).to_owned());
        }
    }
    let note = (!notes.is_empty()).then(|| notes.join("; "));
    Ok(Component { name: name.to_owned(), licences: chosen, texts, note })
}

/// The licence files of a crate: at its top, files whose names begin with
/// "licence", "copying", "copyright", "notice" or "unlicense", and the files
/// of a folder `LICENSES`; further down, files whose names mention a licence,
/// outside tests, examples and the like.
fn licence_files(dir: &Path, name: &str) -> Result<Vec<PathBuf>> {
    fn walk(dir: &Path, depth: usize, reuse: bool, out: &mut Vec<PathBuf>) -> Result<()> {
        let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<Result<_, _>>()?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            let file_name = entry.file_name().to_string_lossy().to_lowercase();
            if entry.file_type()?.is_dir() {
                if depth < 4 && !SKIPPED_FOLDERS.contains(&file_name.as_str()) {
                    // `LICENSES/` at the top of a crate (the REUSE layout): every file in it.
                    walk(&path, depth + 1, depth == 0 && file_name == "licenses", out)?;
                }
                continue;
            }
            let top = depth == 0
                && ["licen", "copying", "copyright", "notice", "unlicense"].iter().any(|p| file_name.starts_with(p));
            let nested = depth > 0
                && (file_name.contains("licen") || file_name.starts_with("copying"))
                && !file_name.ends_with(".rs");
            let nested = nested || (depth > 0 && matches!(file_name.as_str(), "ofl.txt" | "ufl.txt"));
            if top || nested || reuse {
                out.push(path);
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(dir, 0, false, &mut files)?;
    for (crate_name, relative) in BUNDLED_EXTRA {
        let path = dir.join(relative);
        if *crate_name == name && path.is_file() && !files.contains(&path) {
            files.push(path);
        }
    }
    Ok(files)
}

/// The licences a file holds, from its name and its text.
fn classify(relative: &str, body: &str) -> BTreeSet<&'static str> {
    let mut found = BTreeSet::new();
    let file_name = relative.rsplit('/').next().unwrap_or(relative).to_lowercase();
    let named = [
        ("mit", "MIT"),
        ("apache", "Apache-2.0"),
        ("zlib", "Zlib"),
        ("unlicense", "Unlicense"),
        ("boost", "BSL-1.0"),
        ("bsl", "BSL-1.0"),
        ("cc0", "CC0-1.0"),
        ("unicode", "Unicode-3.0"),
        ("ijg", "IJG"),
    ];
    for (part, licence) in named {
        if file_name.contains(part) {
            found.insert(licence);
        }
    }
    let collapsed = collapse(body);
    let has = |words: &str| collapsed.contains(words);
    // The words of the licence itself: the font and Unicode licences begin like MIT
    // ("…obtaining a copy of the Font Software"), and a file that only points to the
    // Apache License ("Licensed under the Apache License, Version 2.0 or the MIT
    // license, at your option") is a notice, with copyright lines to keep.
    if has("to any person obtaining a copy of this software and associated documentation files")
        || has("to any person obtaining a copy of this software and/or associated documentation files")
    {
        found.insert("MIT");
    }
    if has("TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION") {
        found.insert(if has("LLVM Exceptions") { "Apache-2.0 WITH LLVM-exception" } else { "Apache-2.0" });
    }
    if has("GNU GENERAL PUBLIC LICENSE Version 2, June 1991") {
        found.insert("GPL-2.0-only");
    }
    if has("GNU LESSER GENERAL PUBLIC LICENSE Version 2.1, February 1999") {
        found.insert("LGPL-2.1-or-later");
    }
    if has("Boost Software License") {
        found.insert("BSL-1.0");
    }
    if has("This is free and unencumbered software released into the public domain") {
        found.insert("Unlicense");
    }
    if has("This software is provided 'as-is', without any express or implied warranty") {
        found.insert("Zlib");
    }
    if has("Redistribution and use in source and binary forms") {
        found.insert(if has("Neither the name") { "BSD-3-Clause" } else { "BSD-2-Clause" });
    }
    if has("Permission to use, copy, modify, and/or distribute this software for any purpose with or without fee") {
        found.insert(if has("provided that the above copyright notice") { "ISC" } else { "0BSD" });
    }
    if has("CC0 1.0 Universal") {
        found.insert("CC0-1.0");
    }
    if has("UNICODE LICENSE V3") {
        found.insert("Unicode-3.0");
    }
    if has("the Independent JPEG Group") {
        found.insert("IJG");
    }
    if has("SIL OPEN FONT LICENSE Version 1.1") {
        found.insert("OFL-1.1");
    }
    if has("UBUNTU FONT LICENCE Version 1.0") {
        found.insert("Ubuntu-font-1.0");
    }
    // A file named for one licence that quotes another stays the file of the first.
    if file_name.contains("mit") && found.len() > 1 && !found.contains("Apache-2.0") {
        found.retain(|l| *l == "MIT");
    }
    found
}

/// An SPDX licence expression.
#[derive(Debug, PartialEq)]
enum Expression {
    Licence(String),
    All(Vec<Expression>),
    Any(Vec<Expression>),
}

/// Parses an SPDX expression; `/` (an old way of writing OR) is understood.
fn parse(text: &str) -> Result<Expression> {
    let spaced = text.replace('/', " OR ").replace('(', " ( ").replace(')', " ) ");
    let mut tokens: Vec<String> = Vec::new();
    let mut words = spaced.split_whitespace().peekable();
    while let Some(word) = words.next() {
        // "Apache-2.0 WITH LLVM-exception" is one licence.
        if words.peek() == Some(&"WITH") {
            words.next();
            let exception = words.next().with_context(|| format!("{text}: WITH without an exception"))?;
            tokens.push(format!("{word} WITH {exception}"));
        } else {
            tokens.push(word.to_owned());
        }
    }
    let mut position = 0;
    let expression = parse_any(&tokens, &mut position, text)?;
    ensure!(position == tokens.len(), "{text}: unexpected {:?}", tokens.get(position));
    Ok(expression)
}

fn parse_any(tokens: &[String], position: &mut usize, text: &str) -> Result<Expression> {
    let mut options = vec![parse_all(tokens, position, text)?];
    while tokens.get(*position).is_some_and(|t| t == "OR") {
        *position += 1;
        options.push(parse_all(tokens, position, text)?);
    }
    Ok(if options.len() == 1 { options.remove(0) } else { Expression::Any(options) })
}

fn parse_all(tokens: &[String], position: &mut usize, text: &str) -> Result<Expression> {
    let mut parts = vec![parse_one(tokens, position, text)?];
    while tokens.get(*position).is_some_and(|t| t == "AND") {
        *position += 1;
        parts.push(parse_one(tokens, position, text)?);
    }
    Ok(if parts.len() == 1 { parts.remove(0) } else { Expression::All(parts) })
}

fn parse_one(tokens: &[String], position: &mut usize, text: &str) -> Result<Expression> {
    let token = tokens.get(*position).with_context(|| format!("{text}: incomplete expression"))?;
    *position += 1;
    match token.as_str() {
        "(" => {
            let inner = parse_any(tokens, position, text)?;
            ensure!(tokens.get(*position).is_some_and(|t| t == ")"), "{text}: missing )");
            *position += 1;
            Ok(inner)
        }
        "OR" | "AND" | ")" => bail!("{text}: unexpected {token}"),
        _ => Ok(Expression::Licence(token.clone())),
    }
}

/// The licences to use a crate under: all parts of AND; of OR, the option whose
/// texts the crate publishes, then the one SumMRIse prefers.
fn choose(expression: &Expression, with_file: &BTreeSet<&str>) -> Vec<String> {
    match expression {
        Expression::Licence(licence) => vec![licence.clone()],
        Expression::All(parts) => parts.iter().flat_map(|p| choose(p, with_file)).collect(),
        Expression::Any(options) => {
            let rank = |licence: &String| PREFERENCE.iter().position(|p| p == licence).unwrap_or(PREFERENCE.len());
            options
                .iter()
                .map(|option| choose(option, with_file))
                .min_by_key(|licences| {
                    let unpublished = licences.iter().filter(|l| !with_file.contains(l.as_str())).count();
                    (unpublished, licences.iter().map(rank).max().unwrap_or(0))
                })
                .unwrap_or_default()
        }
    }
}

/// Line ends as LF, no trailing spaces, no blank lines around the text.
fn normalise(text: &str) -> String {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    let start = lines.iter().position(|l| !l.is_empty()).unwrap_or(lines.len());
    let end = lines.iter().rposition(|l| !l.is_empty()).map_or(start, |i| i + 1);
    lines[start..end].join("\n")
}

/// The words of a text, for comparing texts that differ only in spacing.
fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Wraps `text` at 100 columns, indenting every line by `indent` spaces.
fn wrap(text: &str, indent: usize) -> String {
    let mut out = String::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && indent + line.chars().count() + 1 + word.chars().count() > 100 {
            out.push_str(&format!("{}{line}\n", " ".repeat(indent)));
            line.clear();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        out.push_str(&format!("{}{line}\n", " ".repeat(indent)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn licences(expression: &str, with_file: &[&str]) -> Vec<String> {
        choose(&parse(expression).expect("parses"), &with_file.iter().copied().collect())
    }

    #[test]
    fn expressions_are_parsed() {
        assert_eq!(parse("MIT").expect("parses"), Expression::Licence("MIT".into()));
        assert_eq!(
            parse("MIT/Apache-2.0").expect("parses"),
            Expression::Any(vec![Expression::Licence("MIT".into()), Expression::Licence("Apache-2.0".into())])
        );
        assert!(matches!(parse("(MIT OR Apache-2.0) AND Unicode-3.0").expect("parses"), Expression::All(_)));
        assert!(parse("MIT OR").is_err());
        assert!(parse("(MIT").is_err());
    }

    #[test]
    fn a_choice_falls_on_a_published_text_then_on_the_preferred_licence() {
        assert_eq!(licences("MIT OR Apache-2.0", &[]), ["MIT"]);
        assert_eq!(licences("Apache-2.0 OR MIT", &["MIT", "Apache-2.0"]), ["MIT"]);
        assert_eq!(licences("MIT OR Apache-2.0", &["Apache-2.0"]), ["Apache-2.0"]);
        assert_eq!(licences("MIT OR Apache-2.0 OR LGPL-2.1-or-later", &[]), ["MIT"]);
        assert_eq!(licences("Apache-2.0 OR GPL-2.0-only", &[]), ["Apache-2.0"]);
        assert_eq!(licences("(MIT OR Apache-2.0) AND Unicode-3.0", &["Unicode-3.0"]), ["MIT", "Unicode-3.0"]);
        assert_eq!(licences("Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT", &["Apache-2.0", "MIT"]), ["MIT"]);
    }

    #[test]
    fn files_are_recognised_by_name_and_text() {
        let mit = include_str!("../licenses/MIT.txt");
        let apache = include_str!("../licenses/Apache-2.0.txt");
        assert_eq!(classify("LICENSE-MIT", mit), BTreeSet::from(["MIT"]));
        assert_eq!(classify("LICENSE", apache), BTreeSet::from(["Apache-2.0"]));
        assert_eq!(classify("COPYING", include_str!("../licenses/Zlib.txt")), BTreeSet::from(["Zlib"]));
        assert_eq!(classify("LICENSE", include_str!("../licenses/BSD-3-Clause.txt")), BTreeSet::from(["BSD-3-Clause"]));
        assert_eq!(classify("LICENSE", &format!("{mit}\n\n{apache}")), BTreeSet::from(["MIT", "Apache-2.0"]));
        assert!(classify("NOTICE", "This product includes software developed by someone.").is_empty());
    }

    /// A file that only points to its licences is a notice: it is kept whatever
    /// licence is chosen, with the copyright lines it holds.
    #[test]
    fn a_file_that_points_to_licences_is_a_notice() {
        let pointer = "Copyright 2016-2026 Frank Denis.\n\nLicensed under the Apache License, Version 2.0 \
                       <LICENSE-APACHE or http://www.apache.org/licenses/LICENSE-2.0> or the MIT license \
                       <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your option.";
        assert!(classify("COPYING", pointer).is_empty());
        let header = "Licensed under the Apache License, Version 2.0 (the \"License\"); you may not use this \
                      file except in compliance with the License.";
        assert!(classify("COPYRIGHT", header).is_empty());
    }

    /// The font and Unicode licences begin like MIT; they are not MIT.
    #[test]
    fn licences_that_begin_like_mit_are_not_mit() {
        assert_eq!(classify("fonts/OFL.txt", include_str!("../licenses/OFL-1.1.txt")), BTreeSet::from(["OFL-1.1"]));
        let unicode = "UNICODE LICENSE V3\n\nPermission is hereby granted, free of charge, to any person obtaining \
                       a copy of data files and any associated documentation (the \"Data Files\") or software";
        assert_eq!(classify("LICENSE", unicode), BTreeSet::from(["Unicode-3.0"]));
        let variant = "Permission is hereby granted, free of charge, to any person obtaining a copy of this \
                       software and/or associated documentation files (the \"Materials\")";
        assert_eq!(classify("LICENSE", variant), BTreeSet::from(["MIT"]));
    }

    /// The GNU licences are recognised, so that one a crate only offers as an
    /// alternative is left out and one that applies is named.
    #[test]
    fn the_gnu_licences_are_recognised() {
        let gpl = "GNU GENERAL PUBLIC LICENSE\n   Version 2, June 1991\n\nCopyright (C) 1989, 1991 Free Software";
        assert_eq!(classify("LICENSE-GPL", gpl), BTreeSet::from(["GPL-2.0-only"]));
        let lgpl = "GNU LESSER GENERAL PUBLIC LICENSE\n   Version 2.1, February 1999\n\n[This is the first \
                    released version of the Lesser GPL.]";
        assert_eq!(classify("protocols/COPYING.LIB", lgpl), BTreeSet::from(["LGPL-2.1-or-later"]));
        // Offered as an alternative and not chosen: its text is not among the crate's texts.
        assert_eq!(licences("Apache-2.0 OR GPL-2.0-only", &["Apache-2.0", "GPL-2.0-only"]), ["Apache-2.0"]);
    }

    #[test]
    fn what_a_crate_contains_has_a_text_to_go_with_it() {
        for clarification in CLARIFICATIONS {
            for licence in clarification.standard_texts {
                assert!(clarification.contains.contains(licence), "{}: {licence}", clarification.krate);
                assert!(STANDARD.iter().any(|(id, _)| id == licence), "{}: {licence}", clarification.krate);
            }
        }
    }

    #[test]
    fn texts_are_normalised_and_wrapped() {
        assert_eq!(normalise("\u{feff}\r\nA  \r\nB\r\n\r\n"), "A\nB");
        assert_eq!(collapse("a\n  b\tc"), "a b c");
        let wrapped = wrap(&"word ".repeat(40), 4);
        assert!(wrapped.lines().all(|l| l.starts_with("    ") && l.chars().count() <= 100), "{wrapped}");
    }
}
