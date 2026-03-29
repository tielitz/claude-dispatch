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

config_path="$(pwd)/config.toml"

echo ""
echo "Configuration ($config_path)"
echo "────────────────────"
check "config.toml exists" "$([ -f config.toml ] && echo true || echo false)"
if [ -f config.toml ]; then
    check "  [jira] section" "$(grep -q '^\[jira\]' config.toml && echo true || echo false)"
    check "  [claude] section" "$(grep -q '^\[claude\]' config.toml && echo true || echo false)"
    check "  [paths] section" "$(grep -q '^\[paths\]' config.toml && echo true || echo false)"
    check "  [worktree] section" "$(grep -q '^\[worktree\]' config.toml && echo true || echo false)"
    check "  [tmux] section" "$(grep -q '^\[tmux\]' config.toml && echo true || echo false)"
    check "  [spawner] section" "$(grep -q '^\[spawner\]' config.toml && echo true || echo false)"
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
