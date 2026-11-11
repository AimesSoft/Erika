//! The dandanplay account. Signing in runs on another thread and comes back
//! through a wake; the token is kept in the system's credential store, the
//! names in the database. A token three weeks old is renewed, as NipaPlay
//! does. The anime module hears of every change and loads what the account
//! lets it; the library matches the videos it could not without one.

use super::*;
use std::sync::Mutex;

/// The secret holding the account's token, and the one that held the
/// recognition server's address before it was built in.
const TOKEN: &str = "dandanplay-token";
const OLD_SERVER: &str = "match-server";
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
    Renewed(String, erika_dandanplay::Renewed),
}

#[cfg(test)]
mod tests {
    use super::*;
    use erika::{Commands, Engine};

    #[test]
    fn outer_identity_falls_back_without_mixing_account_names_and_pictures() {
        for (dandan_signed_in, dandan_url, bangumi_signed_in, bangumi_url, expected, name, url) in [
            (true, Some("dandan.png"), true, Some("bangumi.png"), ProfileAccount::Dandanplay, "Dandan Name", Some("dandan.png")),
            (true, None, true, Some("bangumi.png"), ProfileAccount::Bangumi, "Bangumi Name", Some("bangumi.png")),
            (true, Some("  "), true, Some("bangumi.png"), ProfileAccount::Bangumi, "Bangumi Name", Some("bangumi.png")),
            (false, Some("stale.png"), true, Some("bangumi.png"), ProfileAccount::Bangumi, "Bangumi Name", Some("bangumi.png")),
            (true, None, false, Some("stale.png"), ProfileAccount::Dandanplay, "Dandan Name", None),
            (true, None, true, None, ProfileAccount::Dandanplay, "Dandan Name", None),
            (false, None, true, None, ProfileAccount::Bangumi, "Bangumi Name", None),
            (false, Some("stale.png"), false, Some("stale.png"), ProfileAccount::Local, "本地用户", None),
        ] {
            let dandan = Account { signed_in: dandan_signed_in, name: "Dandan Name".into(), avatar: dandan_url.map(MediaSource::new), ..Account::default() };
            let bangumi = BangumiAccount { signed_in: bangumi_signed_in, name: "Bangumi Name".into(), avatar: bangumi_url.map(MediaSource::new), ..BangumiAccount::default() };
            let actual = profile(&dandan, &bangumi);
            assert_eq!(actual.0, name);
            assert_eq!(actual.1, expected);
            assert_eq!(actual.2.as_ref().map(|p| p.url.as_str()), url);
        }
    }

    #[test]
    fn qq_email_avatars_follow_nipaplay_with_https() {
        for user in ["12345678@qq.com", " 12345678@QQ.COM "] {
            assert_eq!(
                avatar_for_user(user),
                Some(MediaSource::new(
                    "https://q.qlogo.cn/headimg_dl?dst_uin=12345678&spec=640"
                ))
            );
        }
    }

    #[test]
    fn other_accounts_keep_the_default_avatar() {
        for user in [
            "",
            "nickname",
            "12345678@example.com",
            "12345678@qq.com.example.com",
            "@qq.com",
            "nickname@qq.com",
            "12&spec=1@qq.com",
        ] {
            assert_eq!(avatar_for_user(user), None, "{user}");
        }
    }

    #[test]
    fn disabling_account_preserves_saved_login_and_rejects_commands() {
        let mut engine = ErikaEngine::default();
        engine.db.set_setting(USER, Some("saved-user"));
        engine.db.set_setting(NAME, Some("Saved Name"));
        engine.state.account_status.signed_in = true;
        engine.state.account_status.busy = true;
        engine.state.account_status.name = "Saved Name".into();
        engine.state.features.account = true;

        engine.modules_changed(&[]);

        assert!(!engine.state.features.account);
        assert!(!engine.state.account_status.signed_in);
        assert!(!engine.state.account_status.busy);
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

/// NipaPlay derives a QQ avatar from the email used to sign in. HTTPS
/// works on Android and secure web pages without cleartext exceptions.
fn avatar_for_user(user: &str) -> Option<MediaSource> {
    let (number, domain) = user.trim().split_once('@')?;
    if !domain.eq_ignore_ascii_case("qq.com")
        || number.is_empty()
        || !number.bytes().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    Some(MediaSource::new(format!(
        "https://q.qlogo.cn/headimg_dl?dst_uin={number}&spec=640"
    )))
}

/// The account as the device kept it, with its token when one is signed in.
/// An account still named by what was typed to sign in has its token
/// renewed at once, which brings the name others see.
pub(super) fn restore(db: &erika_store::Db) -> (Account, Option<String>) {
    let token = bern::secrets::read(TOKEN)
        .ok()
        .flatten()
        .filter(|token| !token.is_empty());
    if bern::secrets::read(OLD_SERVER).ok().flatten().is_some()
        && let Err(error) = bern::secrets::delete(OLD_SERVER)
    {
        eprintln!("erika: forget the old recognition server: {}", error.message);
    }
    if token.is_some() && db.setting(NAME) == db.setting(USER) {
        db.set_setting(RENEWED, None);
    }
    let user = token
        .as_ref()
        .and_then(|_| db.setting(USER))
        .unwrap_or_default();
    let account = Account {
        signed_in: token.is_some(),
        prompt: false,
        settings_window: false,
        name: token
            .as_ref()
            .and_then(|_| db.setting(NAME))
            .unwrap_or_default(),
        avatar: avatar_for_user(&user),
        user,
        busy: false,
        error: String::new(),
    };
    (account, token)
}

/// One selection for every outer account entry. A signed-in account with
/// a picture wins over one without; ties keep the existing dandanplay identity.
fn profile(account: &Account, bangumi: &BangumiAccount) -> (String, ProfileAccount, Option<MediaSource>) {
    let dandan_picture = account.avatar.as_ref()
        .filter(|picture| account.signed_in && !picture.url.trim().is_empty());
    let bangumi_picture = bangumi.avatar.as_ref()
        .filter(|picture| bangumi.signed_in && !picture.url.trim().is_empty());
    let (name, provider, picture) = if dandan_picture.is_some() {
        (&account.name, ProfileAccount::Dandanplay, dandan_picture)
    } else if bangumi_picture.is_some() {
        (&bangumi.name, ProfileAccount::Bangumi, bangumi_picture)
    } else if account.signed_in {
        (&account.name, ProfileAccount::Dandanplay, None)
    } else if bangumi.signed_in {
        (&bangumi.name, ProfileAccount::Bangumi, None)
    } else {
        return (local_user(), ProfileAccount::Local, None);
    };
    let name = if name.trim().is_empty() {
        match provider {
            ProfileAccount::Dandanplay => "弹弹play",
            ProfileAccount::Bangumi => "Bangumi",
            ProfileAccount::Local => "本地用户",
        }.to_string()
    } else {
        name.clone()
    };
    (name, provider, picture.cloned())
}

impl ErikaEngine {
    pub(super) fn refresh_account_profile(&mut self) {
        let (name, provider, avatar) = profile(&self.state.account_status, &self.state.bangumi);
        self.state.user_name = name;
        self.state.user_account = provider;
        self.state.user_avatar = avatar;
    }

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
        erika_dandanplay::set_account(&account.user, token);
        self.state.account_status = account;
        self.refresh_account_profile();
    }

    /// Suspend account use without deleting the saved login. A fresh inbox
    /// also discards work started before this module was switched off.
    pub(super) fn set_account_enabled(&mut self, enabled: bool) {
        self.account_inbox = Arc::default();
        if enabled {
            self.restore_account();
            self.restore_bangumi();
        } else {
            self.suspend_bangumi();
            erika_dandanplay::set_token(None);
            self.state.account_status = Account::default();
        }
        self.refresh_account_profile();
        #[cfg(feature = "bern-module-anime")]
        erika_anime_protocol::anime::calls::account_changed(self.state.account_status.signed_in);
        #[cfg(feature = "bern-module-library")]
        erika_library_protocol::library::calls::account_changed(
            self.state.account_status.signed_in || erika_dandanplay::custom::server().is_some(),
        );
    }

    pub(super) fn open_account_prompt(&mut self, settings_window: bool) {
        if self.state.account_status.signed_in { return; }
        self.close_bangumi();
        self.state.account_status.prompt = true;
        self.state.account_status.settings_window = settings_window;
        self.state.account_status.error.clear();
    }

    pub(super) fn close_account_prompt(&mut self) {
        if !self.state.account_status.prompt { return; }
        self.account_inbox = Arc::default();
        self.state.account_status.prompt = false;
        self.state.account_status.busy = false;
        self.state.account_status.error.clear();
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
            avatar: None,
            busy: false,
            error: String::new(),
            ..self.state.account_status.clone()
        };
        self.refresh_account_profile();
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
        if !self.state.features.account || done.is_empty() {
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
                    erika_dandanplay::set_account(&account.user_name, Some(account.token));
                    self.state.account_status = Account {
                        signed_in: true,
                        prompt: false,
                        name: account.screen_name.clone(),
                        avatar: avatar_for_user(&account.user_name),
                        user: account.user_name,
                        busy: false,
                        error: String::new(),
                        ..self.state.account_status.clone()
                    };
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
                Done::Renewed(previous, renewed) => {
                    if !self.state.account_status.signed_in || erika_dandanplay::token().as_deref() != Some(previous.as_str()) {
                        continue;
                    }
                    if bern::secrets::write(TOKEN, &renewed.token).is_ok() {
                        self.db
                            .set_setting(RENEWED, Some(&erika_store::now().to_string()));
                        erika_dandanplay::set_account(&self.state.account_status.user, Some(renewed.token));
                    }
                    if let Some(name) = renewed
                        .screen_name
                        .filter(|name| *name != self.state.account_status.name)
                    {
                        self.db.set_setting(NAME, Some(&name));
                        self.state.account_status.name = name.clone();
                    }
                }
            }
        }
        self.refresh_account_profile();
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
                Ok(renewed) => vec![Done::Renewed(token, renewed)],
                // An old token goes on working until dandanplay refuses it.
                Err(error) => {
                    eprintln!("erika: renew the dandanplay token: {}", error.message);
                    Vec::new()
                }
            }
        });
    }
}
