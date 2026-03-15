use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const COPPER_HOME_DIR: &str = ".Copper";
const CONTROL_PLANE_DIR: &str = "control-plane";
const AUTH_FILE: &str = "auth.json";
pub const IPC_AUTH_FIELD: &str = "token";
pub const UI_AUTH_HEADER: &str = "x-copper-token";

#[derive(Debug, Clone)]
pub struct ControlPlaneAuth {
    token: String,
    persisted: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedAuth {
    token: String,
}

impl ControlPlaneAuth {
    pub fn ephemeral() -> Self {
        Self {
            token: generate_token(),
            persisted: false,
        }
    }

    pub fn ensure_persisted() -> Result<Self, std::io::Error> {
        let path = auth_file_path()?;
        if path.exists() {
            let raw = fs::read_to_string(&path)?;
            if let Ok(saved) = serde_json::from_str::<PersistedAuth>(&raw) {
                if !saved.token.trim().is_empty() {
                    return Ok(Self {
                        token: saved.token,
                        persisted: true,
                    });
                }
            }
        }

        let auth = Self {
            token: generate_token(),
            persisted: true,
        };
        auth.persist()?;
        Ok(auth)
    }

    pub fn load_persisted_token() -> Result<Option<String>, std::io::Error> {
        let path = auth_file_path()?;
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(path)?;
        let saved: PersistedAuth = serde_json::from_str(&raw)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
        Ok(Some(saved.token))
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn persist(&self) -> Result<(), std::io::Error> {
        if !self.persisted {
            return Ok(());
        }

        let path = auth_file_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(
            path,
            serde_json::to_string_pretty(&PersistedAuth {
                token: self.token.clone(),
            })
            .map_err(std::io::Error::other)?,
        )
    }
}

fn auth_file_path() -> Result<PathBuf, std::io::Error> {
    let home = dirs::home_dir().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "home directory not available")
    })?;
    Ok(home
        .join(COPPER_HOME_DIR)
        .join(CONTROL_PLANE_DIR)
        .join(AUTH_FILE))
}

fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::ControlPlaneAuth;

    #[test]
    fn ephemeral_auth_generates_token() {
        let auth = ControlPlaneAuth::ephemeral();
        assert_eq!(auth.token().len(), 64);
    }
}
