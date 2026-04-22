use axum::{Json, Router, extract::State, routing::get};
use stardive_core::types::{HealthResponse, ModuleCapability};

use crate::{app_state::AppState, config::ModuleFlags};

use super::ModuleDef;

pub fn module_def() -> ModuleDef {
    ModuleDef {
        name: "health",
        register,
        capability,
        enabled: |flags: &ModuleFlags| flags.health,
    }
}

fn register(router: Router<AppState>) -> Router<AppState> {
    router.route("/health", get(get_health))
}

fn capability(_: &AppState) -> ModuleCapability {
    ModuleCapability {
        name: "health".to_string(),
        enabled: true,
        healthy: true,
        detail: Some("core health module".to_string()),
    }
}

async fn get_health(State(state): State<AppState>) -> Json<HealthResponse> {
    let modules = state
        .module_defs
        .iter()
        .map(|def| {
            let enabled = (def.enabled)(&state.config.modules);
            if enabled {
                let mut c = (def.capability)(&state);
                c.enabled = true;
                c
            } else {
                ModuleCapability {
                    name: def.name.to_string(),
                    enabled: false,
                    healthy: true,
                    detail: Some("disabled via config".to_string()),
                }
            }
        })
        .collect::<Vec<_>>();

    let status = if modules.iter().any(|m| m.enabled && !m.healthy) {
        "degraded"
    } else {
        "ok"
    };

    Json(HealthResponse {
        status: status.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        public_mode: state.config.public_mode(),
        modules,
        tools: state.tools.to_public(),
    })
}
