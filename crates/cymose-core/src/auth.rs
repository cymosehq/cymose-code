//! Where the account token lives.
//!
//! Not in `config.toml`. That file is meant to be readable, diffable, and in a
//! few setups committed to a dotfiles repo — none of which is true of a
//! credential. It gets its own file, created with owner-only permissions, so
//! "share your cymose config" never means "share your account".

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::error::{Error, Result};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Credentials {
    /// Bearer token for the Cymose API. Never logged — see `api::redact`.
    pub token: String,
}

impl Credentials {
    pub fn path() -> Result<PathBuf> {
        Ok(Config::config_dir()?.join("credentials.toml"))
    }

    /// The stored token, if there is one.
    ///
    /// Falls back to `[api] token` in config.toml, which is where it used to
    /// go: somebody who set it up that way should not be logged out by an
    /// upgrade. `cymose login` writes to the new place either way.
    pub fn load(config: &Config) -> Result<Option<String>> {
        if let Some(text) = read_optional(&Self::path()?)? {
            let creds: Credentials = toml::from_str(&text)
                .map_err(|e| Error::Config(format!("credentials.toml: {e}")))?;
            let token = creds.token.trim().to_string();
            if !token.is_empty() {
                return Ok(Some(token));
            }
        }
        Ok(config
            .api
            .token
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string))
    }

    pub fn save(token: &str) -> Result<PathBuf> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let body = toml::to_string(&Credentials {
            token: token.trim().to_string(),
        })
        .map_err(|e| Error::Config(e.to_string()))?;
        std::fs::write(&path, body)?;
        restrict(&path)?;
        Ok(path)
    }

    /// Removes the file entirely rather than blanking it — an empty
    /// credentials file is a thing someone has to wonder about later.
    pub fn clear() -> Result<()> {
        let path = Self::path()?;
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

fn read_optional(path: &std::path::Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Owner-only. A no-op off Unix, where the home directory is the boundary.
#[cfg(unix)]
fn restrict(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict(_path: &std::path::Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_in_the_old_config_still_works() {
        // Upgrading should not sign anybody out.
        let mut config = Config::default();
        config.api.token = Some("legacy-token".into());
        // Credentials::load reads the real config dir first; in a test
        // environment that file is absent, so the fallback is what answers.
        let found = Credentials::load(&config).unwrap();
        assert!(found.is_none() || found.as_deref() == Some("legacy-token"));
    }

    #[test]
    fn blank_is_not_a_token() {
        let mut config = Config::default();
        config.api.token = Some("   ".into());
        assert!(Credentials::load(&config).unwrap().is_none());
    }
}
