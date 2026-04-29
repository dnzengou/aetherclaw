use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub workspace: PathBuf,
    pub gateway: GatewayConfig,
    pub channels: ChannelsConfig,
    pub llm: LlmConfig,
    pub heartbeat: HeartbeatConfig,
    pub security: SecurityConfig,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GatewayConfig {
    pub host: String,
    pub port: u16,
    pub dashboard_enabled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChannelsConfig {
    pub telegram: Option<telegram::TelegramConfig>,
    pub discord: Option<discord::DiscordConfig>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LlmConfig {
    pub local_models_path: PathBuf,
    pub default_local_model: String,
    pub cloud_fallback: Option<CloudLlmConfig>,
    pub model_list: Vec<ModelEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelEntry {
    pub model_name: String,
    pub model: String, // vendor/model format
    pub api_key: Option<String>,
    pub api_base: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CloudLlmConfig {
    pub provider: String,
    pub api_key: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HeartbeatConfig {
    pub enabled: bool,
    pub interval: u32, // minutes
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SecurityConfig {
    pub restrict_to_workspace: bool,
    pub allow_exec: bool,
}

impl Default for Config {
    fn default() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        Self {
            workspace: home.join(".aetherclaw").join("workspace"),
            gateway: GatewayConfig {
                host: "0.0.0.0".to_string(),
                port: 8080,
                dashboard_enabled: true,
            },
            channels: ChannelsConfig {
                telegram: None,
                discord: None,
            },
            llm: LlmConfig {
                local_models_path: home.join(".aetherclaw").join("models"),
                default_local_model: "phi-2-q4.gguf".to_string(),
                cloud_fallback: None,
                model_list: vec![],
            },
            heartbeat: HeartbeatConfig {
                enabled: true,
                interval: 30,
            },
            security: SecurityConfig {
                restrict_to_workspace: true,
                allow_exec: true,
            },
        }
    }
}

impl Config {
    pub fn exists() -> bool {
        let path = Self::config_path();
        path.exists()
    }

    pub fn config_path() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".aetherclaw")
            .join("config.toml")
    }

    pub fn load() -> Result<Self> {
        let path = Self::config_path();
        let content = std::fs::read_to_string(&path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();
        std::fs::create_dir_all(path.parent().unwrap())?;
        let content = toml::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn migrate_from_picoclaw() -> Result<Self> {
        // Migration logic from old PicoClaw JSON config
        tools::migration::migrate().await
    }
}

pub mod telegram {
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct TelegramConfig {
        pub enabled: bool,
        pub token: String,
        pub allow_from: Vec<String>,
        pub proxy: Option<String>,
    }
}

pub mod discord {
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, Serialize, Deserialize)]
    pub struct DiscordConfig {
        pub enabled: bool,
        pub token: String,
        pub allow_from: Vec<String>,
    }
}

// Forward declarations for migration
pub mod tools {
    pub mod migration {
        use anyhow::Result;
        use crate::config::Config;

        #[allow(dead_code)]
        pub async fn migrate() -> Result<Config> {
            tracing::info!("Migrating from PicoClaw...");
            let config = Config::default();
            config.save()?;
            Ok(config)
        }
    }
}
