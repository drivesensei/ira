#!/bin/bash
# ira-install — install or update IRA from the latest GitHub release.
#
# Runs without sudo: the binary lands in ~/.local/bin (already on PATH in
# Omarchy) and a launcher entry goes to ~/.local/share/applications, so IRA
# also shows up in the Omarchy app menu as a terminal application.
#
# Used by the Omarchy bar plugin (omarchy-plugin/Ira.qml), and usable on its own:
#
#   omarchy plugin add https://github.com/drivesensei/ira --enable
#   ~/.config/omarchy/plugins/drivesensei.ira/omarchy-plugin/install.sh
#
# Overrides: IRA_REPO, IRA_INSTALL_DIR, IRA_DESKTOP_DIR.

set -euo pipefail

REPO="${IRA_REPO:-drivesensei/ira}"
BIN_DIR="${IRA_INSTALL_DIR:-$HOME/.local/bin}"
APPS_DIR="${IRA_DESKTOP_DIR:-$HOME/.local/share/applications}"

die() {
  printf 'ira-install: %s\n' "$*" >&2
  exit 1
}

for tool in curl jq tar; do
  command -v "$tool" >/dev/null 2>&1 || die "$tool is required but not installed"
done

case "$(uname -m)" in
x86_64 | amd64) asset_arch="x86_64-unknown-linux-musl" ;;
aarch64 | arm64) asset_arch="aarch64-unknown-linux-musl" ;;
*) die "no prebuilt release for $(uname -m); build from source with 'cargo build --release'" ;;
esac

json="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest")" ||
  die "could not reach the GitHub API for $REPO"

tag="$(jq -r '.tag_name // empty' <<<"$json")"
[[ -n $tag ]] || die "the latest release of $REPO has no tag"

asset_url="$(jq -r --arg suffix "-$asset_arch.tar.xz" \
  '.assets[] | select(.name | endswith($suffix)) | .browser_download_url' <<<"$json" | head -n1)"
[[ -n $asset_url ]] || die "release $tag has no $asset_arch asset"

sums_url="$(jq -r '.assets[] | select(.name == "SHA256SUMS") | .browser_download_url' <<<"$json" | head -n1)"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

printf 'Downloading ira %s (%s)…\n' "${tag#v}" "$asset_arch"
curl -fsSL "$asset_url" -o "$work/ira.tar.xz" || die "download failed"

# The release publishes SHA256SUMS next to the binaries; verify when present so
# a truncated or substituted download never reaches ~/.local/bin.
if [[ -n $sums_url ]]; then
  curl -fsSL "$sums_url" -o "$work/SHA256SUMS" || die "checksum download failed"
  expected="$(awk -v name="$(basename "$asset_url")" '
    { file = $2; sub(/^\*/, "", file); sub(/^\.\//, "", file) }
    file == name { print $1; exit }
  ' "$work/SHA256SUMS")"
  [[ -n $expected ]] || die "no checksum for $(basename "$asset_url") in SHA256SUMS"
  actual="$(sha256sum "$work/ira.tar.xz" | cut -d' ' -f1)"
  [[ $expected == "$actual" ]] || die "checksum mismatch for $(basename "$asset_url")"
fi

tar -xJf "$work/ira.tar.xz" -C "$work" ira || die "could not extract $(basename "$asset_url")"
[[ -f "$work/ira" ]] || die "the archive did not contain an ira binary"

install -Dm755 "$work/ira" "$BIN_DIR/ira" || die "could not install into $BIN_DIR"

# Launcher entry. The app id matches the one `omarchy-launch-tui ira` assigns,
# so the bar widget, this menu entry, and `omarchy-launch-or-focus-tui ira`
# all resolve to the same window.
mkdir -p "$APPS_DIR"
cat >"$APPS_DIR/ira.desktop" <<'DESKTOP'
[Desktop Entry]
Version=1.0
Name=IRA
Comment=Integrated Retro Archives — terminal file manager
Exec=xdg-terminal-exec --app-id=org.omarchy.ira -e ira
Terminal=false
Type=Application
Icon=folder
Categories=System;FileTools;FileManager;
StartupNotify=true
DESKTOP

printf 'Installed ira %s → %s/ira\n' "${tag#v}" "$BIN_DIR"
printf 'Launcher entry → %s/ira.desktop\n' "$APPS_DIR"

case ":$PATH:" in
*":$BIN_DIR:"*) ;;
*) printf '\nNote: %s is not on PATH — add it to run \047ira\047 from a shell.\n' "$BIN_DIR" ;;
esac
