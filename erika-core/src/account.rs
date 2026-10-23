//! The dandanplay account. Signing in runs on another thread and comes back
//! through a wake; the token is kept in the system's credential store, the
//! names in the database. A token three weeks old is renewed, as NipaPlay
//! does. The anime module hears of every change and loads what the account
//! lets it; the library matches the videos it could not without one.

use super::*;
use std::sync::Mutex;

/// The secret holding the account's token, and the one holding the
/// address of the user's own server, whose path holds its token.
const TOKEN: &str = "dandanplay-token";
const SERVER: &str = "match-server";
/// The settings holding its names and when the token was last renewed.
const USER: &str = "dandanplay.user";
const NAME: &str = "dandanplay.name";
const RENEWED: &str = "dandanplay.renewed";
/// How old a token gets before it is renewed, in seconds.
const RENEW_AFTER: i64 = 21 * 24 * 3600;

/// What the account's work on other threads came back with.
pub(super) enum Done {
    SignedIn(erika_dandanplay::Account),
    Failed(String),
    Renewed(erika_dandanplay::Renewed),
}

#[cfg(test)]
mod tests {
    use super::*;
    use erika::{Commands, Engine};

    #[test]
    fn disabling_account_preserves_saved_login_and_rejects_commands() {
        let mut engine = ErikaEngine::default();
        engine.db.set_setting(USER, Some("saved-user"));
        engine.db.set_setting(NAME, Some("Saved Name"));
        engine.state.account_status.signed_in = true;
        engine.state.account_status.busy = true;
        engine.state.account_status.name = "Saved Name".into();
        engine.state.account_status.has_server = true;
        engine.state.features.account = true;

        engine.modules_changed(&[]);

        assert!(!engine.state.features.account);
        assert!(!engine.state.account_status.signed_in);
        assert!(!engine.state.account_status.busy);
        assert!(!engine.state.account_status.has_server);
        assert_eq!(engine.db.setting(USER).as_deref(), Some("saved-user"));
        assert_eq!(engine.db.setting(NAME).as_deref(), Some("Saved Name"));
        assert_eq!(
            engine
                .sign_in("user".into(), "password".into())
                .unwrap_err()
                .code,
            "feature_disabled"
        );
        assert_eq!(engine.sign_out().unwrap_err().code, "feature_disabled");
        assert_eq!(
            engine
                .set_match_server("https://example.com".into())
                .unwrap_err()
                .code,
            "feature_disabled"
        );
        assert_eq!(
            engine.clear_match_server().unwrap_err().code,
            "feature_disabled"
        );
    }

    #[test]
    fn results_from_before_disable_cannot_restore_login_after_reenabling() {
        let mut engine = ErikaEngine::default();
        engine.state.features.account = true;
        let old_inbox = Arc::clone(&engine.account_inbox);
        engine.modules_changed(&[]);
        engine.modules_changed(&["account".into()]);
        assert!(!Arc::ptr_eq(&old_inbox, &engine.account_inbox));

        old_inbox
            .lock()
            .unwrap()
            .push(Done::SignedIn(erika_dandanplay::Account {
                token: "expired-request-token".into(),
                user_name: "late-user".into(),
                screen_name: "Late User".into(),
            }));
        old_inbox
            .lock()
            .unwrap()
            .push(Done::Failed("old error".into()));
        engine.take_account_results();

        assert!(!engine.state.account_status.signed_in);
        assert!(engine.state.account_status.error.is_empty());
        assert_eq!(engine.db.setting(USER), None);
    }
}

/// The results waiting for the engine.
pub(super) type Inbox = Arc<Mutex<Vec<Done>>>;

/// The name the sidebar shows when no account is signed in.
fn local_user() -> String {
    "本地用户".into()
}

/// The account as the device kept it, with its token when one is signed in.
/// An account still named by what was typed to sign in has its token
/// renewed at once, which brings the name others see.
pub(super) fn restore(db: &erika_store::Db) -> (Account, Option<String>) {
    let token = bern::secrets::read(TOKEN)
        .ok()
        .flatten()
        .filter(|token| !token.is_empty());
    let server = bern::secrets::read(SERVER)
        .ok()
        .flatten()
        .filter(|address| !address.is_empty());
    erika_dandanplay::custom::set_server(server.clone());
    if token.is_some() && db.setting(NAME) == db.setting(USER) {
        db.set_setting(RENEWED, None);
    }
    let account = Account {
        signed_in: token.is_some(),
        name: token
            .as_ref()
            .and_then(|_| db.setting(NAME))
            .unwrap_or_default(),
        user: token
            .as_ref()
            .and_then(|_| db.setting(USER))
            .unwrap_or_default(),
        busy: false,
        error: String::new(),
        has_server: server.is_some(),
        server: server.as_deref().map(site).unwrap_or_default(),
    };
    (account, token)
}

/// The scheme and host of a server's address: its path holds its token,
/// which the settings do not show.
fn site(address: &str) -> String {
    let (scheme, rest) = address.split_once("://").unwrap_or(("https", address));
    format!("{scheme}://{}", rest.split('/').next().unwrap_or(rest))
}

pub(super) fn display_name(account: &Account) -> String {
    if account.signed_in && !account.name.is_empty() {
        account.name.clone()
    } else {
        local_user()
    }
}

impl ErikaEngine {
    pub(super) fn require_account(&self) -> Result<(), CommandError> {
        if self.state.features.account {
            Ok(())
        } else {
            Err(CommandError::new(
                "feature_disabled",
                "请先在可选功能中启用账号",
            ))
        }
    }

    pub(super) fn restore_account(&mut self) {
        let (account, token) = restore(&self.db);
        erika_dandanplay::set_token(token);
        self.state.user_name = display_name(&account);
        self.state.account_status = account;
    }

    /// Suspend account use without deleting the saved login. A fresh inbox
    /// also discards work started before this module was switched off.
    pub(super) fn set_account_enabled(&mut self, enabled: bool) {
        self.account_inbox = Arc::default();
        if enabled {
            self.restore_account();
        } else {
            erika_dandanplay::set_token(None);
            erika_dandanplay::custom::set_server(None);
            self.state.account_status = Account::default();
            self.state.user_name = local_user();
        }
        #[cfg(feature = "bern-module-anime")]
        erika_anime_protocol::anime::calls::account_changed(self.state.account_status.signed_in);
        #[cfg(feature = "bern-module-library")]
        erika_library_protocol::library::calls::account_changed(
            self.state.account_status.signed_in || self.state.account_status.has_server,
        );
    }

    pub(super) fn sign_in_started(&mut self, user: String, password: String) {
        if !self.state.features.account || self.state.account_status.busy {
            return;
        }
        self.state.account_status.busy = true;
        self.state.account_status.error.clear();
        in_background(&self.account_inbox, move || {
            vec![match erika_dandanplay::login(&user, &password) {
                Ok(account) => Done::SignedIn(account),
                Err(error) => Done::Failed(error.message),
            }]
        });
    }

    pub(super) fn sign_out_now(&mut self) {
        self.account_inbox = Arc::default();
        if let Err(error) = bern::secrets::delete(TOKEN) {
            eprintln!("erika: forget the dandanplay token: {}", error.message);
        }
        for key in [USER, NAME, RENEWED] {
            self.db.set_setting(key, None);
        }
        erika_dandanplay::set_token(None);
        self.state.account_status = Account {
            signed_in: false,
            name: String::new(),
            user: String::new(),
            busy: false,
            error: String::new(),
            ..self.state.account_status.clone()
        };
        self.state.user_name = local_user();
        self.notify(Notice::SignedOut);
        #[cfg(feature = "bern-module-anime")]
        erika_anime_protocol::anime::calls::account_changed(false);
        #[cfg(feature = "bern-module-library")]
        erika_library_protocol::library::calls::account_changed(false);
    }

    /// Take in what signing in or renewing came back with.
    pub(super) fn take_account_results(&mut self) {
        let done: Vec<Done> = std::mem::take(
            &mut *self
                .account_inbox
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        );
        if !self.state.features.account {
            return;
        }
        for done in done {
            match done {
                Done::SignedIn(account) => {
                    if let Err(error) = bern::secrets::write(TOKEN, &account.token) {
                        eprintln!("erika: keep the dandanplay token: {}", error.message);
                    }
                    self.db.set_setting(USER, Some(&account.user_name));
                    self.db.set_setting(NAME, Some(&account.screen_name));
                    self.db
                        .set_setting(RENEWED, Some(&erika_store::now().to_string()));
                    erika_dandanplay::set_token(Some(account.token));
                    self.state.account_status = Account {
                        signed_in: true,
                        name: account.screen_name.clone(),
                        user: account.user_name,
                        busy: false,
                        error: String::new(),
                        ..self.state.account_status.clone()
                    };
                    self.state.user_name = account.screen_name.clone();
                    self.notify(Notice::SignedIn {
                        name: account.screen_name,
                    });
                    #[cfg(feature = "bern-module-anime")]
                    erika_anime_protocol::anime::calls::account_changed(true);
                    #[cfg(feature = "bern-module-library")]
                    erika_library_protocol::library::calls::account_changed(true);
                }
                Done::Failed(message) => {
                    self.state.account_status.busy = false;
                    self.state.account_status.error = message;
                }
                Done::Renewed(renewed) => {
                    if !self.state.account_status.signed_in {
                        continue;
                    }
                    if bern::secrets::write(TOKEN, &renewed.token).is_ok() {
                        self.db
                            .set_setting(RENEWED, Some(&erika_store::now().to_string()));
                        erika_dandanplay::set_token(Some(renewed.token));
                    }
                    if let Some(name) = renewed
                        .screen_name
                        .filter(|name| *name != self.state.account_status.name)
                    {
                        self.db.set_setting(NAME, Some(&name));
                        self.state.account_status.name = name.clone();
                        self.state.user_name = name;
                    }
                }
            }
        }
    }

    pub(super) fn set_match_server_now(&mut self, address: String) {
        let address = address.trim().trim_end_matches('/').to_string();
        if !(address.starts_with("http://") || address.starts_with("https://")) {
            self.notify(Notice::MatchServerInvalid);
            return;
        }
        if let Err(error) = bern::secrets::write(SERVER, &address) {
            eprintln!("erika: keep the recognition server: {}", error.message);
        }
        erika_dandanplay::custom::set_server(Some(address.clone()));
        self.state.account_status.has_server = true;
        self.state.account_status.server = site(&address);
        self.notify(Notice::MatchServerSet);
        // The library matches what it could not without one.
        #[cfg(feature = "bern-module-library")]
        erika_library_protocol::library::calls::account_changed(true);
    }

    pub(super) fn clear_match_server_now(&mut self) {
        if let Err(error) = bern::secrets::delete(SERVER) {
            eprintln!("erika: forget the recognition server: {}", error.message);
        }
        erika_dandanplay::custom::set_server(None);
        self.state.account_status.has_server = false;
        self.state.account_status.server.clear();
        self.notify(Notice::MatchServerCleared);
    }

    /// Renew the token once it is three weeks old.
    pub(super) fn renew_when_old(&mut self) {
        if !self.state.features.account {
            return;
        }
        let Some(token) = erika_dandanplay::token() else {
            return;
        };
        let renewed = self
            .db
            .setting(RENEWED)
            .and_then(|at| at.parse::<i64>().ok())
            .unwrap_or(0);
        if erika_store::now() - renewed < RENEW_AFTER {
            return;
        }
        // Not again while this one is under way.
        self.db
            .set_setting(RENEWED, Some(&erika_store::now().to_string()));
        in_background(&self.account_inbox, move || {
            match erika_dandanplay::renew(&token) {
                Ok(renewed) => vec![Done::Renewed(renewed)],
                // An old token goes on working until dandanplay refuses it.
                Err(error) => {
                    eprintln!("erika: renew the dandanplay token: {}", error.message);
                    Vec::new()
                }
            }
        });
    }
}
