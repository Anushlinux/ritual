use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const ONBOARDING_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnboardingState {
    pub completed: bool,
    pub completed_at: Option<String>,
    pub version: u32,
}

impl Default for OnboardingState {
    fn default() -> Self {
        Self {
            completed: false,
            completed_at: None,
            version: ONBOARDING_VERSION,
        }
    }
}

fn config_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join(".imprint")
}

fn config_path() -> PathBuf {
    config_dir().join("onboarding.json")
}

pub fn get_state() -> OnboardingState {
    let path = config_path();
    if !path.exists() {
        return OnboardingState::default();
    }

    std::fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str::<OnboardingState>(&content).ok())
        .unwrap_or_default()
}

fn save_state(state: &OnboardingState) -> Result<(), String> {
    std::fs::create_dir_all(config_dir()).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    std::fs::write(config_path(), json).map_err(|e| e.to_string())
}

pub fn complete() -> Result<OnboardingState, String> {
    let state = OnboardingState {
        completed: true,
        completed_at: Some(Utc::now().to_rfc3339()),
        version: ONBOARDING_VERSION,
    };
    save_state(&state)?;
    Ok(state)
}

pub fn reset() -> Result<OnboardingState, String> {
    let state = OnboardingState::default();
    save_state(&state)?;
    Ok(state)
}
