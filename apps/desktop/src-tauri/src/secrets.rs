//! OS keyring helpers for provider secrets (prefer over TOML api_key).

const SERVICE: &str = "grok-desktop";

pub fn store_secret(key_name: &str, secret: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE, key_name).map_err(|e| e.to_string())?;
    entry.set_password(secret).map_err(|e| e.to_string())
}

pub fn load_secret(key_name: &str) -> Result<Option<String>, String> {
    let entry = keyring::Entry::new(SERVICE, key_name).map_err(|e| e.to_string())?;
    match entry.get_password() {
        Ok(p) => Ok(Some(p)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

pub fn delete_secret(key_name: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE, key_name).map_err(|e| e.to_string())?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
