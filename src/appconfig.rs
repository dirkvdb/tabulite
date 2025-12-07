use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_theme")]
    pub theme: String,
}

fn default_theme() -> String {
    "Default Light".to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            theme: default_theme(),
        }
    }
}

/// Get the default config file path using XDG config directory
///
/// Respects $XDG_CONFIG_HOME if set, otherwise falls back to platform defaults:
/// - Linux/BSD: ~/.config
/// - macOS: ~/Library/Application Support
/// - Windows: %APPDATA%
fn get_default_config_path() -> Option<PathBuf> {
    // Check for XDG_CONFIG_HOME first (cross-platform)
    if let Ok(xdg_config) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg_config.is_empty() {
            return Some(
                PathBuf::from(xdg_config)
                    .join("tabulite")
                    .join("config.toml"),
            );
        }
    }

    // Fall back to platform-specific config directory
    dirs::config_dir().map(|config_dir| config_dir.join("tabulite").join("config.toml"))
}

pub fn load_config(config_path: Option<&Path>) -> AppConfig {
    let path = match config_path {
        Some(p) => p.to_path_buf(),
        None => match get_default_config_path() {
            Some(p) => p,
            None => {
                log::warn!("Could not determine config directory, using default configuration");
                return AppConfig::default();
            }
        },
    };

    match load_config_from_path(&path) {
        Ok(config) => {
            log::info!("Loaded configuration from: {}", path.display());
            config
        }
        Err(e) => {
            if path.exists() {
                log::warn!(
                    "Failed to load config from {}: {}. Using default configuration.",
                    path.display(),
                    e
                );
            } else {
                log::info!(
                    "Config file not found at {}. Using default configuration.",
                    path.display()
                );
            }
            AppConfig::default()
        }
    }
}

/// Load and parse the config file
fn load_config_from_path(path: &Path) -> Result<AppConfig> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;

    let config: AppConfig = toml::from_str(&contents)
        .with_context(|| format!("Failed to parse TOML config file: {}", path.display()))?;

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_load_config_from_toml() {
        let toml_content = r#"
theme = "light"
"#;
        let temp_dir = std::env::temp_dir();
        let config_path = temp_dir.join("test_config.toml");

        let mut file = fs::File::create(&config_path).unwrap();
        file.write_all(toml_content.as_bytes()).unwrap();

        let config = load_config(Some(&config_path));
        assert_eq!(config.theme, "light");

        fs::remove_file(config_path).ok();
    }

    #[test]
    fn test_load_config_missing_file() {
        let config = load_config(Some(Path::new("/nonexistent/path/config.toml")));
        assert_eq!(config.theme, "Default Light"); // Should return default
    }

    #[test]
    #[cfg_attr(not(target_env = "msvc"), serial_test::serial)]
    fn test_xdg_config_home_respected() {
        use std::env;

        let temp_dir = std::env::temp_dir();
        let xdg_path = temp_dir.join("test_xdg_config");

        // Set XDG_CONFIG_HOME
        unsafe {
            env::set_var("XDG_CONFIG_HOME", &xdg_path);
        }

        let config_path = get_default_config_path();

        // Clean up
        unsafe {
            env::remove_var("XDG_CONFIG_HOME");
        }

        assert!(config_path.is_some());
        let path = config_path.unwrap();
        assert!(path.to_string_lossy().contains("test_xdg_config"));
        assert!(path.ends_with("tabulite/config.toml"));
    }

    #[test]
    #[cfg_attr(not(target_env = "msvc"), serial_test::serial)]
    fn test_xdg_config_home_empty_ignored() {
        use std::env;

        // Set XDG_CONFIG_HOME to empty string
        unsafe {
            env::set_var("XDG_CONFIG_HOME", "");
        }

        let config_path = get_default_config_path();

        // Clean up
        unsafe {
            env::remove_var("XDG_CONFIG_HOME");
        }

        // Should fall back to platform default, not use empty XDG_CONFIG_HOME
        assert!(config_path.is_some());
    }
}
