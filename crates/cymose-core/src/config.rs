//! `~/.config/cymose/config.toml`, plus a per-workspace `.cymose/config.toml`
//! that overrides it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::agent::CommandPolicy;
use crate::error::{Error, Result};
use crate::router::RouterConfig;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub api: ApiConfig,
    #[serde(default)]
    pub router: RouterConfig,
    #[serde(default)]
    pub agent: CommandPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    /// Configuration, not a constant, so a contributor can point at a local
    /// server without patching the binary.
    pub base_url: String,
    /// Cymose account token, used only to read your Web tree (`cymose sync`).
    ///
    /// Optional, and absent by default. 0.1 needs no account: turns go straight
    /// to OpenRouter on your own key whether or not this is ever set. Setting
    /// it buys exactly one thing — seeing the tree you planned in the browser
    /// from the terminal.
    #[serde(default)]
    pub token: Option<String>,
}

impl Default for ApiConfig {
    fn default() -> Self {
        ApiConfig {
            base_url: "https://api.cymose.app".into(),
            token: None,
        }
    }
}

impl Config {
    pub fn config_dir() -> Result<PathBuf> {
        let base = if cfg!(windows) {
            std::env::var_os("APPDATA").map(PathBuf::from)
        } else {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        };
        Ok(base
            .ok_or_else(|| Error::Config("no home directory".into()))?
            .join("cymose"))
    }

    /// Loads the user config, then the workspace one on top of it. A missing
    /// file is not an error — defaults are a working configuration.
    pub fn load(workspace_root: Option<&Path>) -> Result<Self> {
        let mut config: Config =
            Self::read(&Self::config_dir()?.join("config.toml"))?.unwrap_or_default();
        if let Some(root) = workspace_root {
            if let Some(local) = Self::read(&root.join(".cymose").join("config.toml"))? {
                config = local;
            }
        }
        Ok(config)
    }

    fn read(path: &Path) -> Result<Option<Config>> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text)
                .map(Some)
                .map_err(|e| Error::Config(format!("{}: {e}", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router::TaskKind;

    #[test]
    fn defaults_are_a_usable_configuration() {
        let c = Config::default();
        assert!(c.api.base_url.starts_with("https://"));
        // No token by default: `cymose login` writes one, and shipping a
        // default would mean the binary talks to us before anyone asked it to.
        assert!(c.api.token.is_none());
        assert_eq!(c.router.chain.first().unwrap(), "claude-sonnet");
    }

    #[test]
    fn a_partial_file_keeps_the_defaults_for_everything_else() {
        let text = r#"
            [router]
            chain = ["glm-4.5", "qwen-coder"]

            [router.pin]
            summarize = "qwen-coder"
        "#;
        let c: Config = toml::from_str(text).unwrap();
        assert_eq!(c.router.chain, vec!["glm-4.5", "qwen-coder"]);
        assert_eq!(
            c.router.pin.get(&TaskKind::Summarize).unwrap(),
            "qwen-coder"
        );
        assert_eq!(c.api.base_url, ApiConfig::default().base_url);
    }
}
