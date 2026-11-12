//! User settings (`config.toml`) and well-known directories.

use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use lychee_core::RepeatMode;
use serde::{Deserialize, Serialize};

pub struct Paths {
    pub config_file: PathBuf,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
}

impl Paths {
    pub fn discover() -> Paths {
        // LYCHEE_HOME keeps everything in one folder (portable installs, testing).
        if let Some(home) = std::env::var_os("LYCHEE_HOME").map(PathBuf::from) {
            return Paths { config_file: home.join("config.toml"), data_dir: home.join("data"), cache_dir: home.join("cache") };
        }
        match ProjectDirs::from("", "", "Lychee") {
            Some(dirs) => Paths {
                config_file: dirs.config_dir().join("config.toml"),
                data_dir: dirs.data_dir().to_path_buf(),
                cache_dir: dirs.cache_dir().to_path_buf(),
            },
            None => {
                let base = std::env::current_dir().unwrap_or_default().join(".lychee");
                Paths { config_file: base.join("config.toml"), data_dir: base.join("data"), cache_dir: base.join("cache") }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    System,
    Dark,
    Light,
}

impl ThemeMode {
    pub fn index(self) -> i32 {
        match self {
            ThemeMode::System => 0,
            ThemeMode::Dark => 1,
            ThemeMode::Light => 2,
        }
    }

    pub fn from_index(i: i32) -> Self {
        match i {
            1 => ThemeMode::Dark,
            2 => ThemeMode::Light,
            _ => ThemeMode::System,
        }
    }
}

/// Which artist Last.fm gets for a track.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScrobbleArtist {
    /// The main artist only ("A feat. B" and "A, B" scrobble as "A").
    #[default]
    Main,
    /// The artist tag as written.
    Exact,
}

impl ScrobbleArtist {
    pub fn index(self) -> i32 {
        match self {
            ScrobbleArtist::Main => 0,
            ScrobbleArtist::Exact => 1,
        }
    }

    pub fn from_index(i: i32) -> Self {
        if i == 1 { ScrobbleArtist::Exact } else { ScrobbleArtist::Main }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct LastfmSettings {
    /// Your own API account, used when this build has none baked in.
    pub api_key: String,
    pub api_secret: String,
    pub username: Option<String>,
    pub session_key: Option<String>,
    /// Love tracks on Last.fm when they're added to Favorites.
    pub love_sync: bool,
    pub artist: ScrobbleArtist,
}

impl Default for LastfmSettings {
    fn default() -> Self {
        LastfmSettings {
            api_key: String::new(),
            api_secret: String::new(),
            username: None,
            session_key: None,
            love_sync: true,
            artist: ScrobbleArtist::Main,
        }
    }
}

/// The equalizer: on or off, and which curve.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EqSettings {
    pub enabled: bool,
    /// A preset id (see `eq.rs`), or "custom" for `custom`.
    pub preset: String,
    /// The user's own curve, in dB per band (32 Hz … 16 kHz).
    pub custom: [f32; lychee_audio::EQ_BANDS],
}

impl Default for EqSettings {
    fn default() -> Self {
        EqSettings { enabled: false, preset: "flat".into(), custom: [0.0; lychee_audio::EQ_BANDS] }
    }
}

impl EqSettings {
    /// The curve in use.
    pub fn bands(&self) -> [f32; lychee_audio::EQ_BANDS] {
        crate::eq::preset(&self.preset).map_or(self.custom, |p| p.bands)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: ThemeMode,
    /// Theme ids (see `themes.rs`) for dark and light mode.
    pub dark_theme: String,
    pub light_theme: String,
    pub dynamic_accent: bool,
    pub volume: f32,
    pub muted: bool,
    pub output_device: Option<String>,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub right_panel: bool,
    pub right_tab: String,
    pub album_sort: i32,
    pub track_sort: i32,
    /// Keep the mini player above other windows (on Wayland this runs it on XWayland).
    pub mini_on_top: bool,
    /// Play every song at about the same loudness (ReplayGain, or Lychee's own measurement).
    pub volume_levelling: bool,
    /// Extra gain on top of volume levelling, in dB (0, 2, 4 or 6): a louder target than −18 LUFS.
    pub levelling_preamp: f32,
    pub eq: EqSettings,
    /// Mono audio: left and right mixed together and played on both sides.
    pub mono: bool,
    /// Headphone crossfeed, and its strength (1–3: light, medium, strong). Not used while `mono` is on.
    pub crossfeed: bool,
    pub crossfeed_level: u32,
    pub lastfm: LastfmSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: ThemeMode::System,
            dark_theme: crate::themes::DEFAULT_DARK.to_string(),
            light_theme: crate::themes::DEFAULT_LIGHT.to_string(),
            dynamic_accent: true,
            volume: 0.8,
            muted: false,
            output_device: None,
            shuffle: false,
            repeat: RepeatMode::Off,
            right_panel: true,
            right_tab: "queue".into(),
            album_sort: 0,
            track_sort: 0,
            mini_on_top: true,
            volume_levelling: true,
            levelling_preamp: 0.0,
            eq: EqSettings::default(),
            mono: false,
            crossfeed: false,
            crossfeed_level: 2,
            lastfm: LastfmSettings::default(),
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Settings {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
                tracing::warn!("ignoring unreadable settings file {}: {e}", path.display());
                Settings::default()
            }),
            Err(_) => Settings::default(),
        }
    }

    pub fn save(&self, path: &Path) {
        let Ok(text) = toml::to_string_pretty(self) else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = path.with_extension("toml.tmp");
        if std::fs::write(&tmp, text).is_ok() {
            restrict_permissions(&tmp);
            let _ = std::fs::rename(&tmp, path);
        }
    }

    pub fn lastfm_credentials(&self) -> Option<lychee_lastfm::Credentials> {
        let custom = lychee_lastfm::Credentials {
            api_key: self.lastfm.api_key.trim().to_string(),
            api_secret: self.lastfm.api_secret.trim().to_string(),
        };
        if custom.is_complete() { Some(custom) } else { lychee_lastfm::Credentials::built_in() }
    }

    pub fn lastfm_session(&self) -> Option<lychee_lastfm::Session> {
        Some(lychee_lastfm::Session { username: self.lastfm.username.clone()?, key: self.lastfm.session_key.clone()? })
    }
}

/// The settings file holds the Last.fm session key, so keep it private to the user.
fn restrict_permissions(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    let _ = path;
}
