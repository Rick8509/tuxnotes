//! Turns a TipTap/ProseMirror JSON document into plain text, a title and a preview snippet.

use serde_json::Value;

/// Maximum characters kept for a note title.
pub const MAX_TITLE_CHARS: usize = 200;
/// Maximum characters kept for the note-list preview line.
pub const MAX_SNIPPET_CHARS: usize = 160;

/// Node types that end a line of text.
const BLOCK_NODES: &[&str] = &[
    "paragraph",
    "heading",
    "listItem",
    "taskItem",
    "codeBlock",
    "blockquote",
    "horizontalRule",
    "tableRow",
    "image",
    "attachment",
];

/// Node types whose text is separated by a space (table cells on the same row).
const INLINE_SEPARATED_NODES: &[&str] = &["tableCell", "tableHeader"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedText {
    /// First non-empty line.
    pub title: String,
    /// The following non-empty lines joined with spaces.
    pub snippet: String,
    /// All text, one block per line. Used for full-text search.
    pub plain: String,
}

pub fn extract(doc: &Value) -> ExtractedText {
    let mut raw = String::new();
    walk(doc, &mut raw, false);

    let mut lines = raw.lines().map(str::trim).filter(|line| !line.is_empty());
    let title = truncate_chars(lines.next().unwrap_or(""), MAX_TITLE_CHARS);
    let snippet = truncate_chars(&lines.collect::<Vec<_>>().join(" "), MAX_SNIPPET_CHARS);

    let plain = raw
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    ExtractedText {
        title,
        snippet,
        plain,
    }
}

/// `in_cell`: inside a table cell, blocks are separated by spaces so each
/// table row reads as one line ("Milestone Status", "M1 Done").
fn walk(node: &Value, out: &mut String, in_cell: bool) {
    if let Some(text) = node.get("text").and_then(Value::as_str) {
        out.push_str(text);
    }
    let node_type = node.get("type").and_then(Value::as_str).unwrap_or("");
    // An attached file's name is part of the note's text, so search finds it.
    if node_type == "attachment"
        && let Some(name) = node
            .get("attrs")
            .and_then(|a| a.get("filename"))
            .and_then(Value::as_str)
    {
        out.push_str(name);
    }
    if node_type == "hardBreak" {
        out.push(if in_cell { ' ' } else { '\n' });
    }
    let is_cell = INLINE_SEPARATED_NODES.contains(&node_type);
    if let Some(children) = node.get("content").and_then(Value::as_array) {
        for child in children {
            walk(child, out, in_cell || is_cell);
        }
    }
    let ends_line = BLOCK_NODES.contains(&node_type) && !(in_cell && node_type != "tableRow");
    if ends_line {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
    } else if (is_cell || BLOCK_NODES.contains(&node_type))
        && !out.is_empty()
        && !out.ends_with([' ', '\n'])
    {
        out.push(' ');
    }
}

/// Truncate to at most `max` characters (not bytes), adding an ellipsis when cut.
pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut cut: String = s.chars().take(max.saturating_sub(1)).collect();
    cut.truncate(cut.trim_end().len());
    cut.push('…');
    cut
}

/// True when a document has nothing worth keeping: only empty paragraphs/headings.
/// Deliberately strict — any text, list, checklist, rule or other node counts as content.
pub fn is_blank(doc: &Value) -> bool {
    fn walk(node: &Value) -> bool {
        match node.get("type").and_then(Value::as_str) {
            Some("doc" | "paragraph" | "heading" | "hardBreak") | None => {}
            Some(_) => return false,
        }
        if node.get("text").is_some() {
            return false;
        }
        node.get("content")
            .and_then(Value::as_array)
            .is_none_or(|children| children.iter().all(walk))
    }
    walk(doc)
}

/// An empty document, used for new notes.
pub fn empty_doc() -> Value {
    serde_json::json!({ "type": "doc", "content": [{ "type": "paragraph" }] })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn para(text: &str) -> Value {
        json!({ "type": "paragraph", "content": [{ "type": "text", "text": text }] })
    }

    #[test]
    fn empty_document_has_no_title() {
        let out = extract(&empty_doc());
        assert_eq!(out.title, "");
        assert_eq!(out.snippet, "");
        assert_eq!(out.plain, "");
    }

    #[test]
    fn title_is_first_non_empty_line_and_snippet_is_the_rest() {
        let doc = json!({ "type": "doc", "content": [
            { "type": "paragraph" },
            { "type": "heading", "attrs": { "level": 1 }, "content": [{ "type": "text", "text": "  Groceries " }] },
            para("milk"),
            { "type": "paragraph" },
            para("eggs"),
        ]});
        let out = extract(&doc);
        assert_eq!(out.title, "Groceries");
        assert_eq!(out.snippet, "milk eggs");
        assert_eq!(out.plain, "  Groceries\nmilk\neggs");
    }

    #[test]
    fn marks_do_not_split_words() {
        let doc = json!({ "type": "doc", "content": [{ "type": "paragraph", "content": [
            { "type": "text", "text": "Hel" },
            { "type": "text", "text": "lo", "marks": [{ "type": "bold" }] },
            { "type": "text", "text": " world" },
        ]}]});
        assert_eq!(extract(&doc).title, "Hello world");
    }

    #[test]
    fn lists_tasks_and_hard_breaks_become_lines() {
        let doc = json!({ "type": "doc", "content": [
            para("Todo"),
            { "type": "taskList", "content": [
                { "type": "taskItem", "attrs": { "checked": true }, "content": [para("first")] },
                { "type": "taskItem", "attrs": { "checked": false }, "content": [para("second")] },
            ]},
            { "type": "paragraph", "content": [
                { "type": "text", "text": "a" }, { "type": "hardBreak" }, { "type": "text", "text": "b" },
            ]},
        ]});
        assert_eq!(extract(&doc).plain, "Todo\nfirst\nsecond\na\nb");
    }

    #[test]
    fn table_rows_read_as_lines() {
        // The shape TipTap produces: table > tableRow > tableHeader/tableCell > paragraph.
        let cell = |kind: &str, t: &str| {
            json!({ "type": kind, "attrs": { "colspan": 1, "rowspan": 1, "colwidth": null },
                    "content": [if t.is_empty() { json!({ "type": "paragraph" }) } else { para(t) }] })
        };
        let doc = json!({ "type": "doc", "content": [
            para("Plan"),
            { "type": "table", "content": [
                { "type": "tableRow", "content": [cell("tableHeader", "Milestone"), cell("tableHeader", "Status")] },
                { "type": "tableRow", "content": [cell("tableCell", "M1"), cell("tableCell", "Done")] },
                { "type": "tableRow", "content": [cell("tableCell", ""), cell("tableCell", "Later")] },
            ]},
            para("After the table"),
        ]});
        let out = extract(&doc);
        assert_eq!(
            out.plain,
            "Plan\nMilestone Status\nM1 Done\nLater\nAfter the table"
        );
        assert_eq!(out.title, "Plan");
        assert_eq!(
            out.snippet,
            "Milestone Status M1 Done Later After the table"
        );
    }

    #[test]
    fn a_table_first_gives_its_header_row_as_title() {
        let cell = |t: &str| json!({ "type": "tableCell", "content": [para(t)] });
        let doc = json!({ "type": "doc", "content": [{ "type": "table", "content": [
            { "type": "tableRow", "content": [cell("Name"), cell("Phone")] },
            { "type": "tableRow", "content": [cell("Tux"), cell("123")] },
        ]}]});
        let out = extract(&doc);
        assert_eq!(out.title, "Name Phone");
        assert_eq!(out.snippet, "Tux 123");
    }

    #[test]
    fn attachments_contribute_their_file_name() {
        let doc = json!({ "type": "doc", "content": [
            para("Trip"),
            { "type": "image", "attrs": { "attachmentId": "a" } },
            { "type": "attachment", "attrs": { "attachmentId": "b", "filename": "tickets.pdf", "size": 10 } },
            para("Pack light"),
        ]});
        let out = extract(&doc);
        assert_eq!(out.plain, "Trip\ntickets.pdf\nPack light");
        assert!(!is_blank(
            &json!({ "type": "doc", "content": [{ "type": "image", "attrs": { "attachmentId": "a" } }] })
        ));
    }

    #[test]
    fn an_empty_table_is_content() {
        let doc = json!({ "type": "doc", "content": [{ "type": "table", "content": [
            { "type": "tableRow", "content": [{ "type": "tableCell", "content": [{ "type": "paragraph" }] }] },
        ]}]});
        assert!(!is_blank(&doc));
    }

    #[test]
    fn long_titles_are_truncated_on_char_boundaries() {
        let long = "é".repeat(500);
        let out = extract(&json!({ "type": "doc", "content": [para(&long)] }));
        assert_eq!(out.title.chars().count(), MAX_TITLE_CHARS);
        assert!(out.title.ends_with('…'));
    }

    #[test]
    fn blank_detection_is_strict() {
        assert!(is_blank(&empty_doc()));
        assert!(is_blank(&json!({ "type": "doc", "content": [
            { "type": "paragraph" },
            { "type": "heading", "attrs": { "level": 1 } },
            { "type": "paragraph", "content": [{ "type": "hardBreak" }] },
        ]})));
        assert!(
            !is_blank(&json!({ "type": "doc", "content": [para(" ")] })),
            "whitespace is kept"
        );
        assert!(!is_blank(&json!({ "type": "doc", "content": [para("x")] })));
        let empty_checklist = json!({ "type": "doc", "content": [{ "type": "taskList", "content": [
            { "type": "taskItem", "attrs": { "checked": false }, "content": [{ "type": "paragraph" }] }
        ]}]});
        assert!(
            !is_blank(&empty_checklist),
            "an empty checklist is still content"
        );
        assert!(!is_blank(
            &json!({ "type": "doc", "content": [{ "type": "horizontalRule" }] })
        ));
    }

    #[test]
    fn unknown_or_malformed_nodes_are_ignored() {
        let doc =
            json!({ "type": "doc", "content": [42, "x", null, { "foo": "bar" }, para("ok")] });
        assert_eq!(extract(&doc).title, "ok");
    }
}
