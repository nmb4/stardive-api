use std::{net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct ModuleFlags {
    pub health: bool,
    pub search: bool,
    pub files: bool,
    pub render: bool,
    pub lostandfound: bool,
    pub installers: bool,
    pub eternal: bool,
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub bind_addr: SocketAddr,
    pub data_dir: PathBuf,
    pub log_dir: PathBuf,
    pub installers_dir: PathBuf,
    pub eternal_dir: PathBuf,
    pub api_key: Option<String>,
    pub max_upload_bytes: u64,
    pub max_snippet_chars: usize,
    pub modules: ModuleFlags,
}

impl ServerConfig {
    pub fn from_env() -> Result<Self> {
        let bind_addr = std::env::var("STARDIVE_BIND_ADDR")
            .unwrap_or_else(|_| "0.0.0.0:8080".to_string())
            .parse::<SocketAddr>()
            .context("invalid STARDIVE_BIND_ADDR")?;

        let data_dir = std::env::var("STARDIVE_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("data"));

        let installers_dir = std::env::var("STARDIVE_INSTALLERS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("installers"));

        let eternal_dir = std::env::var("STARDIVE_ETERNAL_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("eternal"));

        let log_dir = std::env::var("STARDIVE_LOG_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| data_dir.join("logs"));

        let api_key = std::env::var("STARDIVE_API_KEY").ok();

        let max_upload_bytes = std::env::var("STARDIVE_MAX_UPLOAD_BYTES")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(1_073_741_824);

        let max_snippet_chars = std::env::var("STARDIVE_MAX_SNIPPET_CHARS")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(20_000);

        let modules = ModuleFlags {
            health: read_bool("STARDIVE_ENABLE_HEALTH", true),
            search: read_bool("STARDIVE_ENABLE_SEARCH", true),
            files: read_bool("STARDIVE_ENABLE_FILES", true),
            render: read_bool("STARDIVE_ENABLE_RENDER", true),
            lostandfound: read_bool("STARDIVE_ENABLE_LOSTANDFOUND", true),
            installers: read_bool("STARDIVE_ENABLE_INSTALLERS", true),
            eternal: read_bool("STARDIVE_ENABLE_ETERNAL", true),
        };

        Ok(Self {
            bind_addr,
            data_dir,
            log_dir,
            installers_dir,
            eternal_dir,
            api_key,
            max_upload_bytes,
            max_snippet_chars,
            modules,
        })
    }

    pub fn public_mode(&self) -> bool {
        self.api_key.is_none()
    }
}

fn read_bool(key: &str, default: bool) -> bool {
    match std::env::var(key) {
        Ok(v) => matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"),
        Err(_) => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_bool_handles_true_values() {
        unsafe { std::env::set_var("STARDIVE_ENABLE_TEST", "true") };
        assert!(read_bool("STARDIVE_ENABLE_TEST", false));
        unsafe { std::env::remove_var("STARDIVE_ENABLE_TEST") };
    }
}
