//! Builds the two things the binary embeds: the Vue dashboard in ``web/``,
//! and the compact window's layout in ``ui/``.
//!
//! The layout is written in Slint's own language (``ui/compact.slint``).
//! Slint's compiler turns it into Rust here, with the fonts it names built
//! in, and ``src/compact/mod.rs`` includes the result.
//!
//! For the dashboard, Cargo reruns this only when a source file changes, and Vite
//! writes the result to ``web/dist``, which ``src/server/http.rs`` embeds.
//! It needs Node.js 18+ with npm on ``PATH``. Set ``VIGIL_SKIP_WEB_BUILD=1``
//! to use an existing ``web/dist`` as-is (for example one built in an
//! earlier CI step).
//!
//! For Windows targets it also gives the executable the dashboard's
//! favicon as its icon, so Explorer and the taskbar show the same picture
//! as the tray, and the version details Explorer shows under Properties >
//! Details (see ``windows_resources``).

use std::path::{Path, PathBuf};
use std::process::Command;

const WATCH: &[&str] = &[
    "web/src",
    "web/public",
    "web/index.html",
    "web/package.json",
    "web/package-lock.json",
    "web/vite.config.ts",
    "web/.env",
];

fn main() {
    compact_window();

    for path in WATCH {
        println!("cargo:rerun-if-changed={path}");
    }
    println!("cargo:rerun-if-env-changed=VIGIL_SKIP_WEB_BUILD");

    // CARGO_CFG_* describe the target, which differs from the host when
    // cross-compiling.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        windows_resources();
    }

    let web = Path::new("web");
    let dist_index = web.join("dist").join("index.html");

    if std::env::var_os("VIGIL_SKIP_WEB_BUILD").is_some() {
        if !dist_index.exists() {
            fail("VIGIL_SKIP_WEB_BUILD is set but web/dist/index.html doesn't exist.");
        }
        return;
    }

    if !web.join("node_modules").exists() {
        npm(&["ci", "--no-audit", "--no-fund"]);
    }
    // ``build-only`` is ``vite build``: no type check, and it doesn't
    // rewrite src/version.ts, which would retrigger this script on every
    // cargo invocation. The footer shows the collector's version instead.
    npm(&["run", "build-only"]);

    if !dist_index.exists() {
        fail("the dashboard build finished but web/dist/index.html is missing.");
    }
}

/// Turns the compact window's layout (``ui/compact.slint``) into Rust for
/// ``src/compact/`` to include, with its fonts built into the program.
/// Done for every target, though only Windows has the window: the tests
/// draw it on all of them.
fn compact_window() {
    let config = slint_build::CompilerConfiguration::new()
        .embed_resources(slint_build::EmbedResourcesKind::EmbedFiles);
    if let Err(e) = slint_build::compile_with_config("ui/compact.slint", config) {
        eprintln!("\nerror: building the compact window: {e}\n");
        std::process::exit(1);
    }
}

fn npm(args: &[&str]) {
    // npm is a .cmd shim on Windows, which Command can't start directly.
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg("npm");
        c
    } else {
        Command::new("npm")
    };
    let status = cmd
        .args(args)
        .current_dir("web")
        .env("VITE_APP_VERSION", env!("CARGO_PKG_VERSION"))
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => fail(&format!("`npm {}` failed ({s}).", args.join(" "))),
        Err(e) => fail(&format!(
            "couldn't run npm ({e}). Install Node.js 18 or newer from https://nodejs.org \
             so npm is on PATH."
        )),
    }
}

/// The executable's icon and version details. Windows reads both from the
/// program's resources: the icon as one ``RT_ICON`` per image plus an
/// ``RT_GROUP_ICON`` directory listing them, the details as an
/// ``RT_VERSION``. This writes them straight into a compiled resource file
/// (``.res``), so no resource compiler is needed with MSVC, whose linker
/// takes ``.res`` files as they are. The GNU linker doesn't, so there
/// MinGW's ``windres`` turns it into an object file first; without
/// ``windres`` the build goes on and the program just has no icon or
/// details.
fn windows_resources() {
    const ICO: &str = "web/public/favicon.ico";
    println!("cargo:rerun-if-changed={ICO}");
    // The version details come from the package's version and authors.
    println!("cargo:rerun-if-changed=Cargo.toml");
    let ico = std::fs::read(ICO).unwrap_or_else(|e| fail(&format!("reading {ICO}: {e}")));
    let mut res =
        icon_resources(&ico).unwrap_or_else(|| fail(&format!("{ICO} isn't a valid icon file.")));
    push_resource(&mut res, RT_VERSION, 1, &version_info());

    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR is set by cargo"));
    let res_path = out.join("resources.res");
    std::fs::write(&res_path, res).unwrap_or_else(|e| fail(&format!("writing resources.res: {e}")));

    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins={}", res_path.display());
        return;
    }
    let obj = out.join("resources.o");
    let windres = windres();
    let status = Command::new(&windres)
        .arg("--input-format=res")
        .arg("--output-format=coff")
        .arg("-i")
        .arg(&res_path)
        .arg("-o")
        .arg(&obj)
        .status();
    match status {
        Ok(s) if s.success() => println!("cargo:rustc-link-arg-bins={}", obj.display()),
        _ => println!(
            "cargo:warning=couldn't run {windres}, so the program has no icon or version details"
        ),
    }
}

/// MinGW's ``windres``: prefixed with the target triple when cross-compiling.
fn windres() -> String {
    let target = std::env::var("TARGET").unwrap_or_default();
    let host = std::env::var("HOST").unwrap_or_default();
    let prefixed = match target.split('-').next() {
        Some(arch) if target != host => format!("{arch}-w64-mingw32-windres"),
        _ => "windres".into(),
    };
    if Command::new(&prefixed).arg("--version").output().is_ok() {
        prefixed
    } else {
        "windres".into()
    }
}

/// The ``.res`` file for an icon: each image as its own ``RT_ICON`` (ids 1
/// to n), then the ``RT_GROUP_ICON`` (id 1) that lists them. ``None`` if
/// the icon file is malformed.
fn icon_resources(ico: &[u8]) -> Option<Vec<u8>> {
    const RT_ICON: u16 = 3;
    const RT_GROUP_ICON: u16 = 14;
    let u16_at = |i: usize| Some(u16::from_le_bytes(ico.get(i..i + 2)?.try_into().ok()?));
    let u32_at = |i: usize| Some(u32::from_le_bytes(ico.get(i..i + 4)?.try_into().ok()?));

    if u16_at(0)? != 0 || u16_at(2)? != 1 {
        return None;
    }
    let count = u16_at(4)?;
    // A .res file starts with an empty entry that marks it as 32-bit.
    let mut res = Vec::new();
    push_resource(&mut res, 0, 0, &[]);

    let mut group = Vec::new();
    group.extend_from_slice(&[0, 0, 1, 0]);
    group.extend_from_slice(&count.to_le_bytes());
    for n in 0..count {
        let entry = 6 + 16 * usize::from(n);
        let len = u32_at(entry + 8)? as usize;
        let start = u32_at(entry + 12)? as usize;
        let image = ico.get(start..start.checked_add(len)?)?;
        let id = n + 1;
        push_resource(&mut res, RT_ICON, id, image);
        // The group entry is the icon file's entry with the image's offset
        // replaced by its resource id. Some icon files leave the plane count
        // at 0; resource compilers write 1.
        group.extend_from_slice(ico.get(entry..entry + 4)?);
        group.extend_from_slice(&u16_at(entry + 4)?.max(1).to_le_bytes());
        group.extend_from_slice(ico.get(entry + 6..entry + 12)?);
        group.extend_from_slice(&id.to_le_bytes());
    }
    push_resource(&mut res, RT_GROUP_ICON, 1, &group);
    Some(res)
}

const RT_VERSION: u16 = 16;
/// English (United Kingdom), with strings in UTF-16 (code page 1200).
const LANGUAGE: u16 = 0x0809;
const CODE_PAGE: u16 = 1200;
/// First year of the project's history, for the copyright line.
const FIRST_YEAR: u32 = 2022;

/// The ``VS_VERSIONINFO`` that Explorer shows under Properties > Details
/// and Task Manager shows as the description and publisher.
fn version_info() -> Vec<u8> {
    let env = |key: &str| std::env::var(key).unwrap_or_default();
    let version = env("CARGO_PKG_VERSION");
    // "Chris Rowles <email>" -> "Chris Rowles"; several authors are ":"-separated.
    let authors = env("CARGO_PKG_AUTHORS");
    let author = authors.split(':').next().unwrap_or_default();
    let author = author
        .split('<')
        .next()
        .unwrap_or_default()
        .trim()
        .to_string();
    let year = current_year();
    let years = if year > FIRST_YEAR {
        format!("{FIRST_YEAR}-{year}")
    } else {
        FIRST_YEAR.to_string()
    };

    let strings: [(&str, String); 9] = [
        ("CompanyName", author.clone()),
        ("FileDescription", "Vigil".into()),
        (
            "Comments",
            "A system monitoring utility with remote connection capabilities.".into(),
        ),
        ("FileVersion", version.clone()),
        ("InternalName", "vigil-collector".into()),
        (
            "LegalCopyright",
            format!("Copyright \u{a9} {years} {author}"),
        ),
        ("OriginalFilename", "vigil-collector.exe".into()),
        ("ProductName", "Vigil".into()),
        ("ProductVersion", version.clone()),
    ];
    let table = strings
        .iter()
        .map(|(key, value)| {
            let mut text = utf16(value);
            text.push(0);
            ver_block(key, (text.len()) as u16, 1, &bytes(&text), &[])
        })
        .collect::<Vec<_>>();
    let table_key = format!("{LANGUAGE:04X}{CODE_PAGE:04X}");
    let string_file_info = ver_block(
        "StringFileInfo",
        0,
        1,
        &[],
        &[ver_block(&table_key, 0, 1, &[], &table)],
    );
    let translation = [LANGUAGE.to_le_bytes(), CODE_PAGE.to_le_bytes()].concat();
    let var_file_info = ver_block(
        "VarFileInfo",
        0,
        1,
        &[],
        &[ver_block("Translation", 4, 0, &translation, &[])],
    );

    // VS_FIXEDFILEINFO: the numeric versions, as major.minor.patch.0.
    let mut parts = version
        .split(['.', '-', '+'])
        .map(|p| p.parse::<u32>().unwrap_or(0));
    let mut next = || parts.next().unwrap_or(0) & 0xFFFF;
    let (major, minor, patch) = (next(), next(), next());
    let (ms, ls) = (major << 16 | minor, patch << 16);
    let fixed: Vec<u8> = [
        0xFEEF_04BD, // signature
        0x0001_0000, // structure version 1.0
        ms,
        ls, // file version
        ms,
        ls,          // product version
        0x3F,        // flags mask
        0,           // flags
        0x0004_0004, // VOS_NT_WINDOWS32
        1,           // VFT_APP
        0,           // subtype
        0,
        0, // date
    ]
    .iter()
    .flat_map(|n: &u32| n.to_le_bytes())
    .collect();

    ver_block(
        "VS_VERSION_INFO",
        fixed.len() as u16,
        0,
        &fixed,
        &[string_file_info, var_file_info],
    )
}

/// One block of a version resource: its length, value length (in UTF-16
/// units for text, bytes otherwise) and type, its NUL-terminated key, its
/// value and its children, each starting on a 4-byte boundary.
fn ver_block(key: &str, value_len: u16, kind: u16, value: &[u8], children: &[Vec<u8>]) -> Vec<u8> {
    let mut out = vec![0, 0];
    out.extend_from_slice(&value_len.to_le_bytes());
    out.extend_from_slice(&kind.to_le_bytes());
    let mut name = utf16(key);
    name.push(0);
    out.extend_from_slice(&bytes(&name));
    pad4(&mut out);
    out.extend_from_slice(value);
    for child in children {
        pad4(&mut out);
        out.extend_from_slice(child);
    }
    let len = out.len() as u16;
    out[..2].copy_from_slice(&len.to_le_bytes());
    out
}

fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn bytes(units: &[u16]) -> Vec<u8> {
    units.iter().flat_map(|u| u.to_le_bytes()).collect()
}

fn pad4(out: &mut Vec<u8>) {
    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}

/// The current year (UTC), or the first year if the clock is unreadable.
/// Honours ``SOURCE_DATE_EPOCH`` for reproducible builds.
fn current_year() -> u32 {
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    let secs = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs())
        });
    let Some(secs) = secs else {
        return FIRST_YEAR;
    };
    // Civil-from-days (Howard Hinnant's algorithm), keeping only the year.
    let days = (secs / 86_400) as i64 + 719_468;
    let era = days.div_euclid(146_097);
    let doe = days.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let year = yoe + era * 400 + i64::from(mp >= 10);
    year as u32
}

/// Appends one resource with a numeric type and id to a ``.res`` file.
fn push_resource(res: &mut Vec<u8>, kind: u16, id: u16, data: &[u8]) {
    const HEADER_SIZE: u32 = 32;
    // As resource compilers write them: icons MOVEABLE | DISCARDABLE, the
    // version MOVEABLE | PURE, both tagged US English (Windows falls back
    // to it whatever the system language).
    let flags: u16 = match kind {
        0 => 0,
        RT_VERSION => 0x0030,
        _ => 0x1010,
    };
    let language: u16 = if kind == 0 { 0 } else { 0x0409 };
    res.extend_from_slice(&(data.len() as u32).to_le_bytes());
    res.extend_from_slice(&HEADER_SIZE.to_le_bytes());
    res.extend_from_slice(&[0xFF, 0xFF]);
    res.extend_from_slice(&kind.to_le_bytes());
    res.extend_from_slice(&[0xFF, 0xFF]);
    res.extend_from_slice(&id.to_le_bytes());
    res.extend_from_slice(&0u32.to_le_bytes()); // data version
    res.extend_from_slice(&flags.to_le_bytes());
    res.extend_from_slice(&language.to_le_bytes());
    res.extend_from_slice(&0u32.to_le_bytes()); // version
    res.extend_from_slice(&0u32.to_le_bytes()); // characteristics
    res.extend_from_slice(data);
    while !res.len().is_multiple_of(4) {
        res.push(0);
    }
}

fn fail(msg: &str) -> ! {
    eprintln!("\nerror: building the dashboard: {msg}\n");
    std::process::exit(1);
}
