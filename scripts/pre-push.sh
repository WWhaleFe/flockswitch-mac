#!/bin/sh
# Pre-push guard: runs, as far as this Mac can, what CI runs (.github/workflows/ci.yml),
# so a failure never reaches GitHub (and the failure mail never reaches a phone).
# Installed as .git/hooks/pre-push (a one-line wrapper that calls this script).
#   - A push of tags only (releases) runs just the Cargo.lock check; the branch was
#     already checked when it was pushed.
#   - Emergency escape hatch: FLOCKSWITCH_SKIP_PREPUSH=1 git push ...
set -e
ROOT=$(git rev-parse --show-toplevel)
cd "$ROOT"
# Git hooks run with a minimal PATH: add the usual cargo locations.
PATH="$HOME/.cargo/bin:$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"
export PATH

if [ "${FLOCKSWITCH_SKIP_PREPUSH:-}" = 1 ]; then
  echo "pre-push: skipped (FLOCKSWITCH_SKIP_PREPUSH=1)"
  exit 0
fi

# git gives one line per pushed ref on stdin: "<local ref> <local sha> <remote ref> <remote sha>".
tags_only=0
if [ ! -t 0 ]; then
  seen=0
  tags_only=1
  while read -r _ _ remote_ref _; do
    seen=1
    case "$remote_ref" in refs/tags/*) ;; *) tags_only=0 ;; esac
  done
  [ "$seen" = 1 ] || tags_only=0
fi

step() { echo "pre-push: $*"; }

step "Cargo.lock matches Cargo.toml"
sh scripts/check-lock.sh
if [ "$tags_only" = 1 ]; then
  step "tags only, slow checks skipped"
  exit 0
fi

# CI also runs `cargo build --release --locked` (slow, LTO); the debug test build
# below compiles the same code. CI does not use -D warnings, so neither does this.
step "cargo test --locked"
cargo test --locked
step "netcore tests"
cargo test --manifest-path netcore/Cargo.toml --locked
step "all checks passed"
