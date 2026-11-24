//! DOCX styles.xml parser and document-default CJK font injection.
//! License: GPL-3.0-or-later

use super::xml::{escape_xml_attr, extract_attribute_value, find_tag_closing_bracket, update_or_insert_attribute};

pub fn is_styles_part(name: &str) -> bool {
    name == "word/styles.xml"
}

/// Adapt `word/styles.xml` to ensure document-level default styles (<w:docDefaults> and Normal style)
/// include the CJK fallback font in `w:eastAsia`, enabling proper style-inherited Chinese typesetting.
pub fn adapt_styles_xml(
    styles_xml: &str,
    fallback_font: &str,
    available_fonts: &[String],
) -> String {
    let escaped_font = escape_xml_attr(fallback_font);
    let mut result = styles_xml.to_string();

    // 1. Handle <w:docDefaults>
    if let Some(dd_start) = result.find("<w:docDefaults>") {
        if let Some(dd_end_rel) = result[dd_start..].find("</w:docDefaults>") {
            let dd_end = dd_start + dd_end_rel;
            let dd_block = &result[dd_start..dd_end];

            if let Some(rf_start_rel) = dd_block.find("<w:rFonts") {
                let abs_rf_start = dd_start + rf_start_rel;
                let after_rf = abs_rf_start + "<w:rFonts".len();
                if let Some(rf_end_rel) = find_tag_closing_bracket(&result[after_rf..]) {
                    let abs_rf_end = after_rf + rf_end_rel;
                    let rf_tag = &result[abs_rf_start..=abs_rf_end];

                    let already_has_cjk = extract_attribute_value(rf_tag, "w:eastAsia")
                        .map(|font_name| available_fonts.iter().any(|f| f.eq_ignore_ascii_case(font_name)))
                        .unwrap_or(false);

                    if !already_has_cjk {
                        let updated_rf = update_or_insert_attribute(rf_tag, "w:eastAsia", fallback_font);

                        let mut new_xml = String::with_capacity(result.len() + 64);
                        new_xml.push_str(&result[..abs_rf_start]);
                        new_xml.push_str(&updated_rf);
                        new_xml.push_str(&result[abs_rf_end + 1..]);
                        result = new_xml;
                    }
                }
            } else if let Some(rpr_end_rel) = dd_block.find("</w:rPr>") {
                let abs_rpr_end = dd_start + rpr_end_rel;
                let mut new_xml = String::with_capacity(result.len() + 64);
                new_xml.push_str(&result[..abs_rpr_end]);
                new_xml.push_str(&format!(r#"<w:rFonts w:eastAsia="{}"/>"#, escaped_font));
                new_xml.push_str(&result[abs_rpr_end..]);
                result = new_xml;
            } else {
                // Insert <w:rPrDefault> block right before </w:docDefaults>
                let mut new_xml = String::with_capacity(result.len() + 128);
                new_xml.push_str(&result[..dd_end]);
                new_xml.push_str(&format!(
                    r#"<w:rPrDefault><w:rPr><w:rFonts w:eastAsia="{}"/></w:rPr></w:rPrDefault>"#,
                    escaped_font
                ));
                new_xml.push_str(&result[dd_end..]);
                result = new_xml;
            }
        }
    } else if let Some(styles_start) = result.find("<w:styles") {
        // Insert <w:docDefaults> right after <w:styles ...> opening tag
        let after_styles = styles_start + "<w:styles".len();
        if let Some(tag_end_rel) = find_tag_closing_bracket(&result[after_styles..]) {
            let abs_tag_end = after_styles + tag_end_rel;
            let mut new_xml = String::with_capacity(result.len() + 128);
            new_xml.push_str(&result[..=abs_tag_end]);
            new_xml.push_str(&format!(
                r#"<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:eastAsia="{}"/></w:rPr></w:rPrDefault></w:docDefaults>"#,
                escaped_font
            ));
            new_xml.push_str(&result[abs_tag_end + 1..]);
            result = new_xml;
        }
    }

    // 2. Handle Normal style (w:styleId="Normal")
    if let Some(normal_idx) = result.find(r#"w:styleId="Normal""#) {
        // Find enclosing <w:style ...> ... </w:style>
        if let Some(style_start_rel) = result[..normal_idx].rfind("<w:style") {
            if let Some(style_end_rel) = result[normal_idx..].find("</w:style>") {
                let abs_style_end = normal_idx + style_end_rel;
                let style_block = &result[style_start_rel..abs_style_end];

                if let Some(rf_start_rel) = style_block.find("<w:rFonts") {
                    let abs_rf_start = style_start_rel + rf_start_rel;
                    let after_rf = abs_rf_start + "<w:rFonts".len();
                    if let Some(rf_end_rel) = find_tag_closing_bracket(&result[after_rf..]) {
                        let abs_rf_end = after_rf + rf_end_rel;
                        let rf_tag = &result[abs_rf_start..=abs_rf_end];

                        let already_has_cjk = extract_attribute_value(rf_tag, "w:eastAsia")
                            .map(|font_name| available_fonts.iter().any(|f| f.eq_ignore_ascii_case(font_name)))
                            .unwrap_or(false);

                        if !already_has_cjk {
                            let updated_rf = update_or_insert_attribute(rf_tag, "w:eastAsia", fallback_font);

                            let mut new_xml = String::with_capacity(result.len() + 64);
                            new_xml.push_str(&result[..abs_rf_start]);
                            new_xml.push_str(&updated_rf);
                            new_xml.push_str(&result[abs_rf_end + 1..]);
                            result = new_xml;
                        }
                    }
                }
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adapt_styles_inject_doc_defaults_when_missing() {
        let styles = r#"<?xml version="1.0" encoding="UTF-8"?><w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"></w:styles>"#;
        let adapted = adapt_styles_xml(styles, "LXGW WenKai Lite", &[]);
        assert!(adapted.contains(r#"<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:eastAsia="LXGW WenKai Lite"/></w:rPr></w:rPrDefault></w:docDefaults>"#));
    }

    #[test]
    fn test_adapt_styles_update_existing_rfonts_preserving_ascii() {
        let styles = r#"<w:styles><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/></w:rPr></w:rPrDefault></w:docDefaults></w:styles>"#;
        let adapted = adapt_styles_xml(styles, "SimSun", &[]);
        assert!(adapted.contains(r#"w:ascii="Calibri""#), "West-European font must be preserved");
        assert!(adapted.contains(r#"w:eastAsia="SimSun""#), "East Asian font must be injected");
    }

    #[test]
    fn test_adapt_styles_not_skipped_by_available_western_font() {
        // Even if available_fonts includes "Calibri" or "Arial", presence of Calibri in styles.xml
        // must not skip injecting w:eastAsia="SimSun".
        let styles = r#"<w:styles><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:hAnsi="Calibri"/></w:rPr></w:rPrDefault></w:docDefaults></w:styles>"#;
        let available = vec!["Calibri".to_string(), "Arial".to_string()];
        let adapted = adapt_styles_xml(styles, "SimSun", &available);
        assert!(adapted.contains(r#"w:eastAsia="SimSun""#), "East Asian font must still be injected even if western fonts are present in available_fonts");
    }

    #[test]
    fn test_adapt_styles_skip_when_available_font_already_present() {
        let styles = r#"<w:styles><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:eastAsia="SimHei"/></w:rPr></w:rPrDefault></w:docDefaults></w:styles>"#;
        let available = vec!["SimHei".to_string()];
        let adapted = adapt_styles_xml(styles, "SimSun", &available);
        assert_eq!(adapted, styles, "Must not overwrite existing available Chinese font");
    }
}
