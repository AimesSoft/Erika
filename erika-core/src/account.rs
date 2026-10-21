//! The dandanplay account. Signing in runs on another thread and comes back
//! through a wake; the token is kept in the system's credential store, the
//! names in the database. A token three weeks old is renewed, as NipaPlay
//! does. The anime module hears of every change and loads what the account
//! lets it; the library matches the videos it could not without one.

use super::*;
use std::sync::Mutex;

/// The secret holding the account's token.
const TOKEN: &str = "dandanplay-token";
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
    let token = bern::secrets::read(TOKEN).ok().flatten().filter(|token| !token.is_empty());
    if token.is_some() && db.setting(NAME) == db.setting(USER) {
        db.set_setting(RENEWED, None);
    }
    let account = Account {
        signed_in: token.is_some(),
        name: token.as_ref().and_then(|_| db.setting(NAME)).unwrap_or_default(),
        user: token.as_ref().and_then(|_| db.setting(USER)).unwrap_or_default(),
        busy: false,
        error: String::new(),
    };
    (account, token)
}

pub(super) fn display_name(account: &Account) -> String {
    if account.signed_in && !account.name.is_empty() { account.name.clone() } else { local_user() }
}

impl ErikaEngine {
    pub(super) fn sign_in_started(&mut self, user: String, password: String) {
        if self.state.account.busy {
            return;
        }
        self.state.account.busy = true;
        self.state.account.error.clear();
        in_background(&self.account_inbox, move || {
            vec![match erika_dandanplay::login(&user, &password) {
                Ok(account) => Done::SignedIn(account),
                Err(error) => Done::Failed(error.message),
            }]
        });
    }

    pub(super) fn sign_out_now(&mut self) {
        if let Err(error) = bern::secrets::delete(TOKEN) {
            eprintln!("erika: forget the dandanplay token: {}", error.message);
        }
        for key in [USER, NAME, RENEWED] {
            self.db.set_setting(key, None);
        }
        erika_dandanplay::set_token(None);
        self.state.account = Account { signed_in: false, name: String::new(), user: String::new(), busy: false, error: String::new() };
        self.state.user_name = local_user();
        self.notify(Notice::SignedOut);
        erika_anime_protocol::anime::calls::account_changed(false);
        erika_library_protocol::library::calls::account_changed(false);
    }

    /// Take in what signing in or renewing came back with.
    pub(super) fn take_account_results(&mut self) {
        let done: Vec<Done> = std::mem::take(&mut *self.account_inbox.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
        for done in done {
            match done {
                Done::SignedIn(account) => {
                    if let Err(error) = bern::secrets::write(TOKEN, &account.token) {
                        eprintln!("erika: keep the dandanplay token: {}", error.message);
                    }
                    self.db.set_setting(USER, Some(&account.user_name));
                    self.db.set_setting(NAME, Some(&account.screen_name));
                    self.db.set_setting(RENEWED, Some(&erika_store::now().to_string()));
                    erika_dandanplay::set_token(Some(account.token));
                    self.state.account = Account {
                        signed_in: true,
                        name: account.screen_name.clone(),
                        user: account.user_name,
                        busy: false,
                        error: String::new(),
                    };
                    self.state.user_name = account.screen_name.clone();
                    self.notify(Notice::SignedIn { name: account.screen_name });
                    erika_anime_protocol::anime::calls::account_changed(true);
                    erika_library_protocol::library::calls::account_changed(true);
                }
                Done::Failed(message) => {
                    self.state.account.busy = false;
                    self.state.account.error = message;
                }
                Done::Renewed(renewed) => {
                    if !self.state.account.signed_in {
                        continue;
                    }
                    if bern::secrets::write(TOKEN, &renewed.token).is_ok() {
                        self.db.set_setting(RENEWED, Some(&erika_store::now().to_string()));
                        erika_dandanplay::set_token(Some(renewed.token));
                    }
                    if let Some(name) = renewed.screen_name.filter(|name| *name != self.state.account.name) {
                        self.db.set_setting(NAME, Some(&name));
                        self.state.account.name = name.clone();
                        self.state.user_name = name;
                    }
                }
            }
        }
    }

    /// Renew the token once it is three weeks old.
    pub(super) fn renew_when_old(&mut self) {
        let Some(token) = erika_dandanplay::token() else {
            return;
        };
        let renewed = self.db.setting(RENEWED).and_then(|at| at.parse::<i64>().ok()).unwrap_or(0);
        if erika_store::now() - renewed < RENEW_AFTER {
            return;
        }
        // Not again while this one is under way.
        self.db.set_setting(RENEWED, Some(&erika_store::now().to_string()));
        in_background(&self.account_inbox, move || match erika_dandanplay::renew(&token) {
            Ok(renewed) => vec![Done::Renewed(renewed)],
            // An old token goes on working until dandanplay refuses it.
            Err(error) => {
                eprintln!("erika: renew the dandanplay token: {}", error.message);
                Vec::new()
            }
        });
    }
}
