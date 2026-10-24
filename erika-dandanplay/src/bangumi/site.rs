//! The signed-in user's collection on the Bangumi website, through the
//! session the account settings keep ([`set_session`]). It works as the
//! website's own collect box and episode grid do: the subject page says
//! what is collected and which episodes are watched, and their forms are
//! posted with the page's form hash. The session goes to bgm.tv only,
//! never to the public API.

use crate::{Error, Result};
use std::sync::RwLock;

const SITE: &str = "https://bgm.tv";

/// The session's cookies, as the account settings saved them.
static SESSION: RwLock<Option<String>> = RwLock::new(None);

/// Use the session saved as `cookies` from now on, or none after signing
/// out or expiring.
pub fn set_session(cookies: Option<String>) {
    *SESSION
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = cookies;
}

fn session() -> Option<String> {
    SESSION
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

pub fn signed_in() -> bool {
    session().is_some()
}

/// A collection type of the website, by the number its form posts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interest {
    /// 想看
    Wish = 1,
    /// 看过
    Collect = 2,
    /// 在看
    Do = 3,
    /// 搁置
    OnHold = 4,
    /// 抛弃
    Dropped = 5,
}

impl Interest {
    pub fn id(self) -> i64 {
        self as i64
    }

    fn from_id(id: i64) -> Option<Self> {
        [
            Self::Wish,
            Self::Collect,
            Self::Do,
            Self::OnHold,
            Self::Dropped,
        ]
        .into_iter()
        .find(|interest| interest.id() == id)
    }

    /// The id the collect box gives the radio button of each type.
    fn from_key(key: &str) -> Option<Self> {
        match key {
            "wish" => Some(Self::Wish),
            "collect" => Some(Self::Collect),
            "do" => Some(Self::Do),
            "on_hold" => Some(Self::OnHold),
            "dropped" => Some(Self::Dropped),
            _ => None,
        }
    }
}

/// A main episode on the subject's grid, by its id on the website and its
/// number, and whether the user marked it watched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Episode {
    pub id: i64,
    pub number: i64,
    pub watched: bool,
}

/// What the user collected a subject as, the words of the collect box,
/// and the episodes. `interest` is `None` while it is not collected.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Collection {
    pub interest: Option<Interest>,
    /// 1 to 10, 0 for none.
    pub rating: i64,
    pub comment: String,
    pub tags: String,
    pub private: bool,
    pub episodes: Vec<Episode>,
    /// The `gh` the website's forms carry.
    form_hash: String,
}

fn expired() -> Error {
    Error {
        message: "Bangumi 登录已过期，请重新登录".into(),
        sign_in: true,
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod page {
    use super::*;
    use regex::Regex;
    use scraper::{ElementRef, Html, Selector};

    fn selector(value: &str) -> Selector {
        Selector::parse(value).expect("constant Bangumi selector")
    }

    /// The account the page was made for: `None` for a guest, which an
    /// expired session is.
    fn account(page: &str) -> Option<Option<String>> {
        let uid = Regex::new(r"\bCHOBITS_UID\s*=\s*(\d+)\s*[,;]").unwrap();
        let id = uid.captures(page)?[1].to_string();
        Some((id != "0").then_some(id))
    }

    pub(super) fn form_hash(page: &str) -> Option<String> {
        Regex::new(r#"/logout/([A-Za-z0-9]+)"#)
            .unwrap()
            .captures(page)
            .or_else(|| {
                Regex::new(r#"interest/update\?gh=([A-Za-z0-9]+)"#)
                    .unwrap()
                    .captures(page)
            })
            .map(|captures| captures[1].to_string())
    }

    fn checked<'a>(form: ElementRef<'a>, name: &str) -> Option<ElementRef<'a>> {
        form.select(&selector(&format!("input[name={name}][checked]")))
            .next()
    }

    /// The collect box, wherever the page holds it; `None` when it has
    /// none.
    pub(super) fn collect_box(page: &str) -> Option<Collection> {
        let document = Html::parse_document(page);
        let form = document.select(&selector("#collectBoxForm")).next()?;
        let interest = checked(form, "interest")
            .and_then(|input| input.value().attr("value")?.trim().parse().ok())
            .and_then(Interest::from_id)
            .or_else(|| {
                form.select(&selector(".collectType input[checked]"))
                    .next()
                    .and_then(|input| Interest::from_key(input.value().id()?))
            });
        Some(Collection {
            interest,
            rating: checked(form, "rating")
                .and_then(|input| input.value().attr("value")?.trim().parse().ok())
                .unwrap_or(0)
                .clamp(0, 10),
            comment: form
                .select(&selector("textarea[name=comment]"))
                .next()
                .map(|area| area.text().collect::<String>().trim().to_string())
                .unwrap_or_default(),
            tags: form
                .select(&selector("input[name=tags]"))
                .next()
                .and_then(|input| input.value().attr("value"))
                .unwrap_or_default()
                .trim()
                .to_string(),
            private: checked(form, "privacy").is_some(),
            episodes: Vec::new(),
            form_hash: String::new(),
        })
    }

    /// The main episodes of the grid: those before its first heading
    /// other than 「本篇」 (`SP` and the like).
    pub(super) fn episodes(page: &str) -> Vec<Episode> {
        let document = Html::parse_document(page);
        let Some(grid) = document.select(&selector("ul.prg_list")).next() else {
            return Vec::new();
        };
        let digits = Regex::new(r"\d+").unwrap();
        let mut episodes = Vec::new();
        for item in grid.select(&selector("li")) {
            if item
                .value()
                .has_class("subtitle", scraper::CaseSensitivity::CaseSensitive)
            {
                if item.text().collect::<String>().trim() == "本篇" {
                    continue;
                }
                break;
            }
            let Some(link) = item.select(&selector("a[id^=prg_]")).next() else {
                continue;
            };
            let id = link
                .value()
                .id()
                .and_then(|id| id.strip_prefix("prg_"))
                .and_then(|id| id.parse().ok());
            let text = link.text().collect::<String>();
            let number = digits
                .find(&text)
                .and_then(|number| number.as_str().parse().ok());
            if let (Some(id), Some(number)) = (id, number) {
                episodes.push(Episode {
                    id,
                    number,
                    watched: link
                        .value()
                        .has_class("epBtnWatched", scraper::CaseSensitivity::CaseSensitive),
                });
            }
        }
        episodes
    }

    /// The collection on the subject's page; the collect box may come from
    /// `update`, the page the website opens it in.
    pub(super) fn parse(page: &str, update: Option<&str>) -> Result<Collection> {
        match account(page) {
            Some(Some(_)) => {}
            Some(None) => return Err(expired()),
            None => return Err(Error::new("暂时读不到 Bangumi 收藏，请稍后重试")),
        }
        let mut collection = collect_box(page)
            .or_else(|| update.and_then(collect_box))
            .ok_or_else(|| Error::new("Bangumi 页面已变化，读不到收藏"))?;
        collection.form_hash = form_hash(page)
            .or_else(|| update.and_then(form_hash))
            .ok_or_else(|| Error::new("Bangumi 页面已变化，读不到表单校验码"))?;
        collection.episodes = episodes(page);
        Ok(collection)
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod http {
    use super::*;
    use std::time::Duration;

    pub(super) fn agent() -> Result<ureq::Agent> {
        let cookies = session().ok_or_else(|| Error {
            message: "请先登录 Bangumi 账号".into(),
            sign_in: true,
        })?;
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(20)))
            .http_status_as_error(false)
            .https_only(true)
            .max_redirects(0)
            .user_agent(crate::bangumi::user_agent())
            .build()
            .into();
        agent
            .cookie_jar_lock()
            .load_json(cookies.as_bytes())
            .map_err(|_| expired())?;
        Ok(agent)
    }

    fn failed(_: ureq::Error) -> Error {
        Error::new("暂时连不上 Bangumi")
    }

    /// A page of the website, following its redirects within it.
    pub(super) fn get(agent: &ureq::Agent, path: &str) -> Result<String> {
        let mut url = format!("{SITE}{path}");
        for _ in 0..4 {
            let mut response = agent.get(&url).call().map_err(failed)?;
            if response.status().is_redirection() {
                let location = response
                    .headers()
                    .get("location")
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("");
                url = if location.starts_with('/') && !location.starts_with("//") {
                    format!("{SITE}{location}")
                } else if location.starts_with("https://bgm.tv/") {
                    location.to_string()
                } else {
                    return Err(Error::new("Bangumi 返回了无法识别的跳转"));
                };
                continue;
            }
            if !response.status().is_success() {
                return Err(Error::new(format!(
                    "Bangumi 暂时无法处理请求（{}）",
                    response.status().as_u16()
                )));
            }
            return response
                .body_mut()
                .with_config()
                .limit(4 * 1024 * 1024)
                .read_to_string()
                .map_err(failed);
        }
        Err(Error::new("Bangumi 跳转次数过多"))
    }

    /// Post a form of the website; a redirect is its usual answer.
    pub(super) fn post(
        agent: &ureq::Agent,
        url: &str,
        referer: &str,
        form: &[(&str, &str)],
    ) -> Result<()> {
        let mut response = agent
            .post(url)
            .header("Referer", referer)
            .header("Origin", SITE)
            .send_form(form.iter().copied())
            .map_err(failed)?;
        let status = response.status();
        if status.is_success() || status.is_redirection() {
            let text = response.body_mut().read_to_string().unwrap_or_default();
            if text.contains("CHOBITS_UID = 0") {
                return Err(expired());
            }
            Ok(())
        } else {
            Err(Error::new(format!(
                "Bangumi 没能保存（{}）",
                status.as_u16()
            )))
        }
    }
}

/// The user's collection of `subject`, with its episode grid.
#[cfg(not(target_arch = "wasm32"))]
pub fn collection(subject: i64) -> Result<Collection> {
    if subject <= 0 {
        return Err(Error::new("无效的 Bangumi 条目"));
    }
    let agent = http::agent()?;
    let page = http::get(&agent, &format!("/subject/{subject}"))?;
    if page::collect_box(&page).is_some() {
        return page::parse(&page, None);
    }
    let update = http::get(&agent, &format!("/update/{subject}")).ok();
    page::parse(&page, update.as_deref())
}

/// Save `collection` as the collection of `subject`: its type, score,
/// comment, tags and privacy all at once, as the collect box posts them.
#[cfg(not(target_arch = "wasm32"))]
pub fn save(subject: i64, collection: &Collection) -> Result<()> {
    let interest = collection
        .interest
        .ok_or_else(|| Error::new("要先选择收藏类型"))?;
    if collection.form_hash.is_empty() {
        return Err(Error::new("读不到 Bangumi 表单校验码"));
    }
    let agent = http::agent()?;
    let interest = interest.id().to_string();
    let rating = collection.rating.clamp(0, 10).to_string();
    http::post(
        &agent,
        &format!(
            "{SITE}/subject/{subject}/interest/update?gh={}",
            collection.form_hash
        ),
        &format!("{SITE}/subject/{subject}"),
        &[
            ("referer", "ajax"),
            ("interest", &interest),
            ("rating", &rating),
            ("tags", &collection.tags),
            ("comment", &collection.comment),
            ("privacy", if collection.private { "1" } else { "0" }),
            ("update", "保存"),
        ],
    )
}

/// Mark the episodes `ids` of a collection watched, or take the marks
/// back, as the grid's 「看过」 and 「撤销」 do.
#[cfg(not(target_arch = "wasm32"))]
pub fn mark(subject: i64, collection: &Collection, ids: &[i64], watched: bool) -> Result<()> {
    let Some(&first) = ids.first() else {
        return Ok(());
    };
    let agent = http::agent()?;
    let referer = format!("{SITE}/subject/{subject}");
    let hash = &collection.form_hash;
    if watched {
        // 「看到」 posts the ids of every episode it marks, joined by commas.
        let joined = ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
        return http::post(
            &agent,
            &format!("{SITE}/subject/ep/{first}/status/watched?gh={hash}&ajax=1"),
            &referer,
            &[("ep_id", &joined)],
        );
    }
    for id in ids {
        http::post(
            &agent,
            &format!("{SITE}/subject/ep/{id}/status/remove?gh={hash}&ajax=1"),
            &referer,
            &[("ep_id", &id.to_string())],
        )?;
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
pub fn collection(_subject: i64) -> Result<Collection> {
    Err(Error::new("网页版暂时连不上 Bangumi"))
}

#[cfg(target_arch = "wasm32")]
pub fn save(_subject: i64, _collection: &Collection) -> Result<()> {
    Err(Error::new("网页版暂时连不上 Bangumi"))
}

#[cfg(target_arch = "wasm32")]
pub fn mark(_subject: i64, _collection: &Collection, _ids: &[i64], _watched: bool) -> Result<()> {
    Err(Error::new("网页版暂时连不上 Bangumi"))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    const SUBJECT: &str = r#"<html><head><script>var CHOBITS_VER = 'r1', CHOBITS_UID = 123, CHOBITS_USERNAME = 'me';</script></head><body>
        <div id="headerNeue2"><a href="https://bgm.tv/logout/a1b2c3d4">登出</a></div>
        <ul class="prg_list">
          <li class="subtitle"><span>本篇</span></li>
          <li><a href="/ep/1001" id="prg_1001" class="load-epinfo epBtnWatched" title="ep.01 旅途">01</a></li>
          <li><a href="/ep/1002" id="prg_1002" class="load-epinfo epBtnAir" title="ep.02">02</a></li>
          <li><a href="/ep/1003" id="prg_1003" class="load-epinfo epBtnWatched" title="ep.03">03</a></li>
          <li class="subtitle"><span>SP</span></li>
          <li><a href="/ep/1099" id="prg_1099" class="load-epinfo epBtnWatched" title="sp.1">1</a></li>
        </ul>
        <div id="collectBoxForm_wrap"><form id="collectBoxForm" name="collectBoxForm" method="post" action="/subject/393010/interest/update?gh=a1b2c3d4">
          <div class="collectType">
            <input type="radio" name="interest" id="wish" value="1">
            <input type="radio" name="interest" id="collect" value="2">
            <input type="radio" name="interest" id="do" value="3" checked="checked">
            <input type="radio" name="interest" id="on_hold" value="4">
            <input type="radio" name="interest" id="dropped" value="5">
          </div>
          <input type="radio" name="rating" class="star" value="7">
          <input type="radio" name="rating" class="star" value="8" checked="checked">
          <input id="tags" name="tags" type="text" value=" 奇幻 治愈 ">
          <textarea id="comment" name="comment"> 旅途很好看 </textarea>
          <input type="checkbox" id="privacy" name="privacy" value="1">
        </form></div></body></html>"#;

    #[test]
    fn the_subject_page_gives_the_collection_and_the_main_episodes() {
        let collection = page::parse(SUBJECT, None).unwrap();
        assert_eq!(collection.interest, Some(Interest::Do));
        assert_eq!(collection.rating, 8);
        assert_eq!(collection.comment, "旅途很好看");
        assert_eq!(collection.tags, "奇幻 治愈");
        assert!(!collection.private);
        assert_eq!(collection.form_hash, "a1b2c3d4");
        assert_eq!(
            collection.episodes,
            vec![
                Episode {
                    id: 1001,
                    number: 1,
                    watched: true
                },
                Episode {
                    id: 1002,
                    number: 2,
                    watched: false
                },
                Episode {
                    id: 1003,
                    number: 3,
                    watched: true
                },
            ]
        );
    }

    #[test]
    fn a_subject_not_collected_reads_as_none_and_a_guest_page_as_expired() {
        let fresh = SUBJECT.replace(r#" checked="checked">"#, ">").replace(
            r#"<input type="checkbox" id="privacy" name="privacy" value="1">"#,
            r#"<input type="checkbox" id="privacy" name="privacy" value="1" checked>"#,
        );
        let collection = page::parse(&fresh, None).unwrap();
        assert_eq!((collection.interest, collection.rating), (None, 0));
        assert!(collection.private);
        let guest = SUBJECT.replace("CHOBITS_UID = 123", "CHOBITS_UID = 0");
        assert!(page::parse(&guest, None).unwrap_err().sign_in);
        assert!(
            !page::parse("<h1>Checking your browser</h1>", None)
                .unwrap_err()
                .sign_in
        );
    }

    #[test]
    fn the_collect_box_may_come_from_its_own_page() {
        let (head, rest) = SUBJECT
            .split_once("<div id=\"collectBoxForm_wrap\">")
            .unwrap();
        let page = format!("{head}</body></html>");
        let update = format!("<div>{rest}");
        assert!(page::collect_box(&page).is_none());
        let collection = page::parse(&page, Some(&update)).unwrap();
        assert_eq!(collection.interest, Some(Interest::Do));
        assert_eq!(collection.episodes.len(), 3);
    }

    #[test]
    fn writing_needs_a_session_and_a_collection_type() {
        set_session(None);
        assert!(collection(393010).unwrap_err().sign_in);
        assert!(save(393010, &Collection::default()).is_err());
        assert!(mark(393010, &Collection::default(), &[], true).is_ok());
        assert_eq!(Interest::from_key("on_hold"), Some(Interest::OnHold));
    }
}
