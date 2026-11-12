**Lychee 0.1.2** levels the volume of every song, adds an equalizer and other audio settings, brings
colour themes, and lets you drag playlists into order.

## Download

| Your system | File | How to install |
|---|---|---|
| **Fedora** (also openSUSE, RHEL) | `lychee-0.1.2-1.x86_64.rpm` | Double-click it to install with GNOME Software, or run `sudo dnf install ./lychee-0.1.2-1.x86_64.rpm` (`sudo zypper install ./…` on openSUSE). It updates an installed older version, and dnf installs the libraries it needs. |
| **Windows 10 / 11** (64-bit) | `Lychee-0.1.2-windows-x64-setup.exe` | Run it. It installs for your account (no admin needed) or, if you choose, for everyone, and updates an installed older version. |
| Windows, without installing | `Lychee-0.1.2-windows-x64-portable.zip` | Unzip anywhere and run `lychee.exe`. |
| **macOS 11 or newer** (Apple silicon and Intel) | `Lychee-0.1.2-macos.dmg` | Open it and drag Lychee into Applications. The first time you open it, see *Opening Lychee on a Mac* below. |
| **Ubuntu, Debian, Mint, Pop!_OS** | `lychee_0.1.2_amd64.deb` | `sudo apt install ./lychee_0.1.2_amd64.deb` (apt installs the libraries it needs) |
| **Any other Linux** (Arch, Manjaro…) | `lychee-0.1.2-linux-x86_64.tar.xz` | Unpack it, open a terminal in the folder and run `./install.sh`. It installs any missing libraries with your package manager, then Lychee for your account (`--system` for everyone, `--uninstall` to remove it). |

`SHA256SUMS.txt` has checksums for every file.

## What's new

- **Volume levelling**, on by default: every song plays at about the same loudness (ReplayGain 2.0). Lychee
  uses your files' ReplayGain tags and measures the rest itself. Albums keep their own loud and quiet
  moments, and a pre-amp of up to +6 dB plays everything louder.
- **Settings → Audio**, a page of its own:
  - A ten-band equalizer with 16 presets, or drag the curve into a shape of your own. It lowers the level by
    its biggest boost, so songs never distort.
  - Mono audio, for listening with one earbud.
  - Headphone crossfeed (light, medium or strong), which takes the edge off songs mixed hard left and right.
  - The output device and volume levelling are here too.
- **Colour themes:** pick one dark and one light theme in Settings → Appearance, from eleven: Flow,
  Blackout, Catppuccin Mocha, Tokyo Night, Gruvbox, Rosé Pine, Moss, Afterglow, Flow Light, Liner Notes and
  Lychee.
- **Playlists:**
  - Drag songs and playlists into order: in a playlist, in the queue and in the sidebar.
  - Undo (or Ctrl+Z) after deleting a playlist, removing songs, sorting or removing duplicates.
  - Save the queue as a playlist, right-click the playing song to add it to one, and right-click a playlist
    in the sidebar for everything else.
  - Covers show the first four albums, and adding a song that's already in a playlist skips it.
- **Genres:** a song can have several genres, and spellings like "Hip-Hop" and "Hip Hop" count as one. The
  tag editor's scanner can tidy genre names.
- **Tag editor:** "Add albums" picks several albums from your library at once, and each open album gets a
  card of its own in the file list. Click a card's header to select the whole album, ready for Look up.
- **Play and Pause:** the Play button of whatever is playing (an album, playlist or artist, Favorites, Songs)
  turns into Pause, clicking the playing song pauses it instead of starting it over, and the playing bars
  move while music plays.
- **One Lychee at a time:** files opened with Lychee (from the file manager on Linux and Windows, or from the
  command line) play in the Lychee that's already open, instead of starting a second one.

## Good to know

- **After updating:** the first start re-reads your files' tags once, for genres and volume levelling. Then Lychee
  measures the loudness of songs without ReplayGain tags in the background, which keeps half your CPU cores busy
  for a while on a large library. Your play counts, favourites and playlists stay as they are.
- **Windows SmartScreen:** Windows may warn that it "protected your PC", because the installer isn't code-signed yet. Choose *More info → Run anyway*.
- **Linux requirements:** glibc 2.35 or newer (Ubuntu 22.04+, Debian 12+, Fedora 36+, current Arch, openSUSE Tumbleweed).
- **Wayland:** the mini player runs through XWayland so it can stay above other windows. You can turn that off in Settings → Appearance.
- **Last.fm:** scrobbling needs a free Last.fm API account. Enter its key in Settings → Last.fm.
- **Opening Lychee on a Mac:** Lychee isn't notarised by Apple, so the first time you open it macOS says
  it can't check it for malware. Click **Done**, open **System Settings → Privacy & Security**, and click
  **Open Anyway** next to the message about Lychee. You only need to do this once. Or, in Terminal:
  `xattr -dr com.apple.quarantine /Applications/Lychee.app`.
- **The macOS version** is still new: please report anything odd.
