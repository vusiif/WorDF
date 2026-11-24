//! Batch conversion JSON report generation.
//! License: GPL-3.0-or-later

use super::escape_json_str;

#[derive(Debug, Clone, PartialEq)]
pub struct BatchItemReport {
    pub input_path: String,
    pub output_path: String,
    pub status: String,
    pub page_count: u32,
    pub elapsed_ms: f64,
    pub error: Option<String>,
}

pub struct BatchReportData<'a> {
    pub tool_name: &'a str,
    pub tool_version: &'a str,
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub skipped: usize,
    pub elapsed_ms: f64,
    pub items: &'a [BatchItemReport],
}

pub fn format_batch_report_json(data: &BatchReportData) -> String {
    let mut items_json = String::new();
    if data.items.is_empty() {
        items_json.push_str("[]");
    } else {
        items_json.push_str("[\n");
        for (i, item) in data.items.iter().enumerate() {
            items_json.push_str("    {\n");
            items_json.push_str(&format!(
                "      \"input\": \"{}\",\n",
                escape_json_str(&item.input_path)
            ));
            items_json.push_str(&format!(
                "      \"output\": \"{}\",\n",
                escape_json_str(&item.output_path)
            ));
            items_json.push_str(&format!(
                "      \"status\": \"{}\",\n",
                escape_json_str(&item.status)
            ));
            items_json.push_str(&format!("      \"page_count\": {},\n", item.page_count));
            items_json.push_str(&format!("      \"elapsed_ms\": {:.1},\n", item.elapsed_ms));
            match &item.error {
                Some(err) => {
                    items_json
                        .push_str(&format!("      \"error\": \"{}\"\n", escape_json_str(err)));
                }
                None => {
                    items_json.push_str("      \"error\": null\n");
                }
            }
            if i + 1 < data.items.len() {
                items_json.push_str("    },\n");
            } else {
                items_json.push_str("    }\n");
            }
        }
        items_json.push_str("  ]");
    }

    format!(
        r#"{{
  "schema_version": 1,
  "tool": {{
    "name": "{}",
    "version": "{}"
  }},
  "mode": "batch",
  "summary": {{
    "total": {},
    "succeeded": {},
    "failed": {},
    "skipped": {},
    "elapsed_ms": {:.1}
  }},
  "items": {}
}}
"#,
        escape_json_str(data.tool_name),
        escape_json_str(data.tool_version),
        data.total,
        data.succeeded,
        data.failed,
        data.skipped,
        data.elapsed_ms,
        items_json
    )
}
