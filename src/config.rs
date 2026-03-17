use color_eyre::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub general: GeneralConfig,
    #[serde(default)]
    pub weather: WeatherConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherConfig {
    pub enabled: bool,
    pub location: String,
    pub units: String, // "imperial" or "metric"
}

impl Default for WeatherConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            location: String::new(),
            units: "imperial".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    pub weight_unit: String,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            weight_unit: "lbs".to_string(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            weather: WeatherConfig::default(),
        }
    }
}

pub fn config_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".config").join("pulse")
}

pub fn load_config() -> Result<Config> {
    let dir = config_dir();
    let path = dir.join("config.toml");

    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }

    if !path.exists() {
        let default = Config::default();
        let toml = toml::to_string_pretty(&default)?;
        fs::write(&path, toml)?;
        return Ok(default);
    }

    let contents = fs::read_to_string(&path)?;
    let config: Config = toml::from_str(&contents)?;
    Ok(config)
}
