/// Escape plain text for a Markdown table cell.
///
/// Newlines are collapsed so one logical cell cannot split the table row.
pub(crate) fn table_cell(text: &str) -> String {
    escape_plain(&normalize_cell_text(text))
}

/// Render text as an HTML code element that is safe inside a Markdown table cell.
///
/// HTML entities preserve the displayed payload while keeping `|`, `<`, `>`,
/// and `&` from changing the table or HTML structure.
pub(crate) fn code_cell(text: &str) -> String {
    format!(
        "<code>{}</code>",
        escape_html_code(&normalize_cell_text(text))
    )
}

fn normalize_cell_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last_was_space = false;
    for ch in text.chars() {
        match ch {
            '\r' | '\n' => {
                if !last_was_space {
                    out.push(' ');
                    last_was_space = true;
                }
            }
            _ => {
                out.push(ch);
                last_was_space = ch == ' ';
            }
        }
    }
    out
}

fn escape_plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '\\' => out.push_str("\\\\"),
            '|' => out.push_str("\\|"),
            '`' => out.push_str("\\`"),
            '[' | ']' | '*' | '_' | '~' | '!' => {
                out.push('\\');
                out.push(ch);
            }
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    out
}

fn escape_html_code(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '|' => out.push_str("&#124;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{code_cell, table_cell};

    #[test]
    fn table_cell_escapes_row_and_inline_markdown_controls() {
        assert_eq!(
            table_cell("a|b\n`code` [x] <tag> &lt;tag&gt; *em*"),
            "a\\|b \\`code\\` \\[x\\] &lt;tag&gt; &amp;lt;tag&amp;gt; \\*em\\*"
        );
    }

    #[test]
    fn code_cell_preserves_payload_and_protects_table_shape() {
        assert_eq!(
            code_cell(".github/workflows/release_job.yml|`tick` & <tag>"),
            "<code>.github/workflows/release_job.yml&#124;`tick` &amp; &lt;tag&gt;</code>"
        );
    }
}
