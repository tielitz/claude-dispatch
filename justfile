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
    cargo run

run-mark-done ticket:
    cargo run -- mark-done {{ticket}}

test:
    cargo test

check:
    cargo clippy -- -D warnings && cargo fmt --check

fmt:
    cargo fmt

attach:
    tmux attach -t "$(grep 'session_name' config.toml | head -1 | sed 's/.*= *"\(.*\)"/\1/')"

# Build a release tarball locally to dry-run the release workflow's packaging step.
package-local target:
    #!/usr/bin/env bash
    set -euo pipefail
    version=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)"/\1/')
    stage="claude-dispatch-v${version}-{{target}}"
    rustup target add {{target}}
    cargo build --release --locked --target {{target}}
    rm -rf "dist/$stage"
    mkdir -p "dist/$stage"
    cp "target/{{target}}/release/claude-dispatch" "dist/$stage/"
    cp README.md LICENSE config.example.toml "dist/$stage/"
    tar -C dist -czf "dist/${stage}.tar.gz" "$stage"
    ( cd dist && shasum -a 256 "${stage}.tar.gz" > "${stage}.tar.gz.sha256" )
    ls -l dist/

clean:
    cargo clean
    rm -rf dist
