use std::sync::Mutex;

use crate::error::{AppError, ErrorCode};

pub trait SecretStore {
    fn save_api_key(&self, key: &str) -> Result<(), AppError>;
    fn read_api_key(&self) -> Result<String, AppError>;
    fn test_api_key(&self) -> Result<(), AppError>;
    fn clear_api_key(&self) -> Result<(), AppError>;
}

#[derive(Debug, Default)]
pub struct MemorySecretStore {
    value: Mutex<Option<String>>,
}

impl MemorySecretStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SecretStore for MemorySecretStore {
    fn save_api_key(&self, key: &str) -> Result<(), AppError> {
        let key = normalize_key(key)?;
        *self.value.lock().map_err(|_| internal())? = Some(key);
        note_stored();
        Ok(())
    }

    fn read_api_key(&self) -> Result<String, AppError> {
        self.value
            .lock()
            .map_err(|_| internal())?
            .clone()
            .ok_or_else(missing)
    }

    fn test_api_key(&self) -> Result<(), AppError> {
        let key = self.read_api_key()?;
        if key.is_empty() {
            return Err(missing());
        }
        Ok(())
    }

    fn clear_api_key(&self) -> Result<(), AppError> {
        *self.value.lock().map_err(|_| internal())? = None;
        Ok(())
    }
}

#[cfg(windows)]
pub struct KeyringSecretStore {
    service: String,
    user: String,
}

#[cfg(windows)]
impl KeyringSecretStore {
    pub fn app() -> Self {
        Self::new("com.debate.training.android", "deepseek-api-key")
    }

    pub fn new(service: impl Into<String>, user: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            user: user.into(),
        }
    }

    fn entry(&self) -> Result<keyring::Entry, AppError> {
        keyring::Entry::new(&self.service, &self.user).map_err(|_| unavailable())
    }
}

#[cfg(windows)]
impl SecretStore for KeyringSecretStore {
    fn save_api_key(&self, key: &str) -> Result<(), AppError> {
        let key = normalize_key(key)?;
        self.entry()?
            .set_password(&key)
            .map_err(|_| unavailable())?;
        note_stored();
        Ok(())
    }

    fn read_api_key(&self) -> Result<String, AppError> {
        match self.entry()?.get_password() {
            Ok(value) => Ok(value),
            Err(keyring::Error::NoEntry) => Err(missing()),
            Err(_) => Err(unavailable()),
        }
    }

    fn test_api_key(&self) -> Result<(), AppError> {
        let key = self.read_api_key()?;
        if key.is_empty() {
            return Err(missing());
        }
        Ok(())
    }

    fn clear_api_key(&self) -> Result<(), AppError> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(unavailable()),
        }
    }
}

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
pub use android::AndroidSecretStore as KeyringSecretStore;

fn normalize_key(key: &str) -> Result<String, AppError> {
    if key.is_empty() || key.chars().any(char::is_whitespace) {
        return Err(AppError::new(
            ErrorCode::InvalidArgument,
            "api key is invalid",
        ));
    }
    Ok(key.to_owned())
}

fn note_stored() {
    tracing::info!("api key stored");
}

fn missing() -> AppError {
    AppError::new(ErrorCode::NotFound, "api key was not found")
}

fn unavailable() -> AppError {
    AppError::new(ErrorCode::Unavailable, "credential manager failed")
}

fn internal() -> AppError {
    AppError::new(ErrorCode::Internal, "credential store failed")
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    use super::{KeyringSecretStore, MemorySecretStore, SecretStore};
    use crate::error::ErrorCode;

    #[derive(Clone)]
    struct SharedWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("log").extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for SharedWriter {
        type Writer = SharedWriter;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    #[test]
    fn storing_an_api_key_does_not_log_the_secret() {
        let secret = "sk-phase2-log-secret";
        let writer = SharedWriter(Arc::new(Mutex::new(Vec::new())));
        let subscriber = tracing_subscriber::fmt()
            .with_writer(writer.clone())
            .with_ansi(false)
            .finish();
        let store = MemorySecretStore::new();
        tracing::subscriber::with_default(subscriber, || {
            store.save_api_key(secret).expect("save");
        });
        let text = String::from_utf8(writer.0.lock().expect("log").clone()).expect("utf8");
        assert!(text.contains("api key stored"));
        assert!(!text.contains(secret));
        assert!(!text.contains("sk-phase2"));
    }

    #[test]
    #[cfg(windows)]
    fn windows_credential_manager_round_trip() {
        let secret = "sk-phase2-win-secret";
        let store = KeyringSecretStore::new("com.debate.training.android", "phase2-test-key");
        let _ = store.clear_api_key();
        store.save_api_key(secret).expect("save");
        store.test_api_key().expect("test");
        assert_eq!(store.read_api_key().expect("read"), secret);
        store.clear_api_key().expect("clear");
        assert_eq!(
            store.read_api_key().expect_err("cleared").code,
            ErrorCode::NotFound
        );
        store.clear_api_key().expect("clear again");
    }
}
