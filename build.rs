//! Builds the Vue dashboard in ``web/`` so the binary can embed it.
//!
//! Cargo reruns this only when a dashboard source file changes, and Vite
//! writes the result to ``web/dist``, which ``src/server/http.rs`` embeds.
//! It needs Node.js 18+ with npm on ``PATH``. Set ``VIGIL_SKIP_WEB_BUILD=1``
//! to use an existing ``web/dist`` as-is (for example one built in an
//! earlier CI step).
//!
//! For Windows targets it also gives the executable the dashboard's
//! favicon as its icon (see ``windows_icon``), so Explorer and the taskbar
//! show the same picture as the tray.

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
    for path in WATCH {
        println!("cargo:rerun-if-changed={path}");
    }
    println!("cargo:rerun-if-env-changed=VIGIL_SKIP_WEB_BUILD");

    // CARGO_CFG_* describe the target, which differs from the host when
    // cross-compiling.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        windows_icon();
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

/// The executable's icon. Windows reads it from the program's resources: one
/// ``RT_ICON`` per image and an ``RT_GROUP_ICON`` directory listing them.
/// This writes those straight from ``favicon.ico`` into a compiled resource
/// file (``.res``), so no resource compiler is needed with MSVC, whose
/// linker takes ``.res`` files as they are. The GNU linker doesn't, so
/// there MinGW's ``windres`` turns it into an object file first; without
/// ``windres`` the build goes on and the program just has no icon.
fn windows_icon() {
    const ICO: &str = "web/public/favicon.ico";
    println!("cargo:rerun-if-changed={ICO}");
    let ico = std::fs::read(ICO).unwrap_or_else(|e| fail(&format!("reading {ICO}: {e}")));
    let res =
        icon_resources(&ico).unwrap_or_else(|| fail(&format!("{ICO} isn't a valid icon file.")));

    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR is set by cargo"));
    let res_path = out.join("icon.res");
    std::fs::write(&res_path, res).unwrap_or_else(|e| fail(&format!("writing icon.res: {e}")));

    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins={}", res_path.display());
        return;
    }
    let obj = out.join("icon.o");
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
        _ => println!("cargo:warning=couldn't run {windres}, so the program has no icon"),
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

/// Appends one resource with a numeric type and id to a ``.res`` file.
fn push_resource(res: &mut Vec<u8>, kind: u16, id: u16, data: &[u8]) {
    const HEADER_SIZE: u32 = 32;
    // MOVEABLE | DISCARDABLE and US English, as resource compilers write
    // by default (Windows falls back to it whatever the system language).
    let (flags, language): (u16, u16) = if kind == 0 { (0, 0) } else { (0x1010, 0x0409) };
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
