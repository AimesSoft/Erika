use super::{Intent, Job, Remote};
use crate::marks::Level;
use erika_catalog::{Kind, Provider};
use erika_dandanplay::{bangumi::site, catalog};

pub(super) fn follow(level: Level) -> &'static str {
    match level {
        Level::Want | Level::Watching => "favorited",
        Level::Watched => "finished",
        Level::Dropped => "abandoned",
    }
}
fn subject_id(job: &Job) -> Result<i64, String> {
    job.subject
        .reference
        .id
        .parse::<i64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or_else(|| "Bangumi 条目标识不正确".into())
}
pub(super) async fn read(job: &Job) -> Result<Remote, String> {
    if !job.session.is_current() {
        return Err("账号已切换".into());
    }
    match job.subject.reference.provider {
        Provider::Bangumi => site::collection_with(&job.session, subject_id(job)?)
            .map(Remote::Bangumi)
            .map_err(|e| e.message),
        Provider::Dandanplay => catalog::account_details(&job.session, &job.subject.reference.id)
            .await
            .map(Remote::Dandanplay)
            .map_err(|e| e.message),
        _ => Err("此来源不支持账号同步".into()),
    }
}
fn edited(
    collection: &site::Collection,
    intent: &Intent,
    movie_mark: Option<&erika_store::catalog::MediaMark>,
) -> Result<site::Collection, String> {
    let mut saved = collection.clone();
    match intent {
        Intent::Subject(task) => {
            if let Some(level) = task.mark.status.as_deref().and_then(Level::parse) {
                saved.interest = Some(level.interest());
            }
            if let Some(points) = task.mark.rating {
                saved.rating = points;
            }
            if let Some(text) = &task.mark.review {
                saved.comment = text.clone();
            }
            saved.interest.get_or_insert(site::Interest::Wish);
        }
        Intent::Status(level) => saved.interest = Some(level.interest()),
        Intent::Rate(points) => {
            saved.rating = *points;
            saved.interest.get_or_insert(site::Interest::Wish);
        }
        Intent::Review(text) => {
            saved.comment = text.clone();
            saved.interest.get_or_insert(site::Interest::Wish);
        }
        Intent::Watch(w) if w.target.kind == Kind::Subject => {
            let status = movie_mark
                .and_then(|m| m.status.as_deref())
                .and_then(Level::parse);
            if status.is_some_and(|s| (s == Level::Watched) != w.watched) {
                return Err("电影收藏状态与已看记录冲突".into());
            }
            saved.interest = Some(status.map(Level::interest).unwrap_or(if w.watched {
                site::Interest::Collect
            } else {
                site::Interest::Wish
            }));
        }
        _ => return Err("此操作不是收藏表单修改".into()),
    }
    Ok(saved)
}
async fn send(job: &Job, remote: &Remote) -> Result<(), String> {
    if !job.session.is_current() {
        return Err("账号已切换".into());
    }
    match (remote, &job.intent) {
        (Remote::Dandanplay(details), Intent::Subject(task)) => {
            let level = task
                .mark
                .status
                .as_deref()
                .and_then(Level::parse)
                .ok_or_else(|| "此同步任务没有收藏状态".to_string())?;
            catalog::account_follow(&job.session, details.anime_id, follow(level))
                .await
                .map_err(|e| e.message)
        }
        (Remote::Dandanplay(details), Intent::Status(level)) => {
            catalog::account_follow(&job.session, details.anime_id, follow(*level))
                .await
                .map_err(|e| e.message)
        }
        (Remote::Dandanplay(_), Intent::Watch(w)) if w.watched => {
            catalog::account_watched(&job.session, &w.target.id)
                .await
                .map_err(|e| e.message)
        }
        (Remote::Bangumi(c), Intent::Watch(w)) if w.target.kind == Kind::Episode => {
            let episode = w
                .target
                .id
                .parse::<i64>()
                .ok()
                .filter(|id| *id > 0)
                .ok_or_else(|| "Bangumi 分集标识不正确".to_string())?;
            site::mark_with(&job.session, subject_id(job)?, c, &[episode], w.watched)
                .map_err(|e| e.message)
        }
        (Remote::Bangumi(c), _) => {
            let mark = job
                .claim
                .as_ref()
                .and_then(|claim| claim.movie_mark.as_ref());
            site::save_with(
                &job.session,
                subject_id(job)?,
                &edited(c, &job.intent, mark)?,
            )
            .map_err(|e| e.message)
        }
        _ => Err("此账号不支持这项操作".into()),
    }
}
/// An HTTP acknowledgement is not proof of an applied write. Read the same
/// account and target back even after a timeout; do not replay the write.
pub(super) async fn write(job: &Job, remote: Remote) -> Result<Remote, String> {
    let sent = send(job, &remote).await;
    match read(job).await {
        Ok(observed) if observed.satisfies(&job.intent) => Ok(observed),
        Ok(_) => Err(sent
            .err()
            .unwrap_or_else(|| "远端没有确认此次修改，本地记录已保留".into())),
        Err(error) => Err(format!("发送结果尚未确认：{error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn movie_watch_projection_preserves_explicit_collection_choices() {
        use erika_catalog::{MediaId, ProviderRef};
        use erika_store::catalog::{MediaMark, WatchSync};
        let mut original = site::Collection::default();
        original.interest = Some(site::Interest::Collect);
        original.rating = 8;
        original.comment = "原评价".into();
        original.tags = "原标签".into();
        original.private = true;
        let id = MediaId::new();
        let mut watch = WatchSync {
            operation: MediaId::new().0,
            media: id.clone(),
            account: "bangumi:isolated".into(),
            target: ProviderRef {
                provider: Provider::Bangumi,
                scope: String::new(),
                kind: Kind::Subject,
                id: "77".into(),
            },
            watched: false,
            local_revision: 1,
            mapping_revision: 1,
            state: "sending".into(),
            error: String::new(),
            remote_baseline: None,
            revision: 1,
            lease_until: 0,
        };
        let mut mark = MediaMark {
            media: id,
            reference: None,
            status: None,
            rating: None,
            review: None,
            updated_at: 1,
            revision: 1,
        };
        for level in [Level::Want, Level::Watching, Level::Dropped] {
            mark.status = Some(level.key().into());
            let saved = edited(&original, &Intent::Watch(watch.clone()), Some(&mark)).unwrap();
            assert_eq!(saved.interest, Some(level.interest()));
            assert_eq!(saved.rating, original.rating);
            assert_eq!(saved.comment, original.comment);
            assert_eq!(saved.tags, original.tags);
            assert!(saved.private);
            watch.watched = true;
            assert!(edited(&original, &Intent::Watch(watch.clone()), Some(&mark)).is_err());
            watch.watched = false;
        }
        mark.status = Some("watched".into());
        assert!(edited(&original, &Intent::Watch(watch.clone()), Some(&mark)).is_err());
        watch.watched = true;
        assert_eq!(
            edited(&original, &Intent::Watch(watch.clone()), Some(&mark))
                .unwrap()
                .interest,
            Some(site::Interest::Collect)
        );
        watch.watched = false;
        assert_eq!(
            edited(&original, &Intent::Watch(watch), None)
                .unwrap()
                .interest,
            Some(site::Interest::Wish)
        );
    }
    #[test]
    fn editing_one_field_preserves_the_other_fields() {
        let mut original = site::Collection::default();
        original.interest = Some(site::Interest::OnHold);
        original.tags = "原标签".into();
        original.private = true;
        original.comment = "原评价".into();
        original.rating = 7;
        let saved = edited(&original, &Intent::Rate(9), None).unwrap();
        assert_eq!(saved.rating, 9);
        assert_eq!(saved.interest, original.interest);
        assert_eq!(saved.comment, original.comment);
        assert_eq!(saved.tags, original.tags);
        assert!(saved.private);
        let saved = edited(&original, &Intent::Status(Level::Want), None).unwrap();
        assert_eq!(saved.interest, Some(site::Interest::Wish));
        assert_eq!(saved.rating, 7);
        assert_eq!(saved.comment, original.comment);
        assert_eq!(follow(Level::Watching), "favorited");
    }
}
