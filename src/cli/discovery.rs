//! Font path inspection and TTC/TTF font discovery.
//! License: GPL-3.0-or-later

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use wordf::font::{extract_font_family_names, is_cjk_char, FontFallbackChain};

pub struct DiscoveredFonts {
    pub font_bytes: Vec<Vec<u8>>,
    pub font_dir: Option<PathBuf>,
    pub family_names: Vec<String>,
    pub primary_cjk_fallback: Option<String>,
    pub primary_cjk_font_bytes: Option<Vec<u8>>,
    pub fallback_chain: FontFallbackChain,
}

pub fn normalize_parent_dir(parent: Option<&Path>) -> PathBuf {
    match parent {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    }
}

#[allow(dead_code)]
pub fn inspect_fonts(path_str: Option<&String>) -> Result<DiscoveredFonts, i32> {
    let paths = path_str.map(|s| vec![s.clone()]).unwrap_or_default();
    inspect_font_chain(&paths)
}

pub fn inspect_font_chain(explicit_paths: &[String]) -> Result<DiscoveredFonts, i32> {
    let mut df = DiscoveredFonts {
        font_bytes: Vec::new(),
        font_dir: None,
        family_names: Vec::new(),
        primary_cjk_fallback: None,
        primary_cjk_font_bytes: None,
        fallback_chain: FontFallbackChain::new(),
    };

    // 1. Strict existence validation for all explicit font paths
    for p_str in explicit_paths {
        let p = Path::new(p_str);
        if !p.exists() {
            eprintln!("Error: Specified font path {:?} does not exist.", p);
            return Err(4);
        }
    }

    let mut candidate_paths: Vec<PathBuf> = explicit_paths.iter().map(PathBuf::from).collect();
    if candidate_paths.is_empty() {
        if let Ok(env_fp) = env::var("DOCXIDE_FONTS") {
            let p = PathBuf::from(env_fp);
            if p.exists() {
                candidate_paths.push(p);
            }
        }
    }

    if candidate_paths.is_empty() {
        return Ok(df);
    }

    let mut font_files = Vec::new();
    for p in &candidate_paths {
        if p.is_file() {
            font_files.push(p.clone());
            if df.font_dir.is_none() {
                df.font_dir = Some(normalize_parent_dir(p.parent()));
            }
        } else if p.is_dir() {
            if df.font_dir.is_none() {
                let dir = if p.as_os_str().is_empty() {
                    env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
                } else {
                    p.clone()
                };
                df.font_dir = Some(dir);
            }
            if let Ok(entries) = fs::read_dir(p) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                        let ext_lower = ext.to_lowercase();
                        if ext_lower == "ttf" || ext_lower == "otf" || ext_lower == "ttc" {
                            font_files.push(path);
                        }
                    }
                }
            }
        }
    }

    let mut font_entries = Vec::new();
    for ff in font_files {
        match fs::read(&ff) {
            Ok(bytes) => {
                let extracted = extract_font_family_names(&bytes);
                for name in &extracted {
                    if !df.family_names.contains(name) {
                        df.family_names.push(name.clone());
                    }
                }

                if let Some(cmap) = wordf::font::FontCmap::from_bytes(&bytes) {
                    let is_explicit = !explicit_paths.is_empty();
                    let has_basic_cjk = ['\u{4e2d}', '\u{6587}', '\u{5b57}', '\u{4f53}']
                        .iter()
                        .all(|&c| is_cjk_char(c) && cmap.has_glyph(c, &bytes));

                    if is_explicit || has_basic_cjk {
                        let primary_name = extracted
                            .first()
                            .cloned()
                            .unwrap_or_else(|| "DiscoveredFont".to_string());

                        df.fallback_chain
                            .push_with_cmap(primary_name.clone(), bytes.clone(), cmap);

                        if df.primary_cjk_fallback.is_none() {
                            df.primary_cjk_fallback = Some(primary_name);
                            df.primary_cjk_font_bytes = Some(bytes.clone());
                        }
                    }
                }

                font_entries.push((bytes, extracted));
            }
            Err(e) => {
                eprintln!("Warning: Failed to read font file {:?}: {}", ff, e);
            }
        }
    }

    if df.primary_cjk_fallback.is_none() {
        if let Some((bytes, names)) = font_entries.first() {
            df.primary_cjk_fallback = names.first().cloned();
            df.primary_cjk_font_bytes = Some(bytes.clone());
        }
    }

    df.font_bytes = font_entries.into_iter().map(|(b, _)| b).collect();
    Ok(df)
}

#[cfg(test)]
#[path = "discovery_tests.rs"]
mod tests;
