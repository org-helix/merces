//! App configuration (TOML in the OS config folder) and API keys (OS keyring,
//! with a file fallback only when no keyring service exists).

use std::path::PathBuf;

use bevy::prelude::Resource;
use directories::ProjectDirs;
use merces_agents::{EngineConfig, Lang};
use merces_market::MarketKeys;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FontChoice {
    #[default]
    Geist,
    Bricolage,
    GeistMono,
    /// A TTF/OTF file chosen by the user.
    Custom(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub theme: ThemeMode,
    /// Action accent, Helix Red by default.
    pub accent: [u8; 3],
    pub ui_font: FontChoice,
    pub title_font: FontChoice,
    /// Text size multiplier (0.8 – 1.5).
    pub font_scale: f32,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: ThemeMode::Dark,
            accent: [0xB2, 0x4E, 0x30],
            ui_font: FontChoice::Geist,
            title_font: FontChoice::Bricolage,
            font_scale: 1.0,
        }
    }
}

/// The single 3D quality setting: anti-aliasing, glow and star count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Quality {
    /// No MSAA, no bloom, fewer stars: integrated GPUs on battery.
    Low,
    #[default]
    Medium,
    High,
}

impl Quality {
    pub fn msaa(self) -> bool {
        self != Quality::Low
    }
    pub fn bloom(self) -> bool {
        self != Quality::Low
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SceneSettings {
    pub quality: Quality,
    /// 0 = follow the display (vsync).
    pub fps_cap: u32,
    /// Drop to 15 fps when the window is not focused.
    pub eco_unfocused: bool,
    /// Freeze the network and keep only the essential animations.
    pub reduced_motion: bool,
}

impl Default for SceneSettings {
    fn default() -> Self {
        Self {
            quality: Quality::Medium,
            fps_cap: 0,
            eco_unfocused: true,
            reduced_motion: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Resource)]
#[serde(default)]
pub struct AppConfig {
    pub engine: EngineConfig,
    pub appearance: Appearance,
    pub scene: SceneSettings,
    /// Simulated Claude and market: no keys, no network, no cost.
    pub demo_mode: bool,
    pub active_symbol: String,
    /// The "decision support, not financial advice" notice was acknowledged.
    pub disclaimer_seen: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        let engine = EngineConfig::default();
        let active_symbol = engine
            .symbols
            .first()
            .map(|s| s.symbol.clone())
            .unwrap_or_default();
        Self {
            engine,
            appearance: Appearance::default(),
            scene: SceneSettings::default(),
            demo_mode: true,
            active_symbol,
            disclaimer_seen: false,
        }
    }
}

impl AppConfig {
    pub fn lang(&self) -> Lang {
        self.engine.lang
    }
}

pub fn dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("org", "Helix", "Merces")
}

/// `MERCES_HOME` puts the configuration and the data in one folder (benchmarks, tests).
fn home_override() -> Option<PathBuf> {
    std::env::var_os("MERCES_HOME").map(PathBuf::from)
}

pub fn config_path() -> Option<PathBuf> {
    if let Some(home) = home_override() {
        return Some(home.join("merces.toml"));
    }
    dirs().map(|d| d.config_dir().join("merces.toml"))
}

pub fn data_dir() -> PathBuf {
    let dir = home_override()
        .or_else(|| dirs().map(|d| d.data_dir().to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn load() -> AppConfig {
    let Some(path) = config_path() else {
        return AppConfig::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
            tracing::warn!("config {} unreadable, defaults used: {e}", path.display());
            AppConfig::default()
        }),
        Err(_) => AppConfig::default(),
    }
}

pub fn save(cfg: &AppConfig) -> Result<(), String> {
    let path = config_path().ok_or("no config directory")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = toml::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| e.to_string())
}

// ── Secrets ─────────────────────────────────────────────────────────────────

pub const SECRET_NAMES: [&str; 5] = [
    "anthropic",
    "twelve_data",
    "alpaca_key_id",
    "alpaca_secret",
    "finnhub",
];
const SERVICE: &str = "merces";

#[derive(Debug, Clone, Default, PartialEq, Eq, Resource)]
pub struct Secrets {
    pub anthropic: String,
    pub twelve_data: String,
    pub alpaca_key_id: String,
    pub alpaca_secret: String,
    pub finnhub: String,
    /// Where they are kept: "keyring" or the fallback file path.
    pub backend: String,
}

impl Secrets {
    fn slot(&mut self, name: &str) -> &mut String {
        match name {
            "anthropic" => &mut self.anthropic,
            "twelve_data" => &mut self.twelve_data,
            "alpaca_key_id" => &mut self.alpaca_key_id,
            "alpaca_secret" => &mut self.alpaca_secret,
            _ => &mut self.finnhub,
        }
    }

    pub fn market_keys(&self) -> MarketKeys {
        let opt = |s: &str| (!s.trim().is_empty()).then(|| s.trim().to_owned());
        MarketKeys {
            twelve_data: opt(&self.twelve_data),
            alpaca_key_id: opt(&self.alpaca_key_id),
            alpaca_secret: opt(&self.alpaca_secret),
            finnhub: opt(&self.finnhub),
        }
    }

    pub fn anthropic_key(&self) -> Option<String> {
        (!self.anthropic.trim().is_empty()).then(|| self.anthropic.trim().to_owned())
    }
}

fn fallback_path() -> PathBuf {
    data_dir().join("secrets.toml")
}

pub fn load_secrets() -> Secrets {
    let mut s = Secrets {
        backend: "keyring".into(),
        ..Default::default()
    };
    let mut keyring_ok = true;
    for name in SECRET_NAMES {
        match keyring::Entry::new(SERVICE, name).and_then(|e| e.get_password()) {
            Ok(v) => *s.slot(name) = v,
            Err(keyring::Error::NoEntry) => {}
            Err(e) => {
                tracing::debug!("keyring {name}: {e}");
                keyring_ok = false;
            }
        }
    }
    if !keyring_ok && let Ok(text) = std::fs::read_to_string(fallback_path()) {
        if let Ok(t) = text.parse::<toml::Table>() {
            for name in SECRET_NAMES {
                if let Some(v) = t.get(name).and_then(|v| v.as_str()) {
                    *s.slot(name) = v.to_owned();
                }
            }
        }
        s.backend = fallback_path().display().to_string();
    }
    s
}

/// Save to the OS keyring; without one, to a file readable only by the user.
pub fn save_secrets(s: &Secrets) -> Result<String, String> {
    let mut copy = s.clone();
    let mut failed = None;
    for name in SECRET_NAMES {
        let value = copy.slot(name).trim().to_owned();
        let entry = match keyring::Entry::new(SERVICE, name) {
            Ok(e) => e,
            Err(e) => {
                failed = Some(e.to_string());
                break;
            }
        };
        let r = if value.is_empty() {
            match entry.delete_credential() {
                Err(keyring::Error::NoEntry) => Ok(()),
                other => other,
            }
        } else {
            entry.set_password(&value)
        };
        if let Err(e) = r {
            failed = Some(e.to_string());
            break;
        }
    }
    let Some(err) = failed else {
        return Ok("keyring".into());
    };
    tracing::warn!("keyring unavailable ({err}), using a private file");
    let mut t = toml::Table::new();
    for name in SECRET_NAMES {
        t.insert(
            name.into(),
            toml::Value::String(copy.slot(name).trim().to_owned()),
        );
    }
    let path = fallback_path();
    std::fs::write(&path, t.to_string()).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trips_through_toml() {
        let mut c = AppConfig::default();
        c.appearance.ui_font = FontChoice::Custom("C:/fonts/x.ttf".into());
        c.scene.quality = Quality::High;
        c.disclaimer_seen = true;
        let text = toml::to_string_pretty(&c).unwrap();
        assert_eq!(toml::from_str::<AppConfig>(&text).unwrap(), c);
    }

    #[test]
    fn partial_config_uses_defaults() {
        let c: AppConfig =
            toml::from_str("demo_mode = false\n[appearance]\nfont_scale = 1.2\n").unwrap();
        assert!(!c.demo_mode);
        assert_eq!(c.appearance.font_scale, 1.2);
        assert_eq!(c.appearance.theme, ThemeMode::Dark);
        assert_eq!(c.engine.symbols.len(), 1);
    }

    #[test]
    fn older_config_keys_still_load() {
        let c: AppConfig = toml::from_str(
            "[scene]\nview = \"graph\"\nbloom = false\nstars = 900\norbit_speed = 2.0\n\
             [appearance]\ndensity = \"compact\"\n",
        )
        .unwrap();
        assert!(!c.scene.reduced_motion);
        assert_eq!(c.scene.quality, Quality::Medium);
    }
}
