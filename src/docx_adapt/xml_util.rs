//! Low-level XML inspection, bracket scanning, attribute manipulation, and text node traversal.
//! License: GPL-3.0-or-later

use crate::font::count_cjk_chars;

/// Determines if the given ZIP entry is a Word text-bearing XML part.
pub fn is_text_part(name: &str) -> bool {
    name == "word/document.xml"
        || name.starts_with("word/header")
        || name.starts_with("word/footer")
        || name == "word/footnotes.xml"
        || name == "word/endnotes.xml"
        || name == "word/comments.xml"
}

/// Find the index of the matching '>' tag closing bracket in `slice`,
/// ignoring any '>' that appear inside quoted attribute strings.
pub fn find_tag_closing_bracket(slice: &str) -> Option<usize> {
    let mut in_quote = None;
    for (i, b) in slice.bytes().enumerate() {
        match b {
            b'"' | b'\'' => {
                if in_quote == Some(b) {
                    in_quote = None;
                } else if in_quote.is_none() {
                    in_quote = Some(b);
                }
            }
            b'>' if in_quote.is_none() => return Some(i),
            _ => {}
        }
    }
    None
}

/// Traverse all XML text nodes (<w:t>, <w:tblText>), skipping comments and CDATA.
pub fn for_each_xml_text_node<F>(xml: &str, mut f: F)
where
    F: FnMut(&str),
{
    let bytes = xml.as_bytes();
    let mut cursor = 0;

    while cursor < bytes.len() {
        let Some(rel_start) = xml[cursor..].find('<') else {
            break;
        };
        let tag_start = cursor + rel_start;

        // Skip comments <!-- ... -->
        if xml[tag_start..].starts_with("<!--") {
            if let Some(comment_end) = xml[tag_start + 4..].find("-->") {
                cursor = tag_start + 4 + comment_end + 3;
                continue;
            } else {
                break;
            }
        }

        // Skip CDATA <![CDATA[ ... ]]>
        if xml[tag_start..].starts_with("<![CDATA[") {
            if let Some(cdata_end) = xml[tag_start + 9..].find("]]>") {
                cursor = tag_start + 9 + cdata_end + 3;
                continue;
            } else {
                break;
            }
        }

        let rem = &xml[tag_start..];
        let (prefix_len, close_tag) = if rem.starts_with("<w:tblText") {
            (10, "</w:tblText>")
        } else if rem.starts_with("<w:t") {
            (4, "</w:t>")
        } else {
            cursor = tag_start + 1;
            continue;
        };

        let byte_after = rem.as_bytes().get(prefix_len);
        let is_valid_tag = matches!(byte_after, Some(b'>' | b' ' | b'\t' | b'\r' | b'\n' | b'/'));
        if !is_valid_tag {
            cursor = tag_start + prefix_len;
            continue;
        }

        let Some(tag_end_rel) = find_tag_closing_bracket(&rem[prefix_len..]) else {
            break;
        };
        let abs_tag_end = tag_start + prefix_len + tag_end_rel;
        let open_tag = &xml[tag_start..=abs_tag_end];

        if open_tag.trim_end().ends_with("/>") {
            cursor = abs_tag_end + 1;
            continue;
        }

        let content_start = abs_tag_end + 1;
        if let Some(close_rel) = xml[content_start..].find(close_tag) {
            let content_end = content_start + close_rel;
            f(&xml[content_start..content_end]);
            cursor = content_end + close_tag.len();
        } else {
            break;
        }
    }
}

/// Count CJK characters across all text nodes in XML snippet.
pub fn count_cjk_in_xml_text_nodes(xml: &str) -> usize {
    let mut count = 0;
    for_each_xml_text_node(xml, |text| {
        count += count_cjk_chars(text);
    });
    count
}

/// Extracts the value of an XML attribute in `tag`, if present.
pub fn extract_attribute_value<'a>(tag: &'a str, attr_name: &str) -> Option<&'a str> {
    let bytes = tag.as_bytes();
    let mut cursor = 0;

    while let Some(rel_pos) = tag[cursor..].find(attr_name) {
        let abs_pos = cursor + rel_pos;
        let char_before_ok =
            abs_pos == 0 || matches!(bytes[abs_pos - 1], b' ' | b'\t' | b'\r' | b'\n');

        if char_before_ok {
            let mut check_pos = abs_pos + attr_name.len();
            while check_pos < bytes.len()
                && matches!(bytes[check_pos], b' ' | b'\t' | b'\r' | b'\n')
            {
                check_pos += 1;
            }
            if check_pos < bytes.len() && bytes[check_pos] == b'=' {
                check_pos += 1;
                while check_pos < bytes.len()
                    && matches!(bytes[check_pos], b' ' | b'\t' | b'\r' | b'\n')
                {
                    check_pos += 1;
                }
                if check_pos < bytes.len()
                    && (bytes[check_pos] == b'"' || bytes[check_pos] == b'\'')
                {
                    let quote_char = bytes[check_pos];
                    let val_start = check_pos + 1;
                    if let Some(val_end_rel) = tag[val_start..].find(quote_char as char) {
                        return Some(&tag[val_start..val_start + val_end_rel]);
                    }
                }
            }
        }
        cursor = abs_pos + attr_name.len();
    }
    None
}

/// Escapes string for safe inclusion in XML attribute values.
pub fn escape_xml_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// Updates an existing XML attribute or inserts it before the closing `>` or `/>`.
pub fn update_or_insert_attribute(tag: &str, attr_name: &str, new_value: &str) -> String {
    let escaped_val = escape_xml_attr(new_value);
    let new_value = &escaped_val;
    let bytes = tag.as_bytes();
    let mut cursor = 0;
    let mut existing_val_span: Option<(usize, usize)> = None;

    while let Some(rel_pos) = tag[cursor..].find(attr_name) {
        let abs_pos = cursor + rel_pos;
        let char_before_ok = if abs_pos == 0 {
            false
        } else {
            matches!(bytes[abs_pos - 1], b' ' | b'\t' | b'\r' | b'\n')
        };

        if char_before_ok {
            let mut check_pos = abs_pos + attr_name.len();
            while check_pos < bytes.len()
                && matches!(bytes[check_pos], b' ' | b'\t' | b'\r' | b'\n')
            {
                check_pos += 1;
            }
            if check_pos < bytes.len() && bytes[check_pos] == b'=' {
                check_pos += 1;
                while check_pos < bytes.len()
                    && matches!(bytes[check_pos], b' ' | b'\t' | b'\r' | b'\n')
                {
                    check_pos += 1;
                }
                if check_pos < bytes.len()
                    && (bytes[check_pos] == b'"' || bytes[check_pos] == b'\'')
                {
                    let quote_char = bytes[check_pos];
                    let val_start = check_pos + 1;
                    if let Some(val_end_rel) = tag[val_start..].find(quote_char as char) {
                        let val_end = val_start + val_end_rel;
                        existing_val_span = Some((abs_pos, val_end + 1));
                        break;
                    }
                }
            }
        }
        cursor = abs_pos + attr_name.len();
    }

    if let Some((attr_start, attr_end)) = existing_val_span {
        let mut res = String::with_capacity(tag.len() + attr_name.len() + new_value.len() + 4);
        res.push_str(&tag[..attr_start]);
        res.push_str(attr_name);
        res.push_str("=\"");
        res.push_str(new_value);
        res.push('"');
        res.push_str(&tag[attr_end..]);
        res
    } else {
        let trimmed = tag.trim_end();
        if let Some(stripped) = trimmed.strip_suffix("/>") {
            let prefix = stripped.trim_end();
            let mut res =
                String::with_capacity(prefix.len() + attr_name.len() + new_value.len() + 6);
            res.push_str(prefix);
            res.push(' ');
            res.push_str(attr_name);
            res.push_str("=\"");
            res.push_str(new_value);
            res.push_str("\"/>");
            res
        } else if let Some(stripped) = trimmed.strip_suffix('>') {
            let prefix = stripped.trim_end();
            let mut res =
                String::with_capacity(prefix.len() + attr_name.len() + new_value.len() + 6);
            res.push_str(prefix);
            res.push(' ');
            res.push_str(attr_name);
            res.push_str("=\"");
            res.push_str(new_value);
            res.push_str("\" >");
            res
        } else {
            let mut res = String::with_capacity(tag.len() + attr_name.len() + new_value.len() + 5);
            res.push_str(tag);
            res.push(' ');
            res.push_str(attr_name);
            res.push_str("=\"");
            res.push_str(new_value);
            res.push('"');
            res
        }
    }
}
