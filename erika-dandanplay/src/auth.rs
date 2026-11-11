//! Immutable credentials for an account operation. Never serialize or log a
//! session; persist only its non-secret account key in the delivery outbox.
use crate::{Error, Result};
use std::sync::RwLock;

struct State {
    account: Option<String>,
    secret: Option<String>,
    generation: u64,
}
/// An isolated account credential source. A captured session remains tied to
/// this registry; replacing its account or secret invalidates all older work.
pub struct Registry(RwLock<State>);
impl Registry {
    pub const fn new() -> Self {
        Self(RwLock::new(State {
            account: None,
            secret: None,
            generation: 0,
        }))
    }
    pub fn set(&self, account: Option<String>, secret: Option<String>) {
        let secret = secret.filter(|s| !s.is_empty());
        let account = account
            .filter(|a| !a.trim().is_empty() && a.len() <= 256 && !a.chars().any(char::is_control));
        let mut state = self.0.write().unwrap_or_else(|p| p.into_inner());
        if state.account != account || state.secret != secret {
            state.generation = state.generation.wrapping_add(1);
            state.account = account;
            state.secret = secret;
        }
    }
    pub fn secret(&self) -> Option<String> {
        self.0
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .secret
            .clone()
    }
    pub fn capture(&'static self) -> Option<Session> {
        let state = self.0.read().unwrap_or_else(|p| p.into_inner());
        Some(Session {
            registry: self,
            generation: state.generation,
            account: state.account.clone()?,
            secret: state.secret.clone()?,
        })
    }
}

#[derive(Clone)]
pub struct Session {
    registry: &'static Registry,
    generation: u64,
    account: String,
    pub(crate) secret: String,
}
impl Session {
    pub fn account(&self) -> &str {
        &self.account
    }
    pub fn is_current(&self) -> bool {
        self.registry
            .0
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .generation
            == self.generation
    }
    pub(crate) fn check(&self) -> Result<()> {
        if self.is_current() {
            Ok(())
        } else {
            Err(Error::new("账号已切换，请重试"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn switching_away_and_back_does_not_revalidate_old_work() {
        let registry: &'static Registry = Box::leak(Box::new(Registry::new()));
        registry.set(Some("provider:first".into()), Some("test-a".into()));
        let first = registry.capture().unwrap();
        registry.set(Some("provider:second".into()), Some("test-b".into()));
        assert!(!first.is_current());
        assert_eq!(first.secret, "test-a");
        registry.set(Some("provider:first".into()), Some("test-a".into()));
        assert!(!first.is_current());
        assert!(registry.capture().unwrap().is_current());
        registry.set(None, None);
        assert!(registry.capture().is_none());
    }
    #[test]
    fn unidentified_credentials_do_not_authorize_account_writes() {
        let registry: &'static Registry = Box::leak(Box::new(Registry::new()));
        registry.set(None, Some("test-token".into()));
        assert!(registry.secret().is_some());
        assert!(registry.capture().is_none());
    }
}
