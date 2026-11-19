#!/usr/bin/env bash
# Checks and builds the Linux parts (ARCHITECTURE.md §16) and packs the command line
# into dist-release/. Run on Ubuntu 24.04, or from Windows through WSL:
#   wsl -d Ubuntu-24.04 -- bash scripts/build-linux.sh
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

# Building into the Windows checkout through /mnt/c is slow and would mix with the
# Windows target folder: keep Linux build output inside the Linux file system.
if [[ $PWD == /mnt/* && -z "${CARGO_TARGET_DIR:-}" ]]; then
  export CARGO_TARGET_DIR="$HOME/.cache/displaygroups/target"
fi
TARGET="${CARGO_TARGET_DIR:-target}"
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
PKGS=(-p dg-model -p dg-identity -p dg-hotkey -p dg-engine -p dg-gnome -p displaygroups)

echo "==> Checks"
cargo fmt --all -- --check
cargo clippy --locked "${PKGS[@]}" --all-targets -- -D warnings
echo "==> Tests"
cargo test --locked "${PKGS[@]}"
echo "==> Release build"
cargo build --locked --release -p displaygroups

echo "==> Package"
NAME="DisplayGroups-$VERSION-linux-x64-cli"
STAGE="$(mktemp -d)/$NAME"
mkdir -p "$STAGE/locales" dist-release
# Explicit modes: files read through /mnt/c all look executable.
install -m 755 "$TARGET/release/displaygroups" "$STAGE/"
install -m 644 locales/*.json "$STAGE/locales/"
install -m 644 README.md "$STAGE/"
install -m 644 LICENSE "$STAGE/LICENSE.txt"
tar -C "$(dirname "$STAGE")" --owner=0 --group=0 --numeric-owner -czf "dist-release/$NAME.tar.gz" "$NAME"
(cd dist-release && sha256sum "$NAME.tar.gz" > "$NAME.tar.gz.sha256")
rm -rf "$(dirname "$STAGE")"
echo "dist-release/$NAME.tar.gz"
