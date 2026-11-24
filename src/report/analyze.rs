//! Document analysis JSON report generation.
//! License: GPL-3.0-or-later

use super::escape_json_str;
use crate::analyze::DocumentAnalysis;

pub fn format_analysis_json(analysis: &DocumentAnalysis) -> String {
    let mut warnings_json = String::new();
    if analysis.warnings.is_empty() {
        warnings_json.push_str("[]");
    } else {
        warnings_json.push_str("[\n");
        for (i, w) in analysis.warnings.iter().enumerate() {
            warnings_json.push_str("      {\n");
            warnings_json.push_str(&format!(
                "        \"code\": \"{}\",\n",
                escape_json_str(&w.code)
            ));
            warnings_json.push_str(&format!(
                "        \"message\": \"{}\"\n",
                escape_json_str(&w.message)
            ));
            if i + 1 < analysis.warnings.len() {
                warnings_json.push_str("      },\n");
            } else {
                warnings_json.push_str("      }\n");
            }
        }
        warnings_json.push_str("    ]");
    }

    let fonts_json = if analysis.detected_fonts.is_empty() {
        "[]".to_string()
    } else {
        let list: Vec<String> = analysis
            .detected_fonts
            .iter()
            .map(|f| format!("\"{}\"", escape_json_str(f)))
            .collect();
        format!("[{}]", list.join(", "))
    };

    let missing_glyphs_json = if analysis.missing_glyphs.is_empty() {
        "[]".to_string()
    } else {
        let list: Vec<String> = analysis
            .missing_glyphs
            .iter()
            .map(|c| format!("\"{}\"", escape_json_str(&c.to_string())))
            .collect();
        format!("[{}]", list.join(", "))
    };

    let meta_json = format!(
        "{{\"title\":{},\"author\":{},\"subject\":{},\"keywords\":{}}}",
        analysis
            .metadata
            .title
            .as_ref()
            .map(|s| format!("\"{}\"", escape_json_str(s)))
            .unwrap_or_else(|| "null".to_string()),
        analysis
            .metadata
            .author
            .as_ref()
            .map(|s| format!("\"{}\"", escape_json_str(s)))
            .unwrap_or_else(|| "null".to_string()),
        analysis
            .metadata
            .subject
            .as_ref()
            .map(|s| format!("\"{}\"", escape_json_str(s)))
            .unwrap_or_else(|| "null".to_string()),
        analysis
            .metadata
            .keywords
            .as_ref()
            .map(|s| format!("\"{}\"", escape_json_str(s)))
            .unwrap_or_else(|| "null".to_string()),
    );

    format!(
        r#"{{
  "schema_version": 1,
  "tool": {{
    "name": "wordf",
    "version": "1.0.0"
  }},
  "mode": "analyze",
  "document": {{
    "format": "{}",
    "bytes": {}
  }},
  "metadata": {},
  "structure": {{
    "paragraphs": {},
    "tables": {},
    "text_runs": {},
    "media_count": {},
    "media_bytes": {},
    "header_footer_count": {}
  }},
  "typography": {{
    "cjk_characters": {},
    "declared_fonts": {},
    "missing_glyphs": {}
  }},
  "diagnostics": {{
    "status": "{}",
    "warnings": {}
  }}
}}
"#,
        escape_json_str(&analysis.format_name),
        analysis.input_bytes,
        meta_json,
        analysis.paragraph_count,
        analysis.table_count,
        analysis.run_count,
        analysis.media_count,
        analysis.media_bytes,
        analysis.header_footer_count,
        analysis.cjk_char_count,
        fonts_json,
        missing_glyphs_json,
        if analysis.warnings.is_empty() {
            "ok"
        } else {
            "warnings_found"
        },
        warnings_json
    )
}
