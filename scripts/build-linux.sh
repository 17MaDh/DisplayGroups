#!/usr/bin/env bash
# Checks and builds the Linux parts (ARCHITECTURE.md §16) and packs the command line and
# the settings app into dist-release/: a tarball, a .deb and an AppImage. Run on Ubuntu
# 24.04, or from Windows through WSL:
#   wsl -d Ubuntu-24.04 -- bash scripts/build-linux.sh
#
# Needs:
# - rustup, build-essential, pkg-config, and WebKitGTK (libwebkit2gtk-4.1-dev);
# - the settings app's built page (apps/settings/dist, from `npm ci` and
#   `npm run build` in apps/settings);
# - the Tauri CLI for the .deb and AppImage: `npm ci` in apps/settings provides it, or
#   `cargo install tauri-cli --version <the version in apps/settings/package.json>`.
# `--no-bundle` stops after the tarball.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
BUNDLE=1
[[ "${1:-}" == "--no-bundle" ]] && BUNDLE=0
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)

# From WSL on the Windows checkout (/mnt/c/...), every file looks executable and
# writable by everyone, and packages would keep those modes. Build from a copy in the
# Linux file system, with normal modes, and bring the results back. The copy also keeps
# the slow /mnt/c file system out of the build.
if [[ $PWD == /mnt/* && -z "${DG_BUILD_COPY:-}" ]]; then
  COPY="$HOME/.cache/displaygroups/src"
  echo "==> Copying the checkout to $COPY"
  mkdir -p "$COPY"
  rsync -a --delete --chmod=D755,F644 \
    --exclude=/.git --exclude=/target --exclude=/dist-release --exclude=/.claude \
    --exclude=node_modules --exclude=/apps/settings/src-tauri/binaries \
    --exclude=/apps/settings/src-tauri/gen \
    ./ "$COPY/"
  chmod 755 "$COPY"/scripts/*.sh
  rm -rf "$COPY/dist-release"
  export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/displaygroups/target}"
  DG_BUILD_COPY=1 bash "$COPY/scripts/build-linux.sh" "$@"
  mkdir -p dist-release
  rm -f dist-release/DisplayGroups-"$VERSION"-linux-*
  cp "$COPY"/dist-release/* dist-release/
  ls -l dist-release/DisplayGroups-"$VERSION"-linux-*
  exit 0
fi

TARGET="${CARGO_TARGET_DIR:-target}"
PKGS=(-p dg-model -p dg-identity -p dg-hotkey -p dg-engine -p dg-gnome -p displaygroups
  -p displaygroups-settings)

# The page is the same on every system. From WSL, build it on Windows: running npm here
# on /mnt/c would replace the Windows packages in node_modules.
for page in index identify; do
  if [[ ! -f "apps/settings/dist/$page.html" ]]; then
    echo "apps/settings/dist/$page.html is missing: run 'npm ci' and 'npm run build' in apps/settings first." >&2
    exit 1
  fi
done

echo "==> Checks"
cargo fmt --all -- --check
cargo clippy --locked "${PKGS[@]}" --all-targets -- -D warnings
echo "==> Tests"
cargo test --locked "${PKGS[@]}"
echo "==> Release build"
cargo build --locked --release -p displaygroups -p displaygroups-settings \
  --features displaygroups-settings/custom-protocol

echo "==> Tarball"
NAME="DisplayGroups-$VERSION-linux-x64"
STAGE="$(mktemp -d)/$NAME"
mkdir -p "$STAGE/locales" dist-release
rm -f dist-release/"$NAME".*
install -m 755 "$TARGET/release/displaygroups" "$STAGE/"
install -m 755 "$TARGET/release/displaygroups-settings" "$STAGE/"
install -m 644 locales/*.json "$STAGE/locales/"
install -m 644 README.md "$STAGE/"
install -m 644 LICENSE "$STAGE/LICENSE.txt"
tar -C "$(dirname "$STAGE")" --owner=0 --group=0 --numeric-owner -czf "dist-release/$NAME.tar.gz" "$NAME"
rm -rf "$(dirname "$STAGE")"
echo "dist-release/$NAME.tar.gz"

if [[ $BUNDLE == 1 ]]; then
  echo "==> .deb and AppImage"
  # The command line goes into both packages as a Tauri "external binary", named with
  # the target triple; the bundler installs it as /usr/bin/displaygroups.
  TRIPLE=$(rustc -vV | sed -n 's/^host: //p')
  mkdir -p apps/settings/src-tauri/binaries
  install -m 755 "$TARGET/release/displaygroups" "apps/settings/src-tauri/binaries/displaygroups-$TRIPLE"

  if cargo tauri --version >/dev/null 2>&1; then
    TAURI=(cargo tauri)
  elif [[ -x apps/settings/node_modules/.bin/tauri ]] && command -v node >/dev/null; then
    TAURI=("$PWD/apps/settings/node_modules/.bin/tauri")
  else
    echo "The Tauri CLI is missing: run 'npm ci' in apps/settings, or 'cargo install tauri-cli'." >&2
    exit 1
  fi
  # linuxdeploy (used for the AppImage) is itself an AppImage: run it without FUSE, and
  # skip its strip, which does not know the newer ELF sections of Ubuntu 24.04.
  export APPIMAGE_EXTRACT_AND_RUN=1 NO_STRIP=true
  (cd apps/settings &&
    "${TAURI[@]}" build --bundles deb,appimage --config src-tauri/tauri.release-linux.conf.json)

  BUNDLES="$TARGET/release/bundle"
  install -m 644 "$BUNDLES"/deb/*_"$VERSION"_amd64.deb "dist-release/$NAME.deb"
  install -m 755 "$BUNDLES"/appimage/*_"$VERSION"_amd64.AppImage "dist-release/$NAME.AppImage"
  echo "dist-release/$NAME.deb"
  echo "dist-release/$NAME.AppImage"
fi

(cd dist-release && sha256sum "$NAME".* > "$NAME.sha256")
