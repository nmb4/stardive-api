pub mod files;
pub mod health;
pub mod lostandfound;
pub mod render;
pub mod search;
pub mod static_assets;

use axum::Router;
use stardive_core::types::ModuleCapability;

use crate::{app_state::AppState, config::ModuleFlags};

pub type RegisterFn = fn(Router<AppState>) -> Router<AppState>;
pub type CapabilityFn = fn(&AppState) -> ModuleCapability;
pub type EnabledFn = fn(&ModuleFlags) -> bool;

#[derive(Clone, Copy)]
pub struct ModuleDef {
    pub name: &'static str,
    pub register: RegisterFn,
    pub capability: CapabilityFn,
    pub enabled: EnabledFn,
}

pub fn registry() -> Vec<ModuleDef> {
    vec![
        health::module_def(),
        search::module_def(),
        files::module_def(),
        render::module_def(),
        lostandfound::module_def(),
        static_assets::installers_module_def(),
        static_assets::eternal_module_def(),
    ]
}
