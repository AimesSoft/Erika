//! Bangumi's first-party website login. A challenge and its submission use
//! one cookie jar. Only the server's persistent cookies are saved.
use regex::Regex;
use scraper::{Html, Selector};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

const SITE: &str = "https://bgm.tv";
// Keep request stages and error categories, never request bodies or cookies.
fn request_error(stage: &str, error: ureq::Error) -> String {
    let reason = match error {
        ureq::Error::Timeout(_) => "连接超时",
        ureq::Error::HostNotFound => "域名解析失败",
        ureq::Error::InvalidProxyUrl | ureq::Error::ConnectProxyFailed(_) => "代理连接失败",
        ureq::Error::Tls(_) | ureq::Error::Rustls(_) => "安全连接失败",
        ureq::Error::TooManyRedirects | ureq::Error::RedirectFailed => "页面跳转失败",
        ureq::Error::Protocol(_) => "服务器响应异常",
        ureq::Error::BodyExceedsLimit(_) => "服务器响应过大",
        ureq::Error::Io(ref error) if error.kind() == std::io::ErrorKind::TimedOut => "连接超时",
        _ => "连接中断",
    };
    format!("Bangumi {stage}失败：{reason}，请重试")
}

pub struct Session(ureq::Agent);

pub struct User {
    pub id: String,
    pub name: String,
    pub avatar: Option<String>,
    pub email: Option<String>,
}

pub struct Challenge {
    pub session: Session,
    pub image: PathBuf,
    formhash: String,
}

impl Drop for Challenge {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.image);
    }
}

fn selector(value: &str) -> Selector {
    Selector::parse(value).expect("constant Bangumi selector")
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .http_status_as_error(false)
        .https_only(true)
        // Follow GET redirects ourselves; never replay a password to a redirect.
        .max_redirects(0)
        .user_agent(format!(
            "AimesSoft/Erika/{} ({}) (https://github.com/AimesSoft/Erika)",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS
        ))
        .build()
        .into()
}

fn html(mut response: ureq::http::Response<ureq::Body>, stage: &str) -> Result<String, String> {
    if !response.status().is_success() {
        return Err(format!(
            "Bangumi 暂时无法处理请求（{}），请稍后重试",
            response.status().as_u16()
        ));
    }
    response
        .body_mut()
        .with_config()
        .limit(2 * 1024 * 1024)
        .read_to_string()
        .map_err(|error| request_error(stage, error))
}

fn redirect(location: &str) -> Result<String, String> {
    if location.starts_with('/') && !location.starts_with("//") {
        Ok(format!("{SITE}{location}"))
    } else if location.starts_with("https://bgm.tv/") {
        Ok(location.into())
    } else {
        Err("Bangumi 返回了无法识别的登录跳转，请重试".into())
    }
}

impl Session {
    pub fn restore(cookies: &str) -> Result<Self, String> {
        let session = Self(agent());
        session
            .0
            .cookie_jar_lock()
            .load_json(cookies.as_bytes())
            .map_err(|_| "保存的 Bangumi 登录信息无法读取，请重新登录".to_string())?;
        Ok(session)
    }

    pub fn save(&self) -> Result<String, String> {
        let mut bytes = Vec::new();
        self.0
            .cookie_jar_lock()
            .save_json(&mut bytes)
            .map_err(|_| "无法保存 Bangumi 登录信息".to_string())?;
        String::from_utf8(bytes).map_err(|_| "无法保存 Bangumi 登录信息".into())
    }

    fn get(&self, path: &str) -> Result<String, String> {
        let stage = if path == "/login" {
            "加载登录页"
        } else {
            "读取登录状态"
        };
        let mut url = format!("{SITE}{path}");
        for _ in 0..5 {
            // A read can safely reconnect once if a proxy/server closed an
            // idle socket. Password submissions are never automatically replayed.
            let response = self
                .0
                .get(&url)
                .call()
                .or_else(|_| {
                    self.0
                        .get(&url)
                        .config()
                        .max_idle_age(Duration::ZERO)
                        .build()
                        .call()
                })
                .map_err(|error| request_error(stage, error))?;
            if response.status().is_redirection() {
                url = redirect(
                    response
                        .headers()
                        .get("location")
                        .and_then(|s| s.to_str().ok())
                        .unwrap_or(""),
                )?;
            } else {
                return html(response, stage);
            }
        }
        Err("Bangumi 登录跳转次数过多，请稍后重试".into())
    }

    /// A normal page with UID zero is an expired session. A challenge,
    /// server error or changed markup is not evidence of expiry.
    pub fn check(&self, needs_email: bool) -> Result<Option<User>, String> {
        let Some(mut user) = identity(&self.get("/")?)?.map(with_profile) else {
            return Ok(None);
        };
        if needs_email {
            // Older installs saved the UID but not the login email. Read it
            // from this account's private settings without logging out.
            if let Ok(page) = self.get("/settings") {
                user.email = settings_email(&page, &user.id);
                if user.email.is_none() {
                    let doc = Html::parse_document(&page);
                    let path = doc.select(&selector("a[href]")).find_map(|link| {
                        let href = link.value().attr("href")?;
                        let path = href.strip_prefix(SITE).unwrap_or(href);
                        let label = link.text().collect::<String>().to_ascii_lowercase();
                        (path.starts_with("/settings/")
                            && (path.to_ascii_lowercase().contains("email")
                                || label.contains("邮箱")
                                || label.contains("邮件")
                                || label.contains("email")))
                        .then_some(path)
                    });
                    if let Some(path) = path {
                        if let Ok(page) = self.get(path) {
                            user.email = settings_email(&page, &user.id);
                        }
                    }
                }
            }
        }
        Ok(Some(user))
    }
}

pub fn prepare(directory: PathBuf) -> Result<Challenge, String> {
    let session = Session(agent());
    let formhash = login_form(&session.get("/login")?)?;
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nonce = format!(
        "{}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let mut response = session
        .0
        .get(format!("{SITE}/signup/captcha?{nonce}"))
        .header("Referer", "https://bgm.tv/login")
        .call()
        .map_err(|error| request_error("加载验证码", error))?;
    if !response.status().is_success()
        || !response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.starts_with("image/"))
    {
        return Err("Bangumi 验证码加载失败，请刷新验证码".into());
    }
    let bytes = response
        .body_mut()
        .with_config()
        .limit(512 * 1024)
        .read_to_vec()
        .map_err(|error| request_error("读取验证码", error))?;
    if bytes.is_empty() {
        return Err("Bangumi 返回了空验证码，请刷新".into());
    }
    std::fs::create_dir_all(&directory).map_err(|_| "无法创建验证码缓存".to_string())?;
    let image = directory.join(format!("captcha-{nonce}.png"));
    std::fs::write(&image, bytes).map_err(|_| "无法保存验证码图片".to_string())?;
    Ok(Challenge {
        session,
        image,
        formhash,
    })
}

impl Challenge {
    pub fn submit(&self, email: &str, password: &str, captcha: &str) -> Result<User, String> {
        let response = self
            .session
            .0
            .post("https://bgm.tv/FollowTheRabbit")
            // The user may spend minutes filling the form. Keep the cookie
            // jar but establish a fresh connection for this one-time POST.
            .config()
            .max_idle_age(Duration::ZERO)
            .build()
            .header("Referer", "https://bgm.tv/login")
            .header("Origin", SITE)
            .send_form([
                ("formhash", self.formhash.as_str()),
                ("referer", ""),
                ("dreferer", ""),
                ("email", email.trim()),
                ("password", password),
                ("captcha_challenge_field", captcha.trim()),
                ("loginsubmit", "登录"),
                // cookietime=0 means "do not save login" and must be omitted.
            ])
            .map_err(|error| request_error("提交登录", error))?;
        let page = if response.status().is_redirection() {
            let url = redirect(
                response
                    .headers()
                    .get("location")
                    .and_then(|s| s.to_str().ok())
                    .unwrap_or(""),
            )?;
            self.session.get(url.strip_prefix(SITE).unwrap())?
        } else {
            html(response, "读取登录结果")?
        };
        if let Ok(Some(user)) = identity(&page) {
            let mut user = with_profile(user);
            user.email = Some(email.trim().to_string());
            return Ok(user);
        }
        // An explicit rejection is the actual login result. A later status
        // request must not replace it with an unrelated connection error.
        if let Some(error) = login_rejection(&page) {
            return Err(error.into());
        }
        // Some successful submissions use an HTML redirect. Verify against
        // an authenticated page rather than trusting a success string.
        if let Some(mut user) = self.session.check(false)? {
            user.email = Some(email.trim().to_string());
            return Ok(user);
        }
        Err("登录未成功，请确认邮箱、密码，并刷新验证码后重试".into())
    }
}

/// The public API accepts numeric IDs at /user/{id}; /v0/users/{name}
/// requires the custom username. Use a separate agent so website cookies
/// never accompany this public profile request.
fn with_profile(mut user: User) -> User {
    let response = agent()
        .get(format!("https://api.bgm.tv/user/{}", user.id))
        .call();
    if let Ok(mut response) = response {
        if response.status().is_success() {
            if let Ok(body) = response
                .body_mut()
                .with_config()
                .limit(256 * 1024)
                .read_to_string()
            {
                apply_profile(&mut user, &body);
            }
        }
    }
    user
}

fn apply_profile(user: &mut User, body: &str) {
    let Ok(profile) = serde_json::from_str::<serde_json::Value>(body) else {
        return;
    };
    if profile["id"].as_u64().map(|id| id.to_string()).as_deref() != Some(user.id.as_str()) {
        return;
    }
    if let Some(name) = profile["nickname"]
        .as_str()
        .filter(|v| !v.trim().is_empty())
    {
        user.name = name.to_string();
    }
    if let Some(url) = profile["avatar"]["large"].as_str().and_then(image_url) {
        user.avatar = Some(url);
    }
}

fn login_form(page: &str) -> Result<String, String> {
    let doc = Html::parse_document(page);
    let form = doc
        .select(&selector("form#loginForm"))
        .next()
        .ok_or("Bangumi 登录页面暂时不可用，请稍后重试")?;
    if form.value().attr("action") != Some("/FollowTheRabbit") {
        return Err("Bangumi 登录方式已变化，请更新应用".into());
    }
    form.select(&selector("input[name=formhash]"))
        .next()
        .and_then(|v| v.value().attr("value"))
        .filter(|v| !v.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "无法取得 Bangumi 登录验证信息，请重试".into())
}

fn identity(page: &str) -> Result<Option<User>, String> {
    let doc = Html::parse_document(page);
    let uid = Regex::new(r"\bCHOBITS_UID\s*=\s*(\d+)\s*[,;]").unwrap();
    for script in doc.select(&selector("script:not([src])")) {
        let script = script.inner_html();
        let Some(captures) = uid.captures(&script) else {
            continue;
        };
        let id = captures[1].to_string();
        if id == "0" {
            return Ok(None);
        }
        let mut user = User {
            name: format!("Bangumi 用户 {id}"),
            id: id.clone(),
            avatar: None,
            email: None,
        };
        // Only the header's own account link, never a timeline user's name.
        for link in doc.select(&selector(
            "#headerNeue2 .idBadger a[href*='/user/'], #header .idBadger a[href*='/user/']",
        )) {
            let name = link.text().collect::<String>().trim().to_string();
            if !name.is_empty() {
                user.name = name;
            }
            if let Some(img) = link.select(&selector("img")).next() {
                if user.name.starts_with("Bangumi 用户 ") {
                    if let Some(name) = img.value().attr("alt").filter(|v| !v.is_empty()) {
                        user.name = name.into();
                    }
                }
                user.avatar = img.value().attr("src").and_then(image_url);
            }
            if user.avatar.is_none() {
                for span in link.select(&selector("[style]")) {
                    if let Some(style) = span.value().attr("style") {
                        let image = Regex::new(r#"url\(['"]?([^'")]+)"#).unwrap();
                        user.avatar = image.captures(style).and_then(|c| image_url(&c[1]));
                        if user.avatar.is_some() {
                            break;
                        }
                    }
                }
            }
        }
        return Ok(Some(user));
    }
    Err("暂时无法确认 Bangumi 登录状态，请稍后重试".into())
}

fn settings_email(page: &str, expected_user: &str) -> Option<String> {
    if identity(page).ok().flatten()?.id != expected_user {
        return None;
    }
    let doc = Html::parse_document(page);
    let email = Regex::new(
        r"[A-Za-z0-9.!#$%&'*+/=?^_`{|}~-]+@[A-Za-z0-9](?:[A-Za-z0-9.-]*[A-Za-z0-9])?\.[A-Za-z]{2,}",
    )
    .unwrap();
    for input in doc.select(&selector(
        "input[name=email], input[id=email], input[name=current_email]",
    )) {
        if let Some(value) = input.value().attr("value").map(str::trim) {
            if email
                .find(value)
                .is_some_and(|found| found.as_str() == value)
            {
                return Some(value.to_string());
            }
        }
    }
    // The email settings page can show the current address as text, with
    // an empty input reserved for the replacement address.
    // The current /settings/email page puts this paragraph outside #columnA.
    // Match its explicit label, not the old layout container or an unrelated
    // email address elsewhere on the authenticated page.
    let current_email = Regex::new(
        r"(?i)(?:当前|目前)\s*(?:登录\s*)?(?:邮箱|邮件地址|电子邮箱|电子邮件|email)|current\s+email",
    ).unwrap();
    for block in doc.select(&selector("p, td, li")) {
        let text = block.text().collect::<String>();
        if current_email.is_match(&text) {
            if let Some(value) = email.find(&text) {
                return Some(value.as_str().to_string());
            }
        }
    }
    None
}

fn image_url(value: &str) -> Option<String> {
    let url = if value.starts_with("//") {
        format!("https:{value}")
    } else if let Some(rest) = value.strip_prefix("http://") {
        format!("https://{rest}")
    } else {
        value.into()
    };
    (url.starts_with("https://lain.bgm.tv/") || url.starts_with("https://bgm.tv/")).then_some(url)
}

fn login_rejection(page: &str) -> Option<&'static str> {
    // Do not echo arbitrary server HTML (it can include account details).
    let text = Html::parse_document(page)
        .root_element()
        .text()
        .collect::<String>();
    if text.contains("验证码")
        && (text.contains("错误") || text.contains("不正确") || text.contains("不匹配"))
    {
        Some("验证码不正确或已过期，请刷新后重试")
    } else if text.contains("密码")
        && (text.contains("错误") || text.contains("不正确") || text.contains("不匹配"))
    {
        Some("邮箱或密码不正确，请检查后刷新验证码重试")
    } else if text.contains("频繁") || text.contains("次数") || text.contains("分钟") {
        Some("登录尝试过于频繁，请稍后再试")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn login_rejections_are_not_replaced_by_a_status_request_error() {
        assert_eq!(
            login_rejection("<p>验证码不正确</p>"),
            Some("验证码不正确或已过期，请刷新后重试")
        );
        assert_eq!(
            login_rejection("<p>邮箱或密码错误</p>"),
            Some("邮箱或密码不正确，请检查后刷新验证码重试")
        );
        assert_eq!(
            login_rejection("<p>登录尝试过于频繁</p>"),
            Some("登录尝试过于频繁，请稍后再试")
        );
        assert_eq!(login_rejection("<p>正在跳转</p>"), None);
    }

    #[test]
    fn request_errors_identify_the_stage_without_exposing_proxy_details() {
        assert_eq!(
            request_error("提交登录", ureq::Error::HostNotFound),
            "Bangumi 提交登录失败：域名解析失败，请重试"
        );
        assert_eq!(
            request_error(
                "读取登录状态",
                ureq::Error::ConnectProxyFailed("private proxy details".into())
            ),
            "Bangumi 读取登录状态失败：代理连接失败，请重试"
        );
    }

    #[test]
    fn failed_submission_keeps_the_challenge_until_cancelled() {
        use super::super::Done;
        let mut engine = crate::ErikaEngine::default();
        engine.state.features.account = true;
        engine.state.bangumi.prompt = true;
        engine.state.bangumi.busy = true;
        let image =
            std::env::temp_dir().join(format!("erika-captcha-test-{}.png", std::process::id()));
        std::fs::write(&image, b"test image").unwrap();
        engine.state.bangumi.captcha = Some(bern::prelude::MediaSource::file(&image));
        let challenge = Challenge {
            session: Session(agent()),
            image: image.clone(),
            formhash: "test".into(),
        };
        engine
            .bangumi
            .inbox
            .lock()
            .unwrap()
            .push(Done::Submitted(challenge, Err("temporary failure".into())));
        engine.take_bangumi_results();
        assert!(!engine.state.bangumi.busy);
        assert!(engine.state.bangumi.prompt);
        assert!(engine.state.bangumi.captcha.is_some());
        assert!(engine.bangumi.challenge.is_some());
        assert!(image.exists());
        engine.close_bangumi();
        assert!(!image.exists());
        assert!(engine.state.bangumi.captcha.is_none());
    }

    #[test]
    fn email_migration_requires_the_authenticated_account() {
        let page = "<script>var CHOBITS_UID = 123, x = 1;</script><form><input name='email' value=' member@example.com '></form>";
        assert_eq!(
            settings_email(page, "123").as_deref(),
            Some("member@example.com")
        );
        assert_eq!(settings_email(page, "456"), None);
        assert_eq!(
            settings_email("<input name='email' value='member@example.com'>", "123"),
            None
        );
        assert_eq!(
            settings_email(
                "<script>var CHOBITS_UID = 0, x = 1;</script><input name='email' value='member@example.com'>",
                "123"
            ),
            None
        );
    }

    #[test]
    fn email_migration_uses_current_email_not_an_empty_replacement() {
        let current = "<script>var CHOBITS_UID = 123, x = 1;</script><div><p>当前登录邮箱：member@example.com</p><input name='new_email' value=''><p>当前技术支持 support@example.com</p></div>";
        assert_eq!(
            settings_email(current, "123").as_deref(),
            Some("member@example.com")
        );
        assert_eq!(
            settings_email(
                "<script>var CHOBITS_UID = 123, x = 1;</script><p>当前技术支持 support@example.com</p>",
                "123"
            ),
            None
        );
        let page = "<script>var CHOBITS_UID = 123, x = 1;</script><div id='columnA'><p>当前 Email 地址：<strong>member@example.com</strong></p><input name='email' value=''></div>";
        assert_eq!(
            settings_email(page, "123").as_deref(),
            Some("member@example.com")
        );
        assert_eq!(
            settings_email(
                "<script>var CHOBITS_UID = 123, x = 1;</script><div id='columnA'><p>联系 support@example.com</p><input name='email' value=''></div>",
                "123"
            ),
            None
        );
    }

    #[test]
    fn profile_is_bound_to_authenticated_id_and_avatar_uses_https() {
        let mut user = User {
            id: "123".into(),
            name: "fallback".into(),
            avatar: None,
            email: None,
        };
        apply_profile(
            &mut user,
            r#"{"id":123,"nickname":"Nickname","avatar":{"large":"http://lain.bgm.tv/pic/user/l/123.jpg"}}"#,
        );
        assert_eq!(user.name, "Nickname");
        assert_eq!(
            user.avatar.as_deref(),
            Some("https://lain.bgm.tv/pic/user/l/123.jpg")
        );
        apply_profile(&mut user, r#"{"id":456,"nickname":"Someone else"}"#);
        assert_eq!(user.name, "Nickname");
        apply_profile(&mut user, "unavailable");
        assert!(user.avatar.is_some());
        assert!(image_url("http://lain.bgm.tv.evil.test/a.jpg").is_none());
    }

    #[test]
    fn form_hash_is_parsed_and_unknown_forms_fail_closed() {
        assert_eq!(login_form("<form action='/FollowTheRabbit' id='loginForm'><input value='fresh&amp;hash' name='formhash'></form>").unwrap(), "fresh&hash");
        assert!(login_form("<form id='loginForm' action='https://other.test'><input name='formhash' value='x'></form>").is_err());
    }
    #[test]
    fn only_explicit_guest_identity_means_expired() {
        assert!(
            identity("<script>var CHOBITS_UID = 0, CHOBITS_USERNAME = '';</script>")
                .unwrap()
                .is_none()
        );
        assert!(identity("<h1>Checking your browser</h1>").is_err());
        assert!(identity("<div>CHOBITS_UID=0,</div>").is_err());
        let user = identity("<script>var CHOBITS_UID = 123, x = 1;</script><div id='headerNeue2'><div class='idBadger'><a href='/user/123'><img src='//lain.bgm.tv/pic/user/l/a.jpg' alt='测试用户'></a></div></div>").unwrap().unwrap();
        assert_eq!(user.id, "123");
        assert_eq!(user.name, "测试用户");
        assert_eq!(
            user.avatar.as_deref(),
            Some("https://lain.bgm.tv/pic/user/l/a.jpg")
        );
    }
    #[test]
    fn redirects_never_leave_bangumi() {
        for url in [
            "//other.test/",
            "https://bgm.tv.evil.test/",
            "http://bgm.tv/",
            "https://other.test/",
        ] {
            assert!(redirect(url).is_err());
        }
        assert_eq!(redirect("/login").unwrap(), "https://bgm.tv/login");
    }
    #[test]
    fn cookie_roundtrip_keeps_persistent_cookies_and_discards_expired_ones() {
        let session = Session(agent());
        let uri = "https://bgm.tv/".parse().unwrap();
        {
            let mut jar = session.0.cookie_jar_lock();
            for cookie in [
                "auth=example; Path=/; Secure; HttpOnly; Max-Age=3600",
                "gone=x; Path=/; Max-Age=0",
                "temporary=x; Path=/",
            ] {
                let _ = jar.insert(ureq::Cookie::parse(cookie, &uri).unwrap(), &uri);
            }
        }
        let restored = Session::restore(&session.save().unwrap()).unwrap();
        let jar = restored.0.cookie_jar_lock();
        assert_eq!(jar.get("bgm.tv", "/", "auth").unwrap().value(), "example");
        assert!(jar.get("bgm.tv", "/", "gone").is_none());
        assert!(jar.get("bgm.tv", "/", "temporary").is_none());
    }
}
