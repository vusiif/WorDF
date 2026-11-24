//! WorDF schema-v1 JSON report generator.
//! Implements deterministic UTF-8 manual formatting without external serde_json dependency.
//! License: GPL-3.0-or-later

pub mod analyze;
pub mod batch;

pub use analyze::format_analysis_json;
pub use batch::{format_batch_report_json, BatchItemReport, BatchReportData};

use crate::Diagnostic;

pub struct ReportData<'a> {
    pub tool_name: &'a str,
    pub tool_version: &'a str,
    pub input_format: &'a str,
    pub input_bytes: usize,
    pub backend_name: &'a str,
    pub backend_version: &'a str,
    pub conversion_status: &'a str,
    pub delivery_status: &'a str,
    pub pdf_bytes: usize,
    pub pdf_page_count: u32,
    pub warnings: &'a [Diagnostic],
    pub conversion_ms: f64,
    pub pre_report_ms: f64,
    pub font_policy: &'a str,
    pub fallback_used: bool,
}

pub fn escape_json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\x0C' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

pub fn format_schema_v1_json(data: &ReportData) -> String {
    let mut warnings_json = String::new();
    if data.warnings.is_empty() {
        warnings_json.push_str("[]");
    } else {
        warnings_json.push_str("[\n");
        for (i, w) in data.warnings.iter().enumerate() {
            warnings_json.push_str("    {\n");
            warnings_json.push_str(&format!(
                "      \"code\": \"{}\",\n",
                escape_json_str(&w.code)
            ));
            warnings_json.push_str(&format!(
                "      \"message\": \"{}\"\n",
                escape_json_str(&w.message)
            ));
            if i + 1 < data.warnings.len() {
                warnings_json.push_str("    },\n");
            } else {
                warnings_json.push_str("    }\n");
            }
        }
        warnings_json.push_str("  ]");
    }

    format!(
        r#"{{
  "schema_version": 1,
  "tool": {{
    "name": "{}",
    "version": "{}"
  }},
  "input": {{
    "format": "{}",
    "bytes": {}
  }},
  "backend": {{
    "name": "{}",
    "version": "{}"
  }},
  "conversion_status": "{}",
  "delivery_status": "{}",
  "pdf": {{
    "bytes": {},
    "page_count": {}
  }},
  "warnings": {},
  "timing": {{
    "conversion_ms": {:.1},
    "pre_report_ms": {:.1}
  }},
  "font_environment": {{
    "policy": "{}",
    "fallback_used": {}
  }}
}}
"#,
        escape_json_str(data.tool_name),
        escape_json_str(data.tool_version),
        escape_json_str(data.input_format),
        data.input_bytes,
        escape_json_str(data.backend_name),
        escape_json_str(data.backend_version),
        escape_json_str(data.conversion_status),
        escape_json_str(data.delivery_status),
        data.pdf_bytes,
        data.pdf_page_count,
        warnings_json,
        data.conversion_ms,
        data.pre_report_ms,
        escape_json_str(data.font_policy),
        if data.fallback_used { "true" } else { "false" }
    )
}

#[cfg(test)]
mod tests;
