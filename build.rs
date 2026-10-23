//! Build-time inputs:
//!
//! - `BOOKMARK_UI_DIST`: the directory `include_dir!` embeds with feature
//!   `ui`: `ui/dist` when it was built (`npm run build`), else an empty
//!   directory in `OUT_DIR` (the binary then serves no remote).
//! - `BOOKMARK_VERSION`: the release stamped into the binary (the image build
//!   passes `APP_VERSION`); `CARGO_PKG_VERSION` otherwise.
//!
//! `include_dir!` and `sqlx::migrate!` do not track the files they embed in
//! every case, so the embedded trees are declared here.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=docs/help");
    println!("cargo:rerun-if-changed=api/openapi");
    println!("cargo:rerun-if-changed=migrations");
    println!("cargo:rerun-if-changed=ui/dist");
    println!("cargo:rerun-if-env-changed=BOOKMARK_VERSION");
    let manifest_dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let dist = manifest_dir.join("ui").join("dist");
    let embed = if dist.join("mf-manifest.json").is_file() {
        dist
    } else {
        let empty = PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default()).join("no-ui-dist");
        if let Err(e) = std::fs::create_dir_all(&empty) {
            println!("cargo:warning=cannot create {}: {e}", empty.display());
        }
        empty
    };
    println!("cargo:rustc-env=BOOKMARK_UI_DIST={}", embed.display());
}
