use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use super::{Provider, ProviderKind, validate_managed_provider_base_url};
use crate::error::{MooseError, Result};

#[derive(Clone)]
pub struct NetworkPolicy(Arc<AtomicBool>);

impl NetworkPolicy {
    pub fn new(local_only: bool) -> Self {
        Self(Arc::new(AtomicBool::new(local_only)))
    }
    pub fn local_only(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
    pub fn set_local_only(&self, value: bool) {
        self.0.store(value, Ordering::SeqCst);
    }
    pub fn check(&self, provider: &Provider) -> Result<()> {
        if self.local_only() && (!provider.is_managed || provider.kind != ProviderKind::Ollama) {
            return Err(MooseError::LocalOnly);
        }
        if provider.is_managed {
            if provider.kind != ProviderKind::Ollama {
                return Err(MooseError::InvalidProviderUrl);
            }
            validate_managed_provider_base_url(&provider.base_url)?;
        } else {
            provider.kind.validate_url(&provider.base_url)?;
        }
        Ok(())
    }
}

pub fn destination(provider: &Provider) -> String {
    format!(
        "{}|{}|{}",
        provider.id,
        provider.kind.as_str(),
        provider.base_url
    )
}

#[derive(Clone, Copy, Default)]
pub struct RemotePermissions {
    pub messages: bool,
    pub files: bool,
}
