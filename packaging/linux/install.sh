#!/usr/bin/env bash
# Install Lychee on Linux: the libraries it needs (through your package manager) and the app.
#
#   ./install.sh               install for your user (~/.local); sudo is only asked for if
#                              libraries are missing
#   ./install.sh --system      install for everyone (/usr/local; needs sudo)
#   ./install.sh --no-deps     leave the package manager alone
#   ./install.sh --dry-run     show what would happen without changing anything
#   ./install.sh --uninstall   remove Lychee (add --system if you installed it that way)
#
# Your library, settings and caches (~/.config/lychee, ~/.local/share/lychee,
# ~/.cache/lychee) are never touched, not even by --uninstall.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
mode=user
deps=1
dry=0
action=install
for arg in "$@"; do
    case "$arg" in
        --system) mode=system ;;
        --user) mode=user ;;
        --no-deps) deps=0 ;;
        --dry-run) dry=1 ;;
        --uninstall) action=uninstall ;;
        -h | --help)
            sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "Unknown option: $arg (see --help)" >&2
            exit 2
            ;;
    esac
done

if [ "$mode" = system ]; then
    bindir=/usr/local/bin
    datadir=/usr/local/share
else
    bindir="$HOME/.local/bin"
    datadir="${XDG_DATA_HOME:-$HOME/.local/share}"
fi

say() { printf '%s\n' "$*"; }
run() {
    if [ "$dry" = 1 ]; then
        printf '  would run: %s\n' "$*"
    else
        "$@"
    fi
}
as_root() {
    if [ "$(id -u)" = 0 ]; then
        run "$@"
    elif command -v sudo >/dev/null 2>&1; then
        run sudo "$@"
    else
        say "Root access is needed for: $*"
        say "Run that as root, or use --no-deps."
        exit 1
    fi
}
# Install or remove a file, as root for --system.
put() { if [ "$mode" = system ]; then as_root "$@"; else run "$@"; fi; }

# ---------------------------------------------------------------------------------------------
# Libraries. Lychee links ALSA and fontconfig, and loads the rest (Wayland, X11, keyboard
# handling, OpenGL) when it starts.

libs=(libasound.so.2 libfontconfig.so.1 libxkbcommon.so.0 libwayland-client.so.0 libwayland-cursor.so.0
    libwayland-egl.so.1 libEGL.so.1 libGL.so.1 libX11.so.6 libXcursor.so.1 libXi.so.6 libX11-xcb.so.1 libxcb.so.1)

have_lib() {
    local ldconfig
    ldconfig="$(command -v ldconfig || true)"
    [ -z "$ldconfig" ] && [ -x /sbin/ldconfig ] && ldconfig=/sbin/ldconfig
    if [ -n "$ldconfig" ] && "$ldconfig" -p 2>/dev/null | grep -q "[[:space:]]$1 (.*x86-64"; then
        return 0
    fi
    local dir
    for dir in /usr/lib64 /usr/lib/x86_64-linux-gnu /lib64 /lib/x86_64-linux-gnu /usr/lib; do
        [ -e "$dir/$1" ] && return 0
    done
    return 1
}

# Debian/Ubuntu package for a library (Ubuntu 24.04 renamed libasound2 to libasound2t64).
apt_package() {
    case "$1" in
        libasound.so.2)
            if apt-cache show libasound2t64 >/dev/null 2>&1; then echo libasound2t64; else echo libasound2; fi
            ;;
        libfontconfig.so.1) echo libfontconfig1 ;;
        libxkbcommon.so.0) echo libxkbcommon0 ;;
        libwayland-client.so.0) echo libwayland-client0 ;;
        libwayland-cursor.so.0) echo libwayland-cursor0 ;;
        libwayland-egl.so.1) echo libwayland-egl1 ;;
        libEGL.so.1) echo libegl1 ;;
        libGL.so.1) echo libgl1 ;;
        libX11.so.6) echo libx11-6 ;;
        libXcursor.so.1) echo libxcursor1 ;;
        libXi.so.6) echo libxi6 ;;
        libX11-xcb.so.1) echo libx11-xcb1 ;;
        libxcb.so.1) echo libxcb1 ;;
    esac
}

pacman_package() {
    case "$1" in
        libasound.so.2) echo alsa-lib ;;
        libfontconfig.so.1) echo fontconfig ;;
        libxkbcommon.so.0) echo libxkbcommon ;;
        libwayland-*) echo wayland ;;
        libEGL.so.1 | libGL.so.1) echo libglvnd ;;
        libX11.so.6 | libX11-xcb.so.1) echo libx11 ;;
        libXcursor.so.1) echo libxcursor ;;
        libXi.so.6) echo libxi ;;
        libxcb.so.1) echo libxcb ;;
    esac
}

install_deps() {
    local missing=() lib
    for lib in "${libs[@]}"; do
        have_lib "$lib" || missing+=("$lib")
    done
    local need_xdg=0
    command -v xdg-open >/dev/null 2>&1 || need_xdg=1
    if [ "${#missing[@]}" = 0 ] && [ "$need_xdg" = 0 ]; then
        say "All the libraries Lychee needs are already installed."
        return
    fi
    [ "${#missing[@]}" -gt 0 ] && say "Missing libraries: ${missing[*]}"
    [ "$need_xdg" = 1 ] && say "Missing: xdg-open (opens links and folders)"

    local packages=()
    if command -v apt-get >/dev/null 2>&1; then
        for lib in "${missing[@]}"; do packages+=("$(apt_package "$lib")"); done
        mapfile -t packages < <(printf '%s\n' "${packages[@]}" | grep . || true)
        [ "$need_xdg" = 1 ] && packages+=(xdg-utils)
        as_root apt-get install -y "${packages[@]}"
    elif command -v dnf >/dev/null 2>&1; then
        # dnf installs by library name; it finds the right package itself.
        for lib in "${missing[@]}"; do packages+=("$lib()(64bit)"); done
        [ "$need_xdg" = 1 ] && packages+=(xdg-utils)
        as_root dnf install -y "${packages[@]}"
    elif command -v zypper >/dev/null 2>&1; then
        for lib in "${missing[@]}"; do packages+=("$lib()(64bit)"); done
        [ "$need_xdg" = 1 ] && packages+=(xdg-utils)
        as_root zypper --non-interactive install "${packages[@]}"
    elif command -v pacman >/dev/null 2>&1; then
        for lib in "${missing[@]}"; do packages+=("$(pacman_package "$lib")"); done
        [ "$need_xdg" = 1 ] && packages+=(xdg-utils)
        mapfile -t packages < <(printf '%s\n' "${packages[@]}" | grep . | sort -u || true)
        as_root pacman -S --needed --noconfirm "${packages[@]}"
    else
        say "Unknown package manager: please install the libraries above, then run this again with --no-deps."
        exit 1
    fi
}

# ---------------------------------------------------------------------------------------------
# The app

files=("$bindir/lychee" "$datadir/applications/lychee.desktop" "$datadir/icons/hicolor/scalable/apps/lychee.svg"
    "$datadir/icons/hicolor/256x256/apps/lychee.png")

refresh_caches() {
    if command -v update-desktop-database >/dev/null 2>&1; then
        put update-desktop-database -q "$datadir/applications" || true
    fi
    if command -v gtk-update-icon-cache >/dev/null 2>&1 && [ -d "$datadir/icons/hicolor" ]; then
        put gtk-update-icon-cache -q -t -f "$datadir/icons/hicolor" || true
    fi
}

install_app() {
    [ -x "$here/lychee" ] || {
        say "lychee isn't next to this script; run install.sh from the unpacked release folder."
        exit 1
    }
    put install -Dm755 "$here/lychee" "$bindir/lychee"
    # An absolute Exec line keeps the launcher working even when ~/.local/bin isn't on PATH.
    local desktop
    desktop="$(mktemp)"
    sed "s|^Exec=lychee |Exec=\"$bindir/lychee\" |" "$here/share/applications/lychee.desktop" >"$desktop"
    put install -Dm644 "$desktop" "$datadir/applications/lychee.desktop"
    rm -f "$desktop"
    put install -Dm644 "$here/share/icons/hicolor/scalable/apps/lychee.svg" "$datadir/icons/hicolor/scalable/apps/lychee.svg"
    put install -Dm644 "$here/share/icons/hicolor/256x256/apps/lychee.png" "$datadir/icons/hicolor/256x256/apps/lychee.png"
    refresh_caches
    say ""
    say "Lychee is installed: find it in your applications, or run $bindir/lychee"
    case ":$PATH:" in
        *":$bindir:"*) ;;
        *) [ "$mode" = user ] && say "(Add $bindir to your PATH to run plain \`lychee\` from a terminal.)" ;;
    esac
}

uninstall_app() {
    local f removed=0
    for f in "${files[@]}"; do
        if [ -e "$f" ]; then
            put rm -f "$f"
            removed=1
        fi
    done
    refresh_caches
    if [ "$removed" = 1 ]; then
        say "Lychee is removed. Your library and settings are still in ~/.config/lychee and ~/.local/share/lychee."
    else
        say "Lychee isn't installed in $bindir (try --system or --user)."
    fi
}

[ "$dry" = 1 ] && say "Dry run: nothing will be changed."
if [ "$action" = uninstall ]; then
    uninstall_app
else
    [ "$deps" = 1 ] && install_deps
    install_app
fi
