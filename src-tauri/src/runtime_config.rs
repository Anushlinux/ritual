use serde::Serialize;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

pub const DEFAULT_ANTHROPIC_MODEL: &str = "claude-sonnet-4-5-20250929";
pub const DEFAULT_ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Debug, Clone)]
pub struct RuntimeSecret {
    pub value: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeConfigStatus {
    pub provider: String,
    pub model: String,
    pub has_api_key: bool,
    pub key_source: String,
    pub key_fingerprint: String,
    pub user_config_path: String,
    pub user_config_found: bool,
    pub cwd: String,
    pub executable_path: String,
    pub src_tauri_env_found: bool,
}

pub fn read_runtime_secret(name: &str) -> Option<RuntimeSecret> {
    for path in env_candidate_paths() {
        if let Some(value) = read_env_file_value(&path, name) {
            return Some(RuntimeSecret {
                value,
                source: path.display().to_string(),
            });
        }
    }

    if let Ok(v) = std::env::var(name) {
        let trimmed = v.trim().to_string();
        if !trimmed.is_empty() {
            return Some(RuntimeSecret {
                value: trimmed,
                source: format!("process env:{}", name),
            });
        }
    }

    None
}

pub fn read_runtime_env(name: &str) -> Option<String> {
    read_runtime_secret(name).map(|secret| secret.value)
}

pub fn anthropic_model() -> String {
    read_runtime_env("ANTHROPIC_MODEL").unwrap_or_else(|| DEFAULT_ANTHROPIC_MODEL.to_string())
}

pub fn anthropic_version() -> String {
    read_runtime_env("ANTHROPIC_VERSION").unwrap_or_else(|| DEFAULT_ANTHROPIC_VERSION.to_string())
}

pub fn runtime_config_status() -> RuntimeConfigStatus {
    let anthropic = read_runtime_secret("ANTHROPIC_API_KEY");
    let user_config_path = user_config_env_path();
    RuntimeConfigStatus {
        provider: "Claude".to_string(),
        model: anthropic_model(),
        has_api_key: anthropic.is_some(),
        key_source: anthropic
            .as_ref()
            .map(|s| s.source.clone())
            .unwrap_or_else(|| "missing".to_string()),
        key_fingerprint: anthropic
            .as_ref()
            .map(|s| key_fingerprint(&s.value))
            .unwrap_or_else(|| "missing".to_string()),
        user_config_path: user_config_path.display().to_string(),
        user_config_found: user_config_path.exists(),
        cwd: std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "unknown".to_string()),
        executable_path: std::env::current_exe()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "unknown".to_string()),
        src_tauri_env_found: src_tauri_env_path().is_some_and(|p| p.exists()),
    }
}

pub fn save_user_runtime_config(api_key: String, model: Option<String>) -> Result<RuntimeConfigStatus, String> {
    let api_key = clean_single_line_secret("ANTHROPIC_API_KEY", &api_key)?;
    if api_key.is_empty() {
        return Err("ANTHROPIC_API_KEY cannot be empty.".to_string());
    }

    let model = model
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
        .unwrap_or_else(|| DEFAULT_ANTHROPIC_MODEL.to_string());
    let model = clean_single_line_secret("ANTHROPIC_MODEL", &model)?;
    let version = DEFAULT_ANTHROPIC_VERSION;

    let path = user_config_env_path();
    let parent = path
        .parent()
        .ok_or_else(|| format!("Invalid config path: {}", path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("Failed to create config directory {}: {}", parent.display(), e))?;

    let contents = format!(
        "# Imprint local runtime config\nANTHROPIC_API_KEY={}\nANTHROPIC_MODEL={}\nANTHROPIC_VERSION={}\n",
        api_key, model, version
    );
    std::fs::write(&path, contents)
        .map_err(|e| format!("Failed to write runtime config {}: {}", path.display(), e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }

    Ok(runtime_config_status())
}

pub fn clear_user_runtime_config() -> Result<RuntimeConfigStatus, String> {
    let path = user_config_env_path();
    if path.exists() {
        std::fs::remove_file(&path)
            .map_err(|e| format!("Failed to remove runtime config {}: {}", path.display(), e))?;
    }
    Ok(runtime_config_status())
}

pub fn key_fingerprint(secret: &str) -> String {
    let mut hasher = DefaultHasher::new();
    secret.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn clean_single_line_secret(name: &str, value: &str) -> Result<String, String> {
    let trimmed = value.trim().trim_matches('"').trim_matches('\'').to_string();
    if trimmed.contains('\n') || trimmed.contains('\r') {
        return Err(format!("{} must be a single line.", name));
    }
    Ok(trimmed)
}

fn read_env_file_value(path: &PathBuf, name: &str) -> Option<String> {
    let contents = std::fs::read_to_string(path).ok()?;
    for line in contents.lines() {
        let line = line.trim();
        if line.starts_with('#') || !line.contains('=') {
            continue;
        }
        if let Some(rest) = line.strip_prefix(&format!("{}=", name)) {
            let value = rest.trim().trim_matches('"').trim_matches('\'');
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn env_candidate_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    paths.push(user_config_env_path());

    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join(".env"));
        paths.push(cwd.join("src-tauri").join(".env"));
        if let Some(parent) = cwd.parent() {
            paths.push(parent.join(".env"));
            paths.push(parent.join("src-tauri").join(".env"));
        }
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    paths.push(manifest_dir.join(".env"));
    if let Some(parent) = manifest_dir.parent() {
        paths.push(parent.join(".env"));
        paths.push(parent.join("src-tauri").join(".env"));
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            paths.push(exe_dir.join(".env"));
            paths.push(exe_dir.join("src-tauri").join(".env"));
            if let Some(parent) = exe_dir.parent() {
                paths.push(parent.join(".env"));
                paths.push(parent.join("src-tauri").join(".env"));
            }
        }
    }

    let mut deduped = Vec::new();
    for path in paths {
        if !deduped.contains(&path) {
            deduped.push(path);
        }
    }
    deduped
}

fn src_tauri_env_path() -> Option<PathBuf> {
    env_candidate_paths()
        .into_iter()
        .find(|p| p.ends_with("src-tauri/.env"))
}

pub fn user_config_env_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join("imprint")
        .join(".env")
}
