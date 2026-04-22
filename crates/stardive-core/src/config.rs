use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::Deserialize;

const DEFAULT_BASE_URL: &str = "https://api.stardive.space";

#[derive(Debug, Clone)]
pub struct CliConfig {
    pub base_url: String,
    pub api_key: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct FileConfig {
    base_url: Option<String>,
    api_key: Option<String>,
}

fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| Path::new(".").to_path_buf())
        .join("stardive")
        .join("config.toml")
}

fn read_file_config(path: &Path) -> Result<FileConfig> {
    if !path.exists() {
        return Ok(FileConfig::default());
    }
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read config file at {}", path.display()))?;
    let parsed = toml::from_str::<FileConfig>(&raw)
        .with_context(|| format!("failed to parse config file at {}", path.display()))?;
    Ok(parsed)
}

pub fn resolve_cli_config(
    base_url_override: Option<String>,
    api_key_override: Option<String>,
) -> Result<CliConfig> {
    let path = config_path();
    let file_cfg = read_file_config(&path)?;

    let env_base = std::env::var("STARDIVE_BASE_URL").ok();
    let env_key = std::env::var("STARDIVE_API_KEY").ok();

    let base_url = base_url_override
        .or(env_base)
        .or(file_cfg.base_url)
        .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());

    let api_key = api_key_override.or(env_key).or(file_cfg.api_key);

    Ok(CliConfig { base_url, api_key })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_base_url_is_set() {
        let cfg = resolve_cli_config(None, None).expect("config should load");
        assert!(!cfg.base_url.is_empty());
    }
}
