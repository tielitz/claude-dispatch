#!/usr/bin/env bash
set -uo pipefail

passed=0
failed=0

check() {
    local label="$1"
    local ok="$2"
    if [ "$ok" = "true" ]; then
        printf "  \033[32m✔\033[0m %s\n" "$label"
        ((passed++))
    else
        printf "  \033[31m✘\033[0m %s\n" "$label"
        ((failed++))
    fi
}

echo ""
echo "Runtime dependencies"
echo "────────────────────"
check "rust toolchain (cargo)" "$(command -v cargo >/dev/null 2>&1 && echo true || echo false)"
check "claude CLI" "$(command -v claude >/dev/null 2>&1 && echo true || echo false)"
check "tmux" "$(command -v tmux >/dev/null 2>&1 && echo true || echo false)"
check "git" "$(command -v git >/dev/null 2>&1 && echo true || echo false)"
check "just" "$(command -v just >/dev/null 2>&1 && echo true || echo false)"

# Per-OS user config path (mirrors `directories::ProjectDirs` output).
if [[ "$OSTYPE" == "darwin"* ]]; then
    USER_CFG="$HOME/Library/Application Support/dev.claude-dispatch.claude-dispatch/config.toml"
elif [[ "$OSTYPE" == "linux"* ]]; then
    USER_CFG="${XDG_CONFIG_HOME:-$HOME/.config}/claude-dispatch/config.toml"
else
    USER_CFG=""
fi

DEV_CFG="$(pwd)/config.toml"

echo ""
echo "Configuration"
echo "────────────────────"
if [ -n "$USER_CFG" ]; then
    check "user config: $USER_CFG" "$([ -f "$USER_CFG" ] && echo true || echo false)"
fi
check "dev config ($DEV_CFG, for 'just run')" "$([ -f "$DEV_CFG" ] && echo true || echo false)"

# Pick whichever exists to run section checks against.
ACTIVE_CFG=""
if [ -f "$DEV_CFG" ]; then
    ACTIVE_CFG="$DEV_CFG"
elif [ -n "$USER_CFG" ] && [ -f "$USER_CFG" ]; then
    ACTIVE_CFG="$USER_CFG"
fi

if [ -n "$ACTIVE_CFG" ]; then
    echo "  Checking keys in: $ACTIVE_CFG"
    check "  schema_version" "$(grep -q '^schema_version' "$ACTIVE_CFG" && echo true || echo false)"
    check "  [jira] section" "$(grep -q '^\[jira\]' "$ACTIVE_CFG" && echo true || echo false)"
    check "  [claude] section" "$(grep -q '^\[claude\]' "$ACTIVE_CFG" && echo true || echo false)"
    check "  [paths] section" "$(grep -q '^\[paths\]' "$ACTIVE_CFG" && echo true || echo false)"
    check "  [worktree] section" "$(grep -q '^\[worktree\]' "$ACTIVE_CFG" && echo true || echo false)"
    check "  [tmux] section" "$(grep -q '^\[tmux\]' "$ACTIVE_CFG" && echo true || echo false)"
    check "  [spawner] section" "$(grep -q '^\[spawner\]' "$ACTIVE_CFG" && echo true || echo false)"
fi

echo ""
echo "Git hooks"
echo "────────────────────"
check "hooksPath set to ./hooks" "$([ "$(git config core.hooksPath)" = "hooks" ] && echo true || echo false)"
check "pre-commit hook exists" "$([ -x hooks/pre-commit ] && echo true || echo false)"

echo ""
echo "Build"
echo "────────────────────"
check "project compiles (cargo check)" "$(cargo check --quiet 2>/dev/null && echo true || echo false)"

echo ""
echo "────────────────────"
printf "Results: \033[32m%d passed\033[0m, \033[31m%d failed\033[0m\n" "$passed" "$failed"
echo ""
if [ "$failed" -gt 0 ]; then
    exit 1
fi
