#!/usr/bin/env bash
# Everything that must pass before a commit. Fails on the first problem —
# `set -e` rather than a pipeline whose exit code can be swallowed by `head`.
set -euo pipefail

echo "== fmt ==";     cargo fmt --all --check
echo "== clippy ==";  cargo clippy --workspace --all-targets -- -D warnings
echo "== test ==";    cargo test --workspace
echo "== all green =="
