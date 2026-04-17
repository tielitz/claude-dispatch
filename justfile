default:
    just --list

# Check if all required tools and configuration are available
doctor:
    ./scripts/doctor.sh

setup:
    git config core.hooksPath hooks

build:
    cargo build

release:
    cargo build --release

run:
    cargo run -- -c config.toml

run-mark-done ticket:
    cargo run -- -c config.toml mark-done {{ticket}}

# Write a fresh config template to the per-OS user config dir.
init-config:
    cargo run -- --init

test:
    cargo test

check:
    cargo clippy -- -D warnings && cargo fmt --check

fmt:
    cargo fmt

attach:
    tmux attach -t "$(grep 'session_name' config.toml | head -1 | sed 's/.*= *"\(.*\)"/\1/')"

clean:
    cargo clean
