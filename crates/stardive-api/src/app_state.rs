use std::sync::Arc;

use stardive_core::types::{ToolCapability, ToolsCapability};

use crate::{
    command_runner::CommandRunner, config::ServerConfig, file_store::FileStore, modules::ModuleDef,
};

#[derive(Debug, Clone)]
pub struct ToolStatus {
    pub available: bool,
    pub path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RuntimeTools {
    pub ddgs: ToolStatus,
    pub freeze: ToolStatus,
}

impl RuntimeTools {
    pub fn detect() -> Self {
        Self {
            ddgs: detect_tool("ddgs"),
            freeze: detect_tool("freeze"),
        }
    }

    pub fn to_public(&self) -> ToolsCapability {
        ToolsCapability {
            ddgs: ToolCapability {
                available: self.ddgs.available,
                path: self.ddgs.path.clone(),
            },
            freeze: ToolCapability {
                available: self.freeze.available,
                path: self.freeze.path.clone(),
            },
        }
    }
}

fn detect_tool(name: &str) -> ToolStatus {
    match which::which(name) {
        Ok(path) => ToolStatus {
            available: true,
            path: Some(path.display().to_string()),
        },
        Err(_) => ToolStatus {
            available: false,
            path: None,
        },
    }
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<ServerConfig>,
    pub file_store: Arc<FileStore>,
    pub tools: RuntimeTools,
    pub command_runner: Arc<dyn CommandRunner>,
    pub module_defs: Arc<Vec<ModuleDef>>,
}

impl AppState {
    pub fn new(
        config: Arc<ServerConfig>,
        file_store: Arc<FileStore>,
        tools: RuntimeTools,
        command_runner: Arc<dyn CommandRunner>,
        module_defs: Arc<Vec<ModuleDef>>,
    ) -> Self {
        Self {
            config,
            file_store,
            tools,
            command_runner,
            module_defs,
        }
    }
}
