use claude_dispatch::jira::adf::adf_to_markdown;
use serde_json::{Value, json};

#[test]
fn test_simple_paragraph() {
    let adf = json!({
        "type": "doc",
        "content": [
            {
                "type": "paragraph",
                "content": [
                    { "type": "text", "text": "Hello world" }
                ]
            }
        ]
    });
    assert_eq!(adf_to_markdown(&adf), "Hello world");
}

#[test]
fn test_heading() {
    let adf = json!({
        "type": "doc",
        "content": [
            {
                "type": "heading",
                "attrs": { "level": 2 },
                "content": [
                    { "type": "text", "text": "My Heading" }
                ]
            }
        ]
    });
    let result = adf_to_markdown(&adf);
    assert_eq!(result, "## My Heading");
}

#[test]
fn test_bullet_list() {
    let adf = json!({
        "type": "doc",
        "content": [
            {
                "type": "bulletList",
                "content": [
                    {
                        "type": "listItem",
                        "content": [
                            {
                                "type": "paragraph",
                                "content": [
                                    { "type": "text", "text": "Item one" }
                                ]
                            }
                        ]
                    },
                    {
                        "type": "listItem",
                        "content": [
                            {
                                "type": "paragraph",
                                "content": [
                                    { "type": "text", "text": "Item two" }
                                ]
                            }
                        ]
                    }
                ]
            }
        ]
    });
    let result = adf_to_markdown(&adf);
    assert_eq!(result, "- Item one\n- Item two");
}

#[test]
fn test_code_block() {
    let adf = json!({
        "type": "doc",
        "content": [
            {
                "type": "codeBlock",
                "attrs": { "language": "rust" },
                "content": [
                    { "type": "text", "text": "fn main() {}" }
                ]
            }
        ]
    });
    let result = adf_to_markdown(&adf);
    assert!(
        result.contains("```rust"),
        "Expected opening fence with language"
    );
    assert!(result.contains("fn main() {}"), "Expected code content");
}

#[test]
fn test_inline_card() {
    let adf = json!({
        "type": "doc",
        "content": [
            {
                "type": "paragraph",
                "content": [
                    {
                        "type": "inlineCard",
                        "attrs": { "url": "https://example.com/ticket/123" }
                    }
                ]
            }
        ]
    });
    let result = adf_to_markdown(&adf);
    assert_eq!(result, "https://example.com/ticket/123");
}

#[test]
fn test_empty_doc() {
    let adf = json!({
        "type": "doc",
        "content": []
    });
    assert_eq!(adf_to_markdown(&adf), "");
}

#[test]
fn test_null_value() {
    assert_eq!(adf_to_markdown(&Value::Null), "");
}
