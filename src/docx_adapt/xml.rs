//! DOCX XML text run adaptation and CJK font injection.
//! License: GPL-3.0-or-later

pub use super::xml_util::{
    count_cjk_in_xml_text_nodes, escape_xml_attr, extract_attribute_value, find_tag_closing_bracket,
    for_each_xml_text_node, is_text_part, update_or_insert_attribute,
};
use crate::font::count_cjk_chars;

/// Injects or updates `w:eastAsia="fallback_font"` in a `<w:r>...</w:r>` XML run slice.
pub fn adapt_run_xml(run_xml: &str, fallback_font: &str, available_fonts: &[String]) -> String {
    let mut has_cjk = false;
    for_each_xml_text_node(run_xml, |text| {
        if !has_cjk && count_cjk_chars(text) > 0 {
            has_cjk = true;
        }
    });

    if !has_cjk {
        return run_xml.to_string();
    }

    adapt_run_xml_prescanned(run_xml, fallback_font, available_fonts)
}

/// Injects or updates `w:eastAsia` assuming CJK characters have already been identified.
fn adapt_run_xml_prescanned(run_xml: &str, fallback_font: &str, available_fonts: &[String]) -> String {
    // 1. Check if <w:rFonts exists
    if let Some(rfonts_idx) = run_xml.find("<w:rFonts") {
        let rfonts_start = rfonts_idx;
        let rfonts_after = rfonts_idx + "<w:rFonts".len();

        if let Some(rfonts_end_rel) = find_tag_closing_bracket(&run_xml[rfonts_after..]) {
            let abs_rfonts_end = rfonts_after + rfonts_end_rel;
            let rfonts_tag = &run_xml[rfonts_start..=abs_rfonts_end];

            let already_has_cjk_font = extract_attribute_value(rfonts_tag, "w:eastAsia")
                .map(|font_name| available_fonts.iter().any(|f| f.eq_ignore_ascii_case(font_name)))
                .unwrap_or(false);

            if already_has_cjk_font {
                return run_xml.to_string();
            }

            let new_rfonts = update_or_insert_attribute(rfonts_tag, "w:eastAsia", fallback_font);

            let mut result = String::with_capacity(run_xml.len() + 64);
            result.push_str(&run_xml[..rfonts_start]);
            result.push_str(&new_rfonts);
            result.push_str(&run_xml[abs_rfonts_end + 1..]);
            return result;
        }
    }

    let escaped_font = escape_xml_attr(fallback_font);

    // 2. Check if self-closing <w:rPr ... /> exists
    if let Some(rpr_idx) = run_xml.find("<w:rPr") {
        let rpr_after = rpr_idx + "<w:rPr".len();
        if let Some(rpr_end_rel) = find_tag_closing_bracket(&run_xml[rpr_after..]) {
            let abs_tag_end = rpr_after + rpr_end_rel;
            let rpr_tag = &run_xml[rpr_idx..=abs_tag_end];
            let trimmed_tag = rpr_tag.trim_end();

            if let Some(stripped) = trimmed_tag.strip_suffix("/>") {
                let open_prefix = stripped.trim_end();
                let mut result = String::with_capacity(run_xml.len() + 64);
                result.push_str(&run_xml[..rpr_idx]);
                result.push_str(open_prefix);
                result.push_str(&format!(r#"><w:rFonts w:eastAsia="{}"/></w:rPr>"#, escaped_font));
                result.push_str(&run_xml[abs_tag_end + 1..]);
                return result;
            }
        }
    }

    // 3. Paired <w:rPr>...</w:rPr>
    if let Some(rpr_end) = run_xml.find("</w:rPr>") {
        let mut result = String::with_capacity(run_xml.len() + 64);
        result.push_str(&run_xml[..rpr_end]);
        result.push_str(&format!(r#"<w:rFonts w:eastAsia="{}"/>"#, escaped_font));
        result.push_str(&run_xml[rpr_end..]);
        return result;
    }

    // 4. No <w:rPr>: insert <w:rPr><w:rFonts .../></w:rPr> right after <w:r ...>
    if let Some(r_tag_end) = find_tag_closing_bracket(run_xml) {
        let mut result = String::with_capacity(run_xml.len() + 80);
        result.push_str(&run_xml[..=r_tag_end]);
        result.push_str(&format!(
            r#"<w:rPr><w:rFonts w:eastAsia="{}"/></w:rPr>"#,
            escaped_font
        ));
        result.push_str(&run_xml[r_tag_end + 1..]);
        return result;
    }

    run_xml.to_string()
}

/// Adapts all runs in a document XML string for CJK fonts, skipping comments and CDATA.
pub fn adapt_xml_text(
    xml: &str,
    fallback_font: &str,
    available_fonts: &[String],
) -> (String, usize) {
    let mut out = String::with_capacity(xml.len() + 1024);
    let mut total_cjk = 0;
    let mut cursor = 0;

    while cursor < xml.len() {
        if let Some(c_rel) = xml[cursor..].find("<!--") {
            let abs_c_start = cursor + c_rel;
            let r_start_opt = xml[cursor..].find("<w:r");
            let should_skip_comment = match r_start_opt {
                Some(r_rel) => abs_c_start < cursor + r_rel,
                None => true,
            };
            if should_skip_comment {
                if let Some(c_end_rel) = xml[abs_c_start + 4..].find("-->") {
                    let abs_c_end = abs_c_start + 4 + c_end_rel + 3;
                    out.push_str(&xml[cursor..abs_c_end]);
                    cursor = abs_c_end;
                    continue;
                }
            }
        }

        let Some(r_rel) = xml[cursor..].find("<w:r") else {
            break;
        };
        let abs_r_start = cursor + r_rel;
        let after_wr = abs_r_start + 4;

        if after_wr < xml.len() {
            let next_b = xml.as_bytes()[after_wr];
            if matches!(next_b, b'>' | b' ' | b'\t' | b'\r' | b'\n' | b'/') {
                let tag_end_opt = find_tag_closing_bracket(&xml[after_wr..]);
                let abs_tag_end = match tag_end_opt {
                    Some(idx) => after_wr + idx,
                    None => {
                        out.push_str(&xml[cursor..]);
                        return (out, total_cjk);
                    }
                };

                let open_tag = &xml[abs_r_start..=abs_tag_end];
                let is_self_closing = open_tag.trim_end().ends_with("/>");
                if is_self_closing {
                    out.push_str(&xml[cursor..=abs_tag_end]);
                    cursor = abs_tag_end + 1;
                    continue;
                }

                if let Some(r_end) = xml[abs_tag_end + 1..].find("</w:r>") {
                    let abs_r_end = abs_tag_end + 1 + r_end + 6;
                    out.push_str(&xml[cursor..abs_r_start]);
                    let run_slice = &xml[abs_r_start..abs_r_end];
                    let cjk_count = count_cjk_in_xml_text_nodes(run_slice);
                    total_cjk += cjk_count;
                    if cjk_count > 0 {
                        let adapted = adapt_run_xml_prescanned(run_slice, fallback_font, available_fonts);
                        out.push_str(&adapted);
                    } else {
                        out.push_str(run_slice);
                    }
                    cursor = abs_r_end;
                    continue;
                }
            }
        }

        out.push_str(&xml[cursor..abs_r_start + 4]);
        cursor = abs_r_start + 4;
    }

    out.push_str(&xml[cursor..]);
    (out, total_cjk)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skip_xml_comment_containing_fake_run() {
        let xml = r#"<w:p><!-- <w:r><w:t>注释中的中文</w:t></w:r> --><w:r><w:t>正文</w:t></w:r></w:p>"#;
        let (adapted, count) = adapt_xml_text(xml, "SimSun", &[]);
        assert_eq!(count, 2, "Only real run text should be counted, not comments");
        assert!(adapted.contains("<!-- <w:r><w:t>注释中的中文</w:t></w:r> -->"));
        assert!(adapted.contains(r#"<w:rPr><w:rFonts w:eastAsia="SimSun"/></w:rPr><w:t>正文</w:t>"#));
    }

    #[test]
    fn test_single_quoted_and_spaced_attributes_no_panic() {
        let run = "<w:r><w:rFonts w:ascii='Calibri' w:eastAsia = 'OldFont' /><w:t>测试</w:t></w:r>";
        let adapted = adapt_run_xml(run, "SimSun", &[]);
        assert!(adapted.contains(r#"w:eastAsia="SimSun""#) || adapted.contains(r#"w:eastAsia = 'SimSun'"#));
        assert!(!adapted.contains("OldFont"));
    }

    #[test]
    fn test_self_closing_rpr_with_trailing_spaces() {
        let run = "<w:r><w:rPr   /><w:t>测试空格自闭合</w:t></w:r>";
        let adapted = adapt_run_xml(run, "SimSun", &[]);
        assert!(adapted.contains(r#"<w:rPr><w:rFonts w:eastAsia="SimSun"/></w:rPr>"#));
    }

    #[test]
    fn test_attribute_value_with_bracket_no_confusion() {
        let slice = r#" attr="a>b">rest"#;
        assert_eq!(find_tag_closing_bracket(slice), Some(11));
    }

    #[test]
    fn test_run_with_available_western_font_does_not_skip_cjk_fallback() {
        // If a run has w:ascii="Arial" and "Arial" is in available_fonts, it MUST NOT
        // prevent w:eastAsia="SimSun" from being injected when the run contains CJK characters.
        let run = r#"<w:r><w:rPr><w:rFonts w:ascii="Arial" w:hAnsi="Arial"/></w:rPr><w:t>中文测试</w:t></w:r>"#;
        let available = vec!["Arial".to_string()];
        let adapted = adapt_run_xml(run, "SimSun", &available);
        assert!(adapted.contains(r#"w:eastAsia="SimSun""#), "Must inject East Asian fallback even if Arial is available");
        assert!(adapted.contains(r#"w:ascii="Arial""#), "Must preserve original western font");
    }

    #[test]
    fn test_font_name_with_quotes_properly_escaped() {
        let run = r#"<w:r><w:t>中文</w:t></w:r>"#;
        let adapted = adapt_run_xml(run, "Font\" style=\"injection", &[]);
        assert!(adapted.contains(r#"&quot;"#));
        assert!(!adapted.contains(r#"Font" style"#));
    }
}
