The first release of **Lychee**, a modern music player for your own library on Linux and Windows.

## Download

| Your system | File | How to install |
|---|---|---|
| **Windows 10 / 11** (64-bit) | `Lychee-0.1.0-windows-x64-setup.exe` | Run it. It installs for your account (no admin needed) or, if you choose, for everyone. |
| Windows, without installing | `Lychee-0.1.0-windows-x64-portable.zip` | Unzip anywhere and run `lychee.exe`. |
| **Ubuntu, Debian, Mint, Pop!_OS** | `lychee_0.1.0_amd64.deb` | `sudo apt install ./lychee_0.1.0_amd64.deb` (apt installs the libraries it needs) |
| **Fedora, openSUSE, RHEL** | `lychee-0.1.0-1.x86_64.rpm` | `sudo dnf install ./lychee-0.1.0-1.x86_64.rpm` (or `sudo zypper install ./…`) |
| **Any other Linux** (Arch, Manjaro…) | `lychee-0.1.0-linux-x86_64.tar.xz` | Unpack it, open a terminal in the folder and run `./install.sh`. It installs any missing libraries with your package manager, then Lychee for your account (`--system` for everyone, `--uninstall` to remove it). |

`SHA256SUMS.txt` has checksums for every file.

## What's in it

- **Plays your library:** MP3, FLAC (up to 24-bit/192 kHz), WAV, AIFF, ALAC, AAC/M4A, Ogg Vorbis and Opus, gaplessly, with high-quality resampling.
- **The Flow interface:** album pages tinted with their artwork's colours, Queue / Lyrics / Details panel, dark and light themes, and a Ctrl+K command palette.
- **Synced lyrics** from `.lrc` files or embedded in your files.
- **Tag editor** modelled on EasyTAG:
  - Edit many files at once, with track numbering, artwork, lyrics and undo.
  - Its scanner fills tags from file names, renames files from tags and cleans up text.
- **Online lookup:** recognises an album from its tags or folder name and fills in what's missing from MusicBrainz, with covers from the Cover Art Archive and lyrics from LRCLIB.
- **Mini player** (Ctrl+M) that stays on top, **Last.fm** scrobbling, and **media keys** (MPRIS on Linux, the media overlay on Windows).

## Good to know

- **Windows SmartScreen:** Windows may warn that it "protected your PC", because the installer isn't code-signed yet. Choose *More info → Run anyway*.
- **Linux requirements:** glibc 2.35 or newer (Ubuntu 22.04+, Debian 12+, Fedora 36+, current Arch, openSUSE Tumbleweed).
- **Wayland:** the mini player runs through XWayland so it can stay above other windows. You can turn that off in Settings → Appearance.
- **Last.fm:** scrobbling needs a free Last.fm API account. Enter its key in Settings → Last.fm.
- **The Windows build** is new: it is built and tested by CI, but please report anything odd.
