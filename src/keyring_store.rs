use crate::error::{Error, Result};

pub fn default_value(hostname: &str, app_id: u64) -> String {
    format!("gh-app/{hostname}/{app_id}")
}

pub fn store(value: &str, pem: &[u8]) -> Result<()> {
    entry(value)?
        .set_secret(pem)
        .map_err(|e| Error::message(format!("could not write keyring entry: {e}")))
}

pub fn load(value: &str) -> Result<Vec<u8>> {
    entry(value)?
        .get_secret()
        .map_err(|e| Error::message(format!("could not read keyring entry: {e}")))
}

pub fn delete(value: &str) -> Result<()> {
    match entry(value)?.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(Error::message(format!(
            "could not delete keyring entry: {error}"
        ))),
    }
}

fn entry(value: &str) -> Result<keyring::Entry> {
    let (service, username) = value
        .split_once('/')
        .ok_or_else(|| Error::message("keyring value must be SERVICE/USERNAME"))?;
    keyring::Entry::new(service, username)
        .map_err(|e| Error::message(format!("could not open keyring: {e}")))
}
