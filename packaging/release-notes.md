**Lychee 0.1.1** brings Lychee to the Mac, handles featured artists the way your library and Last.fm expect,
and polishes the interface.

## Download

| Your system | File | How to install |
|---|---|---|
| **Fedora** (also openSUSE, RHEL) | `lychee-0.1.1-1.x86_64.rpm` | Double-click it to install with GNOME Software, or run `sudo dnf install ./lychee-0.1.1-1.x86_64.rpm` (`sudo zypper install ./…` on openSUSE). It updates an installed 0.1.0, and dnf installs the libraries it needs. |
| **Windows 10 / 11** (64-bit) | `Lychee-0.1.1-windows-x64-setup.exe` | Run it. It installs for your account (no admin needed) or, if you choose, for everyone, and updates an installed 0.1.0. |
| Windows, without installing | `Lychee-0.1.1-windows-x64-portable.zip` | Unzip anywhere and run `lychee.exe`. |
| **macOS 11 or newer** (Apple silicon and Intel) | `Lychee-0.1.1-macos.dmg` | Open it and drag Lychee into Applications. The first time you open it, see *Opening Lychee on a Mac* below. |
| **Ubuntu, Debian, Mint, Pop!_OS** | `lychee_0.1.1_amd64.deb` | `sudo apt install ./lychee_0.1.1_amd64.deb` (apt installs the libraries it needs) |
| **Any other Linux** (Arch, Manjaro…) | `lychee-0.1.1-linux-x86_64.tar.xz` | Unpack it, open a terminal in the folder and run `./install.sh`. It installs any missing libraries with your package manager, then Lychee for your account (`--system` for everyone, `--uninstall` to remove it). |

`SHA256SUMS.txt` has checksums for every file.

## What's new

- **macOS:** a universal app for Apple silicon and Intel Macs, in a disk image. Media keys and the Now Playing
  widget work, and shortcuts use ⌘.
- **Featured artists:**
  - "A feat. B, C" shows as separate artist links in the player bar and the Queue panel.
  - Last.fm gets the main artist by default, so "A feat. B" counts for A. Settings → Last.fm → *Artist to
    scrobble* switches to the tags exactly as written.
  - A guest on one track no longer turns an album into "Various Artists".
  - The tag editor's scanner can rewrite artist credits in one style ("A feat. B & C", "A, B & C"…).
- **Interface:**
  - Back and forward buttons in the top left, each greyed out when there's nowhere to go.
  - Settings is a link at the bottom of the sidebar, in place of the library status line.
  - The output device is a dropdown in Settings → Playback.
  - The theme button left the top bar (it's in Settings → Appearance), and empty screens lost their icons.
- **Fixes:**
  - GNOME and KDE on Wayland show Lychee's icon and name in the dock, instead of a generic "Unknown".
  - Smaller windows: below 1200 px wide the side panel opens over the content instead of squeezing the page,
    Genres tiles no longer shrink after a resize, the tag editor fits, and the player bar isn't cut off at the
    smallest height.

## Good to know

- **After updating:** the first start re-reads your files' tags once, to find each song's main artist. Your play
  counts, favourites and playlists stay as they are.
- **Windows SmartScreen:** Windows may warn that it "protected your PC", because the installer isn't code-signed yet. Choose *More info → Run anyway*.
- **Linux requirements:** glibc 2.35 or newer (Ubuntu 22.04+, Debian 12+, Fedora 36+, current Arch, openSUSE Tumbleweed).
- **Wayland:** the mini player runs through XWayland so it can stay above other windows. You can turn that off in Settings → Appearance.
- **Last.fm:** scrobbling needs a free Last.fm API account. Enter its key in Settings → Last.fm.
- **Opening Lychee on a Mac:** Lychee isn't notarised by Apple, so the first time you open it macOS says
  it can't check it for malware. Click **Done**, open **System Settings → Privacy & Security**, and click
  **Open Anyway** next to the message about Lychee. You only need to do this once. Or, in Terminal:
  `xattr -dr com.apple.quarantine /Applications/Lychee.app`.
- **The macOS version** is new: please report anything odd.
