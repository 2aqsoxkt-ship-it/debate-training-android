use super::{missing, normalize_key, note_stored, unavailable, SecretStore};
use crate::error::AppError;

/// Every existing AI/analysis/review path resolves this Android implementation.
/// Decrypted credentials never cross the frontend IPC boundary.
pub struct AndroidSecretStore;

impl AndroidSecretStore {
    pub fn app() -> Self {
        Self
    }
}

impl SecretStore for AndroidSecretStore {
    fn save_api_key(&self, key: &str) -> Result<(), AppError> {
        let normalized = normalize_key(key)?;
        crate::platform::call("saveKey", &normalized).map_err(|_| unavailable())?;
        note_stored();
        Ok(())
    }

    fn read_api_key(&self) -> Result<String, AppError> {
        let key = crate::platform::call("readKey", "").map_err(|_| unavailable())?;
        if key.is_empty() {
            return Err(missing());
        }
        Ok(key)
    }

    fn test_api_key(&self) -> Result<(), AppError> {
        self.read_api_key().map(|_| ())
    }

    fn clear_api_key(&self) -> Result<(), AppError> {
        crate::platform::call("clearKey", "").map_err(|_| unavailable())?;
        Ok(())
    }
}
