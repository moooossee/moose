use super::Provider;
use crate::{
    APPLICATION_ID,
    error::{MooseError, Result},
};
use oo7::{Keyring, Secret};
use std::collections::HashMap;
use zeroize::Zeroizing;

static STORE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
#[derive(Debug)]
pub struct ApiKey(Secret);

impl ApiKey {
    pub fn new(value: String) -> Result<Self> {
        let value = Zeroizing::new(value);
        validate(value.as_bytes())?;
        Ok(Self(Secret::text(value.as_str())))
    }

    pub fn header(&self, bearer: bool) -> Result<reqwest::header::HeaderValue> {
        let bytes = self.0.as_bytes();
        validate(bytes)?;
        let value = if bearer {
            let mut value = Zeroizing::new(b"Bearer ".to_vec());
            value.extend_from_slice(bytes);
            reqwest::header::HeaderValue::from_bytes(&value)
        } else {
            reqwest::header::HeaderValue::from_bytes(bytes)
        };
        let mut value = value.map_err(|_| MooseError::InvalidApiKey)?;
        value.set_sensitive(true);
        Ok(value)
    }
}

fn validate(value: &[u8]) -> Result<()> {
    if !(8..=8192).contains(&value.len()) || !value.iter().all(|b| b.is_ascii_graphic()) {
        return Err(MooseError::InvalidApiKey);
    }
    Ok(())
}

fn attributes(provider: &Provider) -> Result<HashMap<&str, &str>> {
    provider.kind.validate_url(&provider.base_url)?;
    if !provider.kind.requires_key() {
        return Err(MooseError::InvalidProviderUrl);
    }
    Ok(HashMap::from([
        ("application", APPLICATION_ID),
        ("provider-id", provider.id.as_str()),
        ("provider-kind", provider.kind.as_str()),
        ("destination", provider.base_url.as_str()),
    ]))
}
async fn open() -> Result<Keyring> {
    let keyring = if oo7::ashpd::is_sandboxed() {
        let keyring = Keyring::new()
            .await
            .map_err(|_| MooseError::CredentialStoreUnavailable)?;
        if !matches!(keyring, Keyring::File(_)) {
            return Err(MooseError::CredentialStoreUnavailable);
        }
        keyring
    } else {
        let service = oo7::dbus::Service::encrypted()
            .await
            .map_err(|_| MooseError::CredentialStoreUnavailable)?;
        Keyring::DBus(
            service
                .default_collection()
                .await
                .map_err(|_| MooseError::CredentialStoreUnavailable)?,
        )
    };
    keyring
        .unlock()
        .await
        .map_err(|_| MooseError::CredentialStoreUnavailable)?;
    Ok(keyring)
}

pub async fn save(provider: &Provider, key: &ApiKey) -> Result<()> {
    let attributes = attributes(provider)?;
    let _guard = STORE_LOCK.lock().await;
    let keyring = open().await?;
    keyring
        .create_item(
            &format!("Moose · {} API key", provider.kind.label()),
            &attributes,
            key.0.clone(),
            true,
        )
        .await
        .map_err(|_| MooseError::CredentialStoreUnavailable)
}

pub async fn load(provider: &Provider) -> Result<ApiKey> {
    let attributes = attributes(provider)?;
    let _guard = STORE_LOCK.lock().await;
    let keyring = open().await?;
    let items = keyring
        .search_items(&attributes)
        .await
        .map_err(|_| MooseError::CredentialStoreUnavailable)?;
    let item = items.first().ok_or(MooseError::MissingApiKey)?;
    let secret = item
        .secret()
        .await
        .map_err(|_| MooseError::CredentialStoreUnavailable)?;
    validate(secret.as_bytes())?;
    Ok(ApiKey(secret))
}

pub async fn delete(provider: &Provider) -> Result<()> {
    let attributes = attributes(provider)?;
    let _guard = STORE_LOCK.lock().await;
    open()
        .await?
        .delete(&attributes)
        .await
        .map_err(|_| MooseError::CredentialStoreUnavailable)
}
