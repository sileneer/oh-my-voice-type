use std::fmt;
use std::sync::{LazyLock, Mutex};

const SERVICE: &str = "io.github.sileneer.ohmyvoicetype";
static VAULT_IO: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

fn vault_guard() -> std::sync::MutexGuard<'static, ()> {
    VAULT_IO.lock().unwrap_or_else(|error| error.into_inner())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CredentialError {
    Vault(String),
}

impl fmt::Display for CredentialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Vault(message) => write!(f, "credential vault unavailable: {message}"),
        }
    }
}

impl std::error::Error for CredentialError {}

pub trait CredentialStore: Send + Sync {
    fn load(&self, id: &str) -> Result<Option<String>, CredentialError>;
    fn store(&self, id: &str, secret: &str) -> Result<(), CredentialError>;
    fn delete(&self, id: &str) -> Result<(), CredentialError>;
}

pub struct SystemCredentialStore;

fn account(id: &str) -> String {
    if id == "asr" || id == "llm" {
        id.to_string()
    } else {
        format!("credential:{id}")
    }
}

fn entry(id: &str) -> Result<keyring::Entry, CredentialError> {
    keyring::Entry::new(SERVICE, &account(id)).map_err(|e| CredentialError::Vault(e.to_string()))
}

impl CredentialStore for SystemCredentialStore {
    fn load(&self, id: &str) -> Result<Option<String>, CredentialError> {
        let _guard = vault_guard();
        match entry(id)?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(CredentialError::Vault(e.to_string())),
        }
    }

    fn store(&self, id: &str, secret: &str) -> Result<(), CredentialError> {
        let _guard = vault_guard();
        entry(id)?
            .set_password(secret)
            .map_err(|e| CredentialError::Vault(e.to_string()))
    }

    fn delete(&self, id: &str) -> Result<(), CredentialError> {
        let _guard = vault_guard();
        match entry(id)?.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(CredentialError::Vault(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_accounts_are_stable_and_namespaced() {
        assert_eq!(account("asr"), "asr");
        assert_eq!(account("llm"), "llm");
        assert_eq!(account("abc"), "credential:abc");
    }
}
