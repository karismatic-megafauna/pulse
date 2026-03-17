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
    #[serde(default)]
    pub jira: JiraConfig,
    #[serde(default)]
    pub gitlab: GitlabConfig,
    #[serde(default)]
    pub slack: SlackConfig,
    #[serde(default)]
    pub calendar: CalendarConfig,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherConfig {
    pub enabled: bool,
    pub location: String,
    pub units: String,
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
pub struct JiraConfig {
    pub enabled: bool,
    pub base_url: String,
    pub email: String,
    pub api_token: String,
}

impl Default for JiraConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: String::new(),
            email: String::new(),
            api_token: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitlabConfig {
    pub enabled: bool,
    pub base_url: String,
    pub private_token: String,
}

impl Default for GitlabConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            base_url: String::new(),
            private_token: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackConfig {
    pub enabled: bool,
    pub bot_token: String,
    #[serde(default)]
    pub important_users: Vec<String>,
}

impl Default for SlackConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_token: String::new(),
            important_users: vec![],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarConfig {
    pub enabled: bool,
    #[serde(default = "default_num_events")]
    pub num_events: u32,
}

fn default_num_events() -> u32 {
    5
}

impl Default for CalendarConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            num_events: 5,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            weather: WeatherConfig::default(),
            jira: JiraConfig::default(),
            gitlab: GitlabConfig::default(),
            slack: SlackConfig::default(),
            calendar: CalendarConfig::default(),
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
