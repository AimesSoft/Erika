//! Independent Bangumi website session; never share it with dandanplay.
use super::*;
use std::sync::Mutex;

#[cfg(not(target_arch = "wasm32"))]
mod http;
#[cfg(target_arch = "wasm32")]
mod http {
    const UNAVAILABLE: &str = "纯网页版无法直接访问 Bangumi 登录，请使用桌面版、手机版或 WebUI";
    pub struct Session;
    pub struct User {
        pub id: String,
        pub name: String,
        pub avatar: Option<String>,
        pub email: Option<String>,
    }
    pub struct Challenge {
        pub session: Session,
        pub image: std::path::PathBuf,
    }
    impl Session {
        pub fn restore(_: &str) -> Result<Self, String> {
            Err(UNAVAILABLE.into())
        }
        pub fn save(&self) -> Result<String, String> {
            Err(UNAVAILABLE.into())
        }
        pub fn check(&self, _: bool) -> Result<Option<User>, String> {
            Err(UNAVAILABLE.into())
        }
    }
    impl Challenge {
        pub fn submit(&self, _: &str, _: &str, _: &str) -> Result<User, String> {
            Err(UNAVAILABLE.into())
        }
    }
    pub fn prepare(_: std::path::PathBuf) -> Result<Challenge, String> {
        Err(UNAVAILABLE.into())
    }
}

const SECRET: &str = "bangumi-cookies";
const USER: &str = "bangumi.user";
const EMAIL: &str = "bangumi.email";
const NAME: &str = "bangumi.name";
const AVATAR: &str = "bangumi.avatar";
const CHECK_AFTER: i64 = 6 * 3600;
const RETRY_AFTER: i64 = 5 * 60;

enum Done {
    Prepared(Result<http::Challenge, String>),
    Submitted(http::Challenge, Result<http::User, String>),
    Checked(http::Session, Result<Option<http::User>, String>),
}

#[derive(Default)]
pub(super) struct Runtime {
    inbox: Arc<Mutex<Vec<Done>>>,
    challenge: Option<http::Challenge>,
    session: Option<http::Session>,
    next_check: i64,
}

impl ErikaEngine {
    fn notify_bangumi_account(&mut self) {
        self.refresh_account_profile();
        if !self.state.bangumi.signed_in {
            erika_dandanplay::bangumi::site::set_session(None);
        }
        erika_dandanplay::reviews::set_bangumi_signed_in(self.state.bangumi.signed_in);
        #[cfg(feature = "bern-module-anime")]
        erika_anime_protocol::anime::calls::bangumi_account_changed(self.state.bangumi.signed_in);
    }

    pub(super) fn restore_bangumi(&mut self) {
        self.bangumi = Runtime::default();
        self.state.bangumi = BangumiAccount::default();
        match bern::secrets::read(SECRET) {
            Ok(Some(cookies)) => match http::Session::restore(&cookies) {
                Ok(session) => {
                    // The anime module syncs the collection with the same session.
                    erika_dandanplay::bangumi::site::set_account(&self.db.setting(USER).unwrap_or_default(),Some(cookies.clone()));
                    self.bangumi.session = Some(session);
                    self.state.bangumi.signed_in = true;
                    self.state.bangumi.name =
                        self.db.setting(NAME).unwrap_or_else(|| "Bangumi".into());
                    self.state.bangumi.user = self.db.setting(USER).unwrap_or_default();
                    self.state.bangumi.email = self.db.setting(EMAIL).unwrap_or_default();
                    self.state.bangumi.avatar = self.db.setting(AVATAR).map(MediaSource::new);
                }
                Err(error) => {
                    self.state.bangumi.error = error;
                    self.state.bangumi.expired = true;
                }
            },
            Ok(None) => self.state.bangumi.expired = self.db.setting(USER).is_some(),
            // An unreadable store leaves the account signed out, without a
            // word: signing in again saves it anew.
            Err(_) => {}
        }
        self.notify_bangumi_account();
    }

    pub(super) fn suspend_bangumi(&mut self) {
        self.bangumi = Runtime::default();
        self.state.bangumi = BangumiAccount::default();
        self.notify_bangumi_account();
    }

    pub(super) fn open_bangumi(&mut self, settings_window: bool) {
        if self.state.bangumi.signed_in {
            return;
        }
        self.state.bangumi.prompt = true;
        self.state.bangumi.settings_window = settings_window;
        self.state.bangumi.expired = false;
        self.prepare_bangumi();
    }

    pub(super) fn prepare_bangumi(&mut self) {
        if !self.state.bangumi.prompt || self.state.bangumi.busy {
            return;
        }
        self.bangumi.inbox = Arc::default();
        self.bangumi.challenge = None;
        self.state.bangumi.captcha = None;
        self.state.bangumi.error.clear();
        self.state.bangumi.busy = true;
        let directory = bern::storage::cache_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("bangumi");
        in_background(&self.bangumi.inbox, move || {
            vec![Done::Prepared(http::prepare(directory))]
        });
    }

    pub(super) fn close_bangumi(&mut self) {
        if !self.state.bangumi.prompt {
            return;
        }
        // Old work can finish, but cannot change a cancelled/newer login.
        self.bangumi.inbox = Arc::default();
        self.bangumi.challenge = None;
        self.state.bangumi.prompt = false;
        self.state.bangumi.busy = false;
        self.state.bangumi.captcha = None;
        self.state.bangumi.error.clear();
    }

    pub(super) fn submit_bangumi(&mut self, email: String, password: String, captcha: String) {
        if self.state.bangumi.busy || !self.state.bangumi.prompt {
            return;
        }
        if email.trim().is_empty() || password.is_empty() || captcha.trim().is_empty() {
            self.state.bangumi.error = "请填写邮箱、密码和图片验证码".into();
            return;
        }
        let Some(challenge) = self.bangumi.challenge.take() else {
            self.state.bangumi.error = "请先刷新验证码".into();
            return;
        };
        self.state.bangumi.busy = true;
        self.state.bangumi.error.clear();
        in_background(&self.bangumi.inbox, move || {
            let result = challenge.submit(&email, &password, &captcha);
            vec![Done::Submitted(challenge, result)]
        });
    }

    pub(super) fn forget_bangumi(&mut self) -> Result<(), CommandError> {
        // Do not claim logout succeeded if the persisted credential remains.
        bern::secrets::delete(SECRET)?;
        self.suspend_bangumi();
        for key in [USER, EMAIL, NAME, AVATAR] {
            self.db.set_setting(key, None);
        }
        Ok(())
    }

    pub(super) fn check_bangumi(&mut self) {
        if !self.state.features.account
            || !self.state.bangumi.signed_in
            || self.state.bangumi.busy
            || erika_store::now() < self.bangumi.next_check
        {
            return;
        }
        let Some(session) = self.bangumi.session.take() else {
            return;
        };
        self.state.bangumi.busy = true;
        let needs_email = self.state.bangumi.email.is_empty();
        in_background(&self.bangumi.inbox, move || {
            let result = session.check(needs_email);
            vec![Done::Checked(session, result)]
        });
    }

    fn keep_bangumi(&mut self, session: &http::Session, user: http::User) {
        let was_signed_in = self.state.bangumi.signed_in;
        let saved = session.save().and_then(|cookies| {
            erika_dandanplay::bangumi::site::set_account(&user.id,Some(cookies.clone()));
            bern::secrets::write(SECRET, &cookies)
                .map_err(|_| "已登录，但无法安全保存登录信息；关闭应用后需要重新登录".to_string())
        });
        self.state.bangumi.signed_in = true;
        let same_user = self.state.bangumi.user == user.id;
        if let Some(email) = user.email {
            self.state.bangumi.email = email;
        } else if !same_user {
            self.state.bangumi.email.clear();
        }
        if !same_user
            || self.state.bangumi.name.is_empty()
            || !user.name.starts_with("Bangumi 用户 ")
        {
            self.state.bangumi.name = user.name;
        }
        self.state.bangumi.user = user.id;
        if user.avatar.is_some() || !same_user {
            self.state.bangumi.avatar = user.avatar.map(MediaSource::new);
        }
        self.state.bangumi.error = saved.err().unwrap_or_default();
        self.state.bangumi.expired = false;
        self.db.set_setting(USER, Some(&self.state.bangumi.user));
        self.db.set_setting(EMAIL, Some(&self.state.bangumi.email));
        self.db.set_setting(NAME, Some(&self.state.bangumi.name));
        self.db.set_setting(
            AVATAR,
            self.state.bangumi.avatar.as_ref().map(|s| s.url.as_str()),
        );
        self.bangumi.next_check = erika_store::now() + CHECK_AFTER;
        self.refresh_account_profile();
        if !was_signed_in || !same_user { self.notify_bangumi_account(); }
    }

    pub(super) fn take_bangumi_results(&mut self) {
        let done =
            std::mem::take(&mut *self.bangumi.inbox.lock().unwrap_or_else(|e| e.into_inner()));
        if !self.state.features.account {
            return;
        }
        for done in done {
            self.state.bangumi.busy = false;
            match done {
                Done::Prepared(Ok(challenge)) => {
                    self.state.bangumi.captcha = Some(MediaSource::file(&challenge.image));
                    self.bangumi.challenge = Some(challenge);
                }
                Done::Prepared(Err(error)) => self.state.bangumi.error = error,
                Done::Submitted(challenge, Ok(user)) => {
                    self.keep_bangumi(&challenge.session, user);
                    // A fresh session loads only persistent cookies, as a restart would.
                    self.bangumi.session = challenge
                        .session
                        .save()
                        .ok()
                        .and_then(|s| http::Session::restore(&s).ok());
                    self.state.bangumi.prompt = false;
                    self.state.bangumi.captcha = None;
                }
                Done::Submitted(challenge, Err(error)) => {
                    // Keep the image and its matching cookie jar available for
                    // retry/refresh; dropping it made every failure a dead end.
                    self.bangumi.challenge = Some(challenge);
                    self.state.bangumi.error = error;
                }
                Done::Checked(session, Ok(Some(user))) => {
                    self.keep_bangumi(&session, user);
                    self.bangumi.session = Some(session);
                }
                Done::Checked(_, Ok(None)) => {
                    // Keep the account marker so dismissal is per launch, not forever.
                    let _ = bern::secrets::delete(SECRET);
                    self.state.bangumi.signed_in = false;
                    self.state.bangumi.expired = true;
                    self.state.bangumi.error = "Bangumi 登录已过期，请重新登录".into();
                    self.notify_bangumi_account();
                }
                Done::Checked(session, Err(error)) => {
                    self.bangumi.session = Some(session);
                    self.bangumi.next_check = erika_store::now() + RETRY_AFTER;
                    self.state.bangumi.error = error;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_and_disable_discard_late_results() {
        let mut e = ErikaEngine::default();
        e.state.features.account = true;
        e.state.bangumi.prompt = true;
        let old = e.bangumi.inbox.clone();
        e.close_bangumi();
        old.lock().unwrap().push(Done::Prepared(Err("late".into())));
        e.take_bangumi_results();
        assert!(!e.state.bangumi.prompt);
        assert!(e.state.bangumi.error.is_empty());
        let old = e.bangumi.inbox.clone();
        e.suspend_bangumi();
        assert!(!Arc::ptr_eq(&old, &e.bangumi.inbox));
    }
    #[test]
    fn transient_errors_keep_login_but_guest_response_expires_it() {
        let mut e = ErikaEngine::default();
        e.state.features.account = true;
        e.state.bangumi.signed_in = true;
        e.state.bangumi.name = "Bangumi Name".into();
        e.state.bangumi.avatar = Some(MediaSource::new("bangumi.png"));
        e.refresh_account_profile();
        let session = http::Session::restore("[]").unwrap();
        e.bangumi
            .inbox
            .lock()
            .unwrap()
            .push(Done::Checked(session, Err("offline".into())));
        e.take_bangumi_results();
        assert!(e.state.bangumi.signed_in);
        assert!(!e.state.bangumi.expired);
        assert_eq!(e.state.user_account, ProfileAccount::Bangumi);
        assert!(e.state.user_avatar.is_some());
        assert!(e.bangumi.next_check > erika_store::now());
        let session = e.bangumi.session.take().unwrap();
        e.bangumi
            .inbox
            .lock()
            .unwrap()
            .push(Done::Checked(session, Ok(None)));
        e.take_bangumi_results();
        assert!(!e.state.bangumi.signed_in);
        assert!(e.state.bangumi.expired);
        assert_eq!(e.state.user_account, ProfileAccount::Local);
        assert!(e.state.user_avatar.is_none());
    }
}
