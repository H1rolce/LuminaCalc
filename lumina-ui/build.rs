//! Embeds the multi-resolution Windows application icon (`assets/icon.ico`) into the
//! `lumina-ui` PE executable so Explorer, Taskbar, Desktop shortcuts, and Alt+Tab show it.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=assets/icon.rc");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));

    // Stage `icon.ico` & `icon.rc` into ASCII `OUT_DIR` so windres never trips on non-ASCII paths.
    let src_ico = manifest_dir.join("assets").join("icon.ico");
    let src_rc = manifest_dir.join("assets").join("icon.rc");
    let dst_ico = out_dir.join("icon.ico");
    let dst_rc = out_dir.join("icon.rc");
    let dst_res = out_dir.join("icon.res");

    if fs::copy(&src_ico, &dst_ico).is_err() || fs::copy(&src_rc, &dst_rc).is_err() {
        return;
    }

    let windres_candidates = ["x86_64-w64-mingw32-windres", "llvm-windres", "windres"];
    for tool in windres_candidates {
        let status = Command::new(tool)
            .current_dir(&out_dir)
            .args(["-O", "coff", "-i", "icon.rc", "-o", "icon.res"])
            .status();
        if let Ok(s) = status
            && s.success()
        {
            println!("cargo:rustc-link-arg-bin=lumina-ui={}", dst_res.display());
            return;
        }
    }
}
