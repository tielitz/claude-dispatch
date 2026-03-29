use serde_json::Value;

/// Convert an Atlassian Document Format (ADF) JSON value to a markdown string.
pub fn adf_to_markdown(adf: &Value) -> String {
    let mut out = String::new();
    convert_node(adf, &mut out);
    out.trim().to_string()
}

/// Recursively convert a single ADF node into the output buffer.
fn convert_node(node: &Value, out: &mut String) {
    let node_type = node["type"].as_str().unwrap_or("");

    match node_type {
        "doc" => {
            convert_children(node, out);
        }
        "paragraph" => {
            convert_children(node, out);
            out.push_str("\n\n");
        }
        "text" => {
            if let Some(text) = node["text"].as_str() {
                out.push_str(text);
            }
        }
        "hardBreak" => {
            out.push('\n');
        }
        "heading" => {
            let level = node["attrs"]["level"].as_u64().unwrap_or(2) as usize;
            let hashes = "#".repeat(level);
            out.push_str(&hashes);
            out.push(' ');
            convert_children(node, out);
            out.push_str("\n\n");
        }
        "bulletList" => {
            convert_children(node, out);
        }
        "orderedList" => {
            if let Some(items) = node["content"].as_array() {
                for (i, item) in items.iter().enumerate() {
                    out.push_str(&format!("{}. ", i + 1));
                    convert_list_item_inline(item, out);
                    out.push('\n');
                }
            }
        }
        "listItem" => {
            out.push_str("- ");
            convert_list_item_inline(node, out);
            out.push('\n');
        }
        "codeBlock" => {
            let lang = node["attrs"]["language"].as_str().unwrap_or("");
            out.push_str("```");
            out.push_str(lang);
            out.push('\n');
            // Code content is typically a single text node
            if let Some(children) = node["content"].as_array() {
                for child in children {
                    if child["type"].as_str() == Some("text")
                        && let Some(text) = child["text"].as_str()
                    {
                        out.push_str(text);
                    }
                }
            }
            out.push_str("\n```\n");
        }
        "inlineCard" => {
            if let Some(url) = node["attrs"]["url"].as_str() {
                out.push_str(url);
            }
        }
        "blockquote" => {
            let mut inner = String::new();
            convert_children(node, &mut inner);
            for line in inner.lines() {
                out.push_str("> ");
                out.push_str(line);
                out.push('\n');
            }
        }
        _ => {
            // Unknown node type — just process children
            convert_children(node, out);
        }
    }
}

/// Process the `.content` array of a node, converting each child.
fn convert_children(node: &Value, out: &mut String) {
    if let Some(children) = node["content"].as_array() {
        for child in children {
            convert_node(child, out);
        }
    }
}

/// Extract inline text from a list item's children without adding paragraph breaks.
/// A list item typically contains paragraph nodes; we want their text without the trailing "\n\n".
fn convert_list_item_inline(node: &Value, out: &mut String) {
    if let Some(children) = node["content"].as_array() {
        for child in children {
            let child_type = child["type"].as_str().unwrap_or("");
            if child_type == "paragraph" {
                // Process paragraph children directly (skip the trailing "\n\n")
                convert_children(child, out);
            } else {
                convert_node(child, out);
            }
        }
    }
}
