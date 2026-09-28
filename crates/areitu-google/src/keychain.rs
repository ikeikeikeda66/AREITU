pub trait TokenStore {
    fn save_refresh_token(&self, refresh_token: &str) -> crate::Result<()>;
    fn load_refresh_token(&self) -> crate::Result<Option<String>>;
    fn clear_refresh_token(&self) -> crate::Result<()>;
}

const SERVICE: &str = "com.areitu.google";
const ACCOUNT: &str = "refresh_token";

pub struct KeyringStore;

impl TokenStore for KeyringStore {
    fn save_refresh_token(&self, refresh_token: &str) -> crate::Result<()> {
        let entry = keyring::Entry::new(SERVICE, ACCOUNT).map_err(|e| crate::Error::Keychain(e.to_string()))?;
        entry.set_password(refresh_token).map_err(|e| crate::Error::Keychain(e.to_string()))
    }

    fn load_refresh_token(&self) -> crate::Result<Option<String>> {
        let entry = keyring::Entry::new(SERVICE, ACCOUNT).map_err(|e| crate::Error::Keychain(e.to_string()))?;
        match entry.get_password() {
            Ok(pw) => Ok(Some(pw)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(crate::Error::Keychain(e.to_string())),
        }
    }

    fn clear_refresh_token(&self) -> crate::Result<()> {
        let entry = keyring::Entry::new(SERVICE, ACCOUNT).map_err(|e| crate::Error::Keychain(e.to_string()))?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(crate::Error::Keychain(e.to_string())),
        }
    }
}

#[cfg(test)]
pub struct InMemoryStore(std::sync::Mutex<Option<String>>);

#[cfg(test)]
impl InMemoryStore {
    pub fn new() -> Self {
        InMemoryStore(std::sync::Mutex::new(None))
    }
}

#[cfg(test)]
impl Default for InMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl TokenStore for InMemoryStore {
    fn save_refresh_token(&self, refresh_token: &str) -> crate::Result<()> {
        *self.0.lock().unwrap() = Some(refresh_token.to_owned());
        Ok(())
    }

    fn load_refresh_token(&self) -> crate::Result<Option<String>> {
        Ok(self.0.lock().unwrap().clone())
    }

    fn clear_refresh_token(&self) -> crate::Result<()> {
        *self.0.lock().unwrap() = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_memory_store_round_trips() {
        let store = InMemoryStore::new();
        assert_eq!(store.load_refresh_token().unwrap(), None);
        store.save_refresh_token("token-1").unwrap();
        assert_eq!(store.load_refresh_token().unwrap(), Some("token-1".to_owned()));
        store.clear_refresh_token().unwrap();
        assert_eq!(store.load_refresh_token().unwrap(), None);
    }

    #[test]
    fn clearing_an_already_empty_store_is_not_an_error() {
        let store = InMemoryStore::new();
        assert!(store.clear_refresh_token().is_ok());
    }

    #[test]
    #[ignore = "touches the real OS keychain / secret service"]
    fn keyring_store_round_trips_on_this_machine() {
        let store = KeyringStore;
        store.save_refresh_token("areitu-test-token").unwrap();
        assert_eq!(store.load_refresh_token().unwrap(), Some("areitu-test-token".to_owned()));
        store.clear_refresh_token().unwrap();
        assert_eq!(store.load_refresh_token().unwrap(), None);
    }
}
