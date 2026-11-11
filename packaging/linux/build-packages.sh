#!/usr/bin/env bash
# Build the Linux release packages from a release binary:
#
#   packaging/linux/build-packages.sh VERSION path/to/lychee OUTDIR
#
# Always makes lychee-VERSION-linux-x86_64.tar.xz (with install.sh). Also makes a .deb when
# dpkg-deb is available and an .rpm when rpmbuild is. CI builds on the oldest supported
# Ubuntu so the binary runs on older distributions too.
set -euo pipefail

version="${1:?version}"
binary="${2:?path to the lychee binary}"
out="${3:?output folder}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
mkdir -p "$out"
out="$(cd "$out" && pwd)"
binary="$(cd "$(dirname "$binary")" && pwd)/$(basename "$binary")"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

summary="Modern, beautiful music player for your own library"
description="Lychee plays your local music library: gapless playback of FLAC, MP3, AAC/ALAC, Ogg, \
Opus and WAV, a fast library with artwork colours, synced lyrics, a command palette, a mini \
player, Last.fm scrobbling, MPRIS media controls and a tag editor with MusicBrainz lookups."

# Newest glibc symbol the binary uses: the minimum glibc it runs on.
glibc="$(objdump -T "$binary" 2>/dev/null | grep -o 'GLIBC_[0-9.]*' | sed 's/GLIBC_//' | sort -V | tail -1 || true)"
glibc="${glibc:-2.35}"

# --- tar.gz with install.sh --------------------------------------------------------------------
name="lychee-$version-linux-x86_64"
t="$work/$name"
install -Dm755 "$binary" "$t/lychee"
install -Dm755 "$root/packaging/linux/install.sh" "$t/install.sh"
install -Dm644 "$root/assets/linux/lychee.desktop" "$t/share/applications/lychee.desktop"
install -Dm644 "$root/assets/icons/lychee.svg" "$t/share/icons/hicolor/scalable/apps/lychee.svg"
install -Dm644 "$root/assets/icons/lychee-256.png" "$t/share/icons/hicolor/256x256/apps/lychee.png"
install -Dm644 "$root/LICENSE" "$t/LICENSE"
install -Dm644 "$root/README.md" "$t/README.md"
tar -C "$work" --owner=0 --group=0 -cJf "$out/$name.tar.xz" "$name"
echo "built $out/$name.tar.xz"

# --- the /usr tree shared by .deb and .rpm -----------------------------------------------------
fs="$work/fs"
install -Dm755 "$binary" "$fs/usr/bin/lychee"
install -Dm644 "$root/assets/linux/lychee.desktop" "$fs/usr/share/applications/lychee.desktop"
install -Dm644 "$root/assets/icons/lychee.svg" "$fs/usr/share/icons/hicolor/scalable/apps/lychee.svg"
install -Dm644 "$root/assets/icons/lychee-256.png" "$fs/usr/share/icons/hicolor/256x256/apps/lychee.png"

# Libraries Lychee loads at runtime (dlopen), which no dependency scanner can see.
runtime_sonames=(libxkbcommon.so.0 libwayland-client.so.0 libwayland-cursor.so.0 libwayland-egl.so.1 libEGL.so.1
    libGL.so.1 libX11.so.6 libXcursor.so.1 libXi.so.6 libX11-xcb.so.1 libxcb.so.1 libasound.so.2 libfontconfig.so.1)

# --- .deb ------------------------------------------------------------------------------------
if command -v dpkg-deb >/dev/null 2>&1; then
    d="$work/deb"
    cp -a "$fs" "$d"
    install -Dm644 "$root/LICENSE" "$d/usr/share/doc/lychee/copyright"
    mkdir -p "$d/DEBIAN"
    size="$(du -sk "$d/usr" | cut -f1)"
    {
        echo "Package: lychee"
        echo "Version: $version"
        echo "Architecture: amd64"
        echo "Maintainer: Lychee <noreply@github.com>"
        echo "Installed-Size: $size"
        echo "Depends: libc6 (>= $glibc), libgcc-s1, libasound2t64 | libasound2, libfontconfig1, libxkbcommon0, libwayland-client0, libwayland-cursor0, libwayland-egl1, libegl1, libgl1, libx11-6, libxcursor1, libxi6, libx11-xcb1, libxcb1, xdg-utils"
        echo "Recommends: xdg-desktop-portal"
        echo "Section: sound"
        echo "Priority: optional"
        echo "Homepage: https://github.com/Noffee25/Lychee"
        echo "Description: $summary"
        echo "$description" | fold -s -w 78 | sed 's/^/ /'
    } >"$d/DEBIAN/control"
    dpkg-deb --root-owner-group --build "$d" "$out/lychee_${version}_amd64.deb"
fi

# --- .rpm ------------------------------------------------------------------------------------
if command -v rpmbuild >/dev/null 2>&1; then
    top="$work/rpm"
    mkdir -p "$top"/{BUILD,RPMS,SOURCES,SPECS,SRPMS}
    install -Dm644 "$root/LICENSE" "$fs/usr/share/licenses/lychee/LICENSE"
    requires=""
    for so in "${runtime_sonames[@]}"; do requires+="Requires: $so()(64bit)"$'\n'; done
    cat >"$top/SPECS/lychee.spec" <<EOF
# The binary is built by cargo beforehand; this only packages it.
%global debug_package %{nil}
%global __os_install_post %{nil}
%global _build_id_links none

Name: lychee
Version: $version
Release: 1
Summary: $summary
License: Apache-2.0
URL: https://github.com/Noffee25/Lychee
BuildArch: x86_64
Requires: glibc >= $glibc
${requires}Requires: xdg-utils
Recommends: xdg-desktop-portal

%description
$description

%install
mkdir -p %{buildroot}
cp -a "$fs/." %{buildroot}/

%files
%license /usr/share/licenses/lychee/LICENSE
/usr/bin/lychee
/usr/share/applications/lychee.desktop
/usr/share/icons/hicolor/scalable/apps/lychee.svg
/usr/share/icons/hicolor/256x256/apps/lychee.png

%changelog
* $(LC_ALL=C date -u '+%a %b %d %Y') Lychee <noreply@github.com> - $version-1
- Lychee $version
EOF
    rpmbuild --quiet --define "_topdir $top" --define "_binary_payload w6.xzdio" -bb "$top/SPECS/lychee.spec"
    cp "$top"/RPMS/x86_64/*.rpm "$out/"
    echo "built $(ls "$out"/*.rpm)"
fi
