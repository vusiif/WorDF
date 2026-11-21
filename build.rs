//! Build script to compress the embedded TTF font at compile-time.
//! License: GPL-3.0-or-later

use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let font_rel_path = "assets/fonts/LXGWWenKaiLite-Regular.ttf";
    println!("cargo:rerun-if-changed={}", font_rel_path);

    let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set by Cargo");
    let target_compressed = Path::new(&out_dir).join("lxgw_wenkai_lite.deflate");

    if env::var("CARGO_FEATURE_EMBEDDED_CJK_FONT").is_ok() {
        let ttf_bytes = fs::read(font_rel_path)
            .unwrap_or_else(|e| panic!("Failed to read font asset at '{}': {}", font_rel_path, e));

        let compressed = miniz_oxide::deflate::compress_to_vec_zlib(&ttf_bytes, 10);
        fs::write(&target_compressed, compressed)
            .unwrap_or_else(|e| panic!("Failed to write compressed font to '{:?}': {}", target_compressed, e));
    } else {
        fs::write(&target_compressed, [])
            .unwrap_or_else(|e| panic!("Failed to write stub file to '{:?}': {}", target_compressed, e));
    }
}
