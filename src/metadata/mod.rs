//! Document metadata model, extraction, and injection for PDF properties.
//! License: GPL-3.0-or-later

pub mod doc;
pub mod docx;

use crate::format::{DOC_MAGIC, ZIP_MAGIC};

/// Standard document metadata attributes injected into PDF Info and XMP.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentMetadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub keywords: Option<String>,
}

impl DocumentMetadata {
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.author.is_none()
            && self.subject.is_none()
            && self.keywords.is_none()
    }

    /// Merge overrides on top of existing metadata (overrides take precedence).
    pub fn merge_with_overrides(&self, overrides: &DocumentMetadata) -> DocumentMetadata {
        DocumentMetadata {
            title: overrides.title.clone().or_else(|| self.title.clone()),
            author: overrides.author.clone().or_else(|| self.author.clone()),
            subject: overrides.subject.clone().or_else(|| self.subject.clone()),
            keywords: overrides.keywords.clone().or_else(|| self.keywords.clone()),
        }
    }
}

/// Extract metadata from document bytes (detects DOC or DOCX automatically).
pub fn extract_metadata(bytes: &[u8]) -> DocumentMetadata {
    if bytes.starts_with(&ZIP_MAGIC) {
        docx::extract_docx_metadata(bytes)
    } else if bytes.starts_with(&DOC_MAGIC) {
        doc::extract_doc_metadata(bytes)
    } else {
        DocumentMetadata::default()
    }
}
