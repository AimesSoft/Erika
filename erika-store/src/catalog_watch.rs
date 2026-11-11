//! Shared delivery policy: a write needs an exact confirmed account mapping;
//! uncertain outcomes must be read back before they can be retried.
use crate::catalog::{Error, Result, WatchSync};
use erika_catalog::{Kind, MediaNode, Provider, ProviderLink, Relation, Watched};

pub(crate) fn validate(
    input: &WatchSync,
    media: &MediaNode,
    watched: Option<&Watched>,
    links: &[ProviderLink],
) -> Result<()> {
    if input.account.trim().is_empty()
        || input.account.len() > 256
        || input.account.chars().any(char::is_control)
        || input.state != "pending"
        || !matches!(
            input.target.provider,
            Provider::Dandanplay | Provider::Bangumi
        )
    {
        return Err(Error::invalid("观看同步目标不正确"));
    }
    input.target.validate()?;
    if media.id != input.media
        || !media.kind.playable()
        || !(input.target.kind == Kind::Episode
            || (media.kind == Kind::Movie
                && input.target.provider == Provider::Bangumi
                && input.target.kind == Kind::Subject))
    {
        return Err(Error::invalid("观看同步缺少正确层级的目标"));
    }
    if !current(input, watched, links) {
        return Err(Error::conflict("观看记录或已确认的精确关联已改变"));
    }
    if input.target.provider == Provider::Dandanplay && !input.watched {
        return Err(Error::invalid(
            "弹弹play 不支持撤回已看；本地未看状态已保留",
        ));
    }
    Ok(())
}
fn current(input: &WatchSync, watched: Option<&Watched>, links: &[ProviderLink]) -> bool {
    watched.is_some_and(|w| {
        w.media == input.media && w.revision == input.local_revision && w.watched == input.watched
    }) && links.iter().any(|l| {
        l.media == input.media
            && l.confirmed
            && l.relation == Relation::Exact
            && l.reference == input.target
            && l.revision == input.mapping_revision
    })
}
pub(crate) fn dedup(input: &WatchSync) -> Result<String> {
    Ok(serde_json::to_string(&(
        &input.media,
        &input.account,
        &input.target,
        input.watched,
        input.local_revision,
        input.mapping_revision,
    ))?)
}
fn bump(input: &mut WatchSync) -> Result<()> {
    input.revision = input
        .revision
        .checked_add(1)
        .filter(|n| *n <= i64::MAX as u64)
        .ok_or_else(|| Error::conflict("观看同步版本超出范围"))?;
    Ok(())
}
/// Unknown blocks the same remote target as sending: a second write must not
/// race an unobserved first write. Accounts and independent targets can proceed.
pub(crate) fn blocks(previous: &WatchSync, input: &WatchSync) -> bool {
    previous.operation != input.operation
        && previous.account == input.account
        && previous.target == input.target
        && matches!(previous.state.as_str(), "sending" | "unknown")
}
pub(crate) fn recover(input: &WatchSync, at: i64) -> Result<Option<WatchSync>> {
    if at < 0 {
        return Err(Error::invalid("恢复时间不正确"));
    }
    if input.state != "sending" || input.lease_until >= at {
        return Ok(None);
    }
    let mut value = input.clone();
    value.state = "unknown".into();
    value.error = "发送中断，需回读远端状态".into();
    value.lease_until = 0;
    bump(&mut value)?;
    Ok(Some(value))
}
pub(crate) fn transition(
    input: &WatchSync,
    expected: &WatchSync,
    state: &str,
    error: &str,
    observed: Option<bool>,
    watched: Option<&Watched>,
    links: &[ProviderLink],
) -> Result<WatchSync> {
    if input != expected {
        return Err(Error::conflict("观看同步任务已改变"));
    }
    let allowed = matches!(
        (input.state.as_str(), state),
        ("pending", "sending" | "cancelled")
            | ("sending", "sent" | "unknown" | "failed")
            | ("unknown", "sent" | "pending" | "conflict")
            | ("failed", "pending" | "cancelled")
    );
    if !allowed || error.len() > 8192 || (input.state == "unknown" && observed.is_none()) {
        return Err(Error::invalid("观看同步状态转换不正确"));
    }
    if input.state == "unknown"
        && ((state == "sent" && observed != Some(input.watched))
            || (state == "pending" && observed == Some(input.watched)))
    {
        return Err(Error::invalid("远端回读结果与重试决定不一致"));
    }
    let mut value = input.clone();
    value.state = state.into();
    value.error = error.into();
    if let Some(observed) = observed {
        value.remote_baseline = Some(observed);
    }
    if matches!(state, "sending" | "pending") && !current(input, watched, links) {
        value.state = "conflict".into();
        value.error = "观看记录或关联已改变".into();
    }
    value.lease_until = if value.state == "sending" {
        crate::now().saturating_add(300)
    } else {
        0
    };
    bump(&mut value)?;
    Ok(value)
}
