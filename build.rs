//! Builds the Vue dashboard in ``web/`` so the binary can embed it.
//!
//! Cargo reruns this only when a dashboard source file changes, and Vite
//! writes the result to ``web/dist``, which ``src/server/http.rs`` embeds.
//! It needs Node.js 18+ with npm on ``PATH``. Set ``VIGIL_SKIP_WEB_BUILD=1``
//! to use an existing ``web/dist`` as-is (for example one built in an
//! earlier CI step).

use std::path::Path;
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

fn fail(msg: &str) -> ! {
    eprintln!("\nerror: building the dashboard: {msg}\n");
    std::process::exit(1);
}
