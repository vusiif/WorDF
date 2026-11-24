//! Unit tests for font discovery and TTC/TTF family name extraction.
//! License: GPL-3.0-or-later

use super::*;
use wordf::font::extract_font_family_names;

fn build_minimal_ttc(family_names: &[&str]) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend_from_slice(b"ttcf");
    buf.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]);
    let num_fonts = family_names.len() as u32;
    buf.extend_from_slice(&num_fonts.to_be_bytes());

    let offsets_start = buf.len();
    buf.resize(offsets_start + (num_fonts as usize) * 4, 0);

    for (idx, name) in family_names.iter().enumerate() {
        let font_offset = buf.len() as u32;
        let off_idx = offsets_start + idx * 4;
        buf[off_idx..off_idx + 4].copy_from_slice(&font_offset.to_be_bytes());

        buf.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]);
        buf.extend_from_slice(&1u16.to_be_bytes());
        buf.extend_from_slice(&[0, 0, 0, 0, 0, 0]);

        let table_rec_pos = buf.len();
        buf.extend_from_slice(b"name");
        buf.extend_from_slice(&[0, 0, 0, 0]);
        buf.extend_from_slice(&[0, 0, 0, 0]);
        buf.extend_from_slice(&[0, 0, 0, 0]);

        let name_table_pos = buf.len() as u32;
        buf[table_rec_pos + 8..table_rec_pos + 12].copy_from_slice(&name_table_pos.to_be_bytes());

        let utf16_bytes: Vec<u8> = name.encode_utf16().flat_map(|u| u.to_be_bytes()).collect();
        let string_offset = 6 + 12;
        buf.extend_from_slice(&0u16.to_be_bytes());
        buf.extend_from_slice(&1u16.to_be_bytes());
        buf.extend_from_slice(&(string_offset as u16).to_be_bytes());

        buf.extend_from_slice(&3u16.to_be_bytes());
        buf.extend_from_slice(&1u16.to_be_bytes());
        buf.extend_from_slice(&0x0409u16.to_be_bytes());
        buf.extend_from_slice(&1u16.to_be_bytes());
        buf.extend_from_slice(&(utf16_bytes.len() as u16).to_be_bytes());
        buf.extend_from_slice(&0u16.to_be_bytes());
        buf.extend_from_slice(&utf16_bytes);

        let table_len = (buf.len() as u32) - name_table_pos;
        buf[table_rec_pos + 12..table_rec_pos + 16].copy_from_slice(&table_len.to_be_bytes());
    }

    buf
}

#[test]
fn test_extract_font_family_names_ttc() {
    let ttc_bytes = build_minimal_ttc(&["Microsoft YaHei", "Microsoft YaHei UI"]);
    let names = extract_font_family_names(&ttc_bytes);
    assert_eq!(names, vec!["Microsoft YaHei", "Microsoft YaHei UI"]);
}

#[test]
fn test_extract_font_family_names_single_font() {
    let mut buf = Vec::new();
    buf.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]);
    buf.extend_from_slice(&1u16.to_be_bytes());
    buf.extend_from_slice(&[0, 0, 0, 0, 0, 0]);

    let table_rec_pos = buf.len();
    buf.extend_from_slice(b"name");
    buf.extend_from_slice(&[0, 0, 0, 0]);
    buf.extend_from_slice(&[0, 0, 0, 0]);
    buf.extend_from_slice(&[0, 0, 0, 0]);

    let name_table_pos = buf.len() as u32;
    buf[table_rec_pos + 8..table_rec_pos + 12].copy_from_slice(&name_table_pos.to_be_bytes());

    let utf16_bytes: Vec<u8> = "SingleFontFamily"
        .encode_utf16()
        .flat_map(|u| u.to_be_bytes())
        .collect();
    let string_offset = 6 + 12;
    buf.extend_from_slice(&0u16.to_be_bytes());
    buf.extend_from_slice(&1u16.to_be_bytes());
    buf.extend_from_slice(&(string_offset as u16).to_be_bytes());

    buf.extend_from_slice(&3u16.to_be_bytes());
    buf.extend_from_slice(&1u16.to_be_bytes());
    buf.extend_from_slice(&0x0409u16.to_be_bytes());
    buf.extend_from_slice(&1u16.to_be_bytes());
    buf.extend_from_slice(&(utf16_bytes.len() as u16).to_be_bytes());
    buf.extend_from_slice(&0u16.to_be_bytes());
    buf.extend_from_slice(&utf16_bytes);

    let table_len = (buf.len() as u32) - name_table_pos;
    buf[table_rec_pos + 12..table_rec_pos + 16].copy_from_slice(&table_len.to_be_bytes());

    let names = extract_font_family_names(&buf);
    assert_eq!(names, vec!["SingleFontFamily"]);
}

#[test]
fn test_normalize_parent_dir_bare_filename() {
    let bare = Path::new("bare_font.ttf");
    assert_eq!(bare.parent(), Some(Path::new("")));
    let normalized = normalize_parent_dir(bare.parent());
    assert!(
        !normalized.as_os_str().is_empty(),
        "Normalized path must not be empty"
    );
    assert_ne!(normalized, PathBuf::from(""));

    let with_dir = Path::new("fonts/bare_font.ttf");
    assert_eq!(
        normalize_parent_dir(with_dir.parent()),
        PathBuf::from("fonts")
    );

    let abs_path = Path::new("/usr/share/fonts/bare_font.ttf");
    assert_eq!(
        normalize_parent_dir(abs_path.parent()),
        PathBuf::from("/usr/share/fonts")
    );
}

#[test]
fn test_inspect_fonts_with_bare_file() {
    let temp_dir = std::env::temp_dir();
    let font_path = temp_dir.join("temp_wordf_test_font.ttf");
    fs::write(&font_path, b"dummy").unwrap();

    let fonts = inspect_fonts(Some(&font_path.to_string_lossy().to_string())).unwrap();
    assert!(fonts.font_dir.is_some());
    let dir = fonts.font_dir.unwrap();
    assert!(
        !dir.as_os_str().is_empty(),
        "font_dir must never be empty string"
    );

    let _ = fs::remove_file(&font_path);
}

#[test]
fn test_inspect_fonts_rejects_nonexistent_explicit_path() {
    let fake_path = "nonexistent_font_path_xyz_12345.ttf".to_string();
    let res = inspect_fonts(Some(&fake_path));
    assert_eq!(
        res.err(),
        Some(4),
        "Explicit font path that does not exist must return exit code 4"
    );
}

#[test]
fn test_font_path_overrides_preset_env() {
    let temp_dir = std::env::temp_dir();
    let font_path = temp_dir.join("test_override_font.ttf");
    fs::write(&font_path, b"dummy").unwrap();

    let old_env = env::var("DOCXIDE_FONTS").ok();
    env::set_var("DOCXIDE_FONTS", "/preset/old/path");

    let cli_font_arg = Some(font_path.to_string_lossy().to_string());
    let discovered = inspect_fonts(cli_font_arg.as_ref()).unwrap();
    if let Some(ref dir) = discovered.font_dir {
        if cli_font_arg.is_some() || env::var("DOCXIDE_FONTS").is_err() {
            env::set_var("DOCXIDE_FONTS", dir);
        }
    }

    let current_env = env::var("DOCXIDE_FONTS").unwrap();
    assert_ne!(current_env, "/preset/old/path");
    assert_eq!(
        current_env,
        normalize_parent_dir(font_path.parent())
            .to_string_lossy()
            .to_string()
    );

    match old_env {
        Some(v) => env::set_var("DOCXIDE_FONTS", v),
        None => env::remove_var("DOCXIDE_FONTS"),
    }
    let _ = fs::remove_file(&font_path);
}
