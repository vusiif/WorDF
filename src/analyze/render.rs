//! Human-readable rendering of document analysis results.
//! License: GPL-3.0-or-later

use super::DocumentAnalysis;

impl DocumentAnalysis {
    /// Formats the analysis results into a human-readable terminal summary.
    pub fn render_console_summary(&self, input_name: &str) -> String {
        let mut out = String::new();
        out.push_str("==================================================\n          WorDF Document Analysis Report          \n==================================================\n");
        out.push_str(&format!(
            "Target File: {}\nFile Size:   {} bytes ({:.2} KiB)\nFormat:      {}\n",
            input_name,
            self.input_bytes,
            self.input_bytes as f64 / 1024.0,
            self.format_name
        ));
        if !self.metadata.is_empty() {
            out.push_str("\n[Document Metadata]\n");
            if let Some(ref t) = self.metadata.title {
                out.push_str(&format!("  - Title:             {}\n", t));
            }
            if let Some(ref a) = self.metadata.author {
                out.push_str(&format!("  - Author:            {}\n", a));
            }
            if let Some(ref s) = self.metadata.subject {
                out.push_str(&format!("  - Subject:           {}\n", s));
            }
            if let Some(ref k) = self.metadata.keywords {
                out.push_str(&format!("  - Keywords:          {}\n", k));
            }
        }
        out.push_str("\n[Structure & Layout]\n");
        out.push_str(&format!(
            "  - Paragraphs:        {}\n",
            self.paragraph_count
        ));
        out.push_str(&format!("  - Tables:            {}\n", self.table_count));
        out.push_str(&format!("  - Text Runs:         {}\n", self.run_count));
        out.push_str(&format!(
            "  - Embedded Media:    {} files ({:.2} KiB)\n",
            self.media_count,
            self.media_bytes as f64 / 1024.0
        ));
        out.push_str(&format!(
            "  - Header/Footers:    {} part(s)\n",
            self.header_footer_count
        ));

        out.push_str("\n[Typography & Fonts]\n");
        out.push_str(&format!("  - East Asian Chars:  {}\n", self.cjk_char_count));
        if !self.detected_fonts.is_empty() {
            out.push_str(&format!(
                "  - Declared Fonts:    {}\n",
                self.detected_fonts.join(", ")
            ));
        } else {
            out.push_str("  - Declared Fonts:    (Default style hierarchy)\n");
        }

        if self.missing_glyphs.is_empty() {
            out.push_str("  - Glyph Coverage:    OK (All East Asian characters supported)\n");
        } else {
            out.push_str(&format!(
                "  - Missing Glyphs:    {} glyph(s) missing in fallback font: [{}]\n",
                self.missing_glyphs.len(),
                self.missing_glyphs
                    .iter()
                    .take(10)
                    .map(|c| format!("'{}' (U+{:04X})", c, *c as u32))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        out.push_str("\n[Diagnostics & Risk Evaluation]\n");
        if self.warnings.is_empty() {
            out.push_str("  - Status: READY (No structural or typographic conflicts detected)\n");
        } else {
            out.push_str(&format!(
                "  - Status: {} Warning(s) Detected\n",
                self.warnings.len()
            ));
            for w in &self.warnings {
                out.push_str(&format!("    * [{}] {}\n", w.code, w.message));
            }
        }
        out.push_str("==================================================\n");
        out
    }
}
