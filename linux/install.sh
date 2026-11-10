#!/usr/bin/env bash
# Builds TodoBar for Linux and installs it (binary, icon, desktop entry).
set -euo pipefail

BINARY=todobar
REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Defaults: user-level install (no root required).
PREFIX="${HOME}/.local"

usage() {
    echo "Usage: $0 [--system] [--prefix PREFIX] [--uninstall]"
    echo ""
    echo "  --system          Install system-wide to /usr/local (requires sudo)"
    echo "  --prefix PREFIX   Override install prefix (default: ~/.local)"
    echo "  --uninstall       Remove a previously installed TodoBar"
    exit 1
}

UNINSTALL=false
while [[ $# -gt 0 ]]; do
    case "$1" in
        --system)    PREFIX=/usr/local; shift ;;
        --prefix)    PREFIX="$2"; shift 2 ;;
        --uninstall) UNINSTALL=true; shift ;;
        -h|--help)   usage ;;
        *) echo "Unknown option: $1"; usage ;;
    esac
done

BIN_DIR="${PREFIX}/bin"
ICON_DIR="${PREFIX}/share/icons/hicolor/scalable/apps"
DESKTOP_DIR="${PREFIX}/share/applications"

refresh_caches() {
    update-desktop-database "${DESKTOP_DIR}" 2>/dev/null || true
    gtk-update-icon-cache -f -t "${PREFIX}/share/icons/hicolor" 2>/dev/null || true
}

if [[ "${UNINSTALL}" == true ]]; then
    echo "Uninstalling TodoBar..."
    pkill -x "${BINARY}" 2>/dev/null || true
    rm -f "${BIN_DIR}/${BINARY}" "${ICON_DIR}/${BINARY}.svg" "${DESKTOP_DIR}/${BINARY}.desktop"
    rm -f "${HOME}/.config/autostart/${BINARY}.desktop"
    refresh_caches
    echo "Uninstall complete (your tasks in ~/.local/share/TodoBar are untouched)."
    exit 0
fi

if ! command -v cargo &>/dev/null; then
    echo "error: 'cargo' not found. Install Rust from https://rustup.rs" >&2
    exit 1
fi

for pkg in xkbcommon wayland-client fontconfig dbus-1; do
    if ! pkg-config --exists "$pkg" 2>/dev/null; then
        echo "error: missing development package for '$pkg'. On Debian/Ubuntu/Pop!_OS run:" >&2
        echo "  sudo apt install libxkbcommon-dev libwayland-dev libfontconfig-dev libdbus-1-dev pkg-config clang" >&2
        exit 1
    fi
done

echo "Building (first build downloads and compiles GPUI; this takes a few minutes)..."
cd "${REPO_DIR}"
cargo build --release
echo "Build complete."

mkdir -p "${BIN_DIR}" "${ICON_DIR}" "${DESKTOP_DIR}"
pkill -x "${BINARY}" 2>/dev/null || true
install -m 755 "target/release/${BINARY}" "${BIN_DIR}/${BINARY}"
echo "Installed binary  → ${BIN_DIR}/${BINARY}"

install -m 644 "assets/${BINARY}.svg" "${ICON_DIR}/${BINARY}.svg"
echo "Installed icon    → ${ICON_DIR}/${BINARY}.svg"

sed "s|^Exec=${BINARY}|Exec=${BIN_DIR}/${BINARY}|" "${BINARY}.desktop" > "${DESKTOP_DIR}/${BINARY}.desktop"
echo "Installed desktop → ${DESKTOP_DIR}/${BINARY}.desktop"
refresh_caches

if [[ ":${PATH}:" != *":${BIN_DIR}:"* ]]; then
    echo ""
    echo "Note: ${BIN_DIR} is not in your PATH."
    echo "Add this to your shell profile:"
    echo "  export PATH=\"${BIN_DIR}:\${PATH}\""
fi

echo ""
echo "Installation complete. Launch TodoBar from your applications menu or run:"
echo "  ${BIN_DIR}/${BINARY}"
echo "It registers itself to start at login on first run (toggle in the … menu)."
