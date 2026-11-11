//! Empty page values and the isolated legacy account adapter. Canonical page
//! projections live in `content`; no service-specific artwork is shown here.
use bern::{date::CalendarDate,media::MediaSource};
use erika_anime_protocol::anime::{Show,WatchStatus,Watching};
use erika_dandanplay::Details;
use erika_store::EpisodeFile;
use std::collections::BTreeSet;

pub fn untitled()->Show {
    Show {id:String::new(),title:String::new(),poster:MediaSource::new(""),backdrop:MediaSource::new(""),logo:MediaSource::new(""),episodes:0,watched:0,rating:0.,movie:false,premiere:CalendarDate::new(1970,1,1),has_premiere:false,tags:Vec::new(),summary:String::new(),status:WatchStatus::Unmarked}
}
pub fn nothing_watched()->Watching {
    Watching {id:String::new(),title:String::new(),still:MediaSource::new(""),episode:0,chapter:String::new(),left:0,progress:0.}
}

pub(crate) fn played(files:&[EpisodeFile])->BTreeSet<i64> {
    files.iter().filter(|f|f.duration>0. && f.position/f.duration>=0.9).map(|f|f.episode.number).collect()
}
pub(crate) fn number(text:&str,index:usize)->i64 {text.trim().parse().unwrap_or(index as i64+1)}
pub(crate) fn numbered(details:&Details)->Vec<(i64,i64)> {
    details.episodes.iter().enumerate().map(|(index,e)|(number(&e.episode_number,index),e.episode_id)).collect()
}
#[cfg(test)]
pub(crate) fn file_of(files:&[EpisodeFile],number:i64,id:i64)->Option<&EpisodeFile> {
    files.iter().find(|f|if id==0 || erika_dandanplay::bangumi::subject_id(id).is_some(){f.episode.number==number}else{f.episode.dandanplay_id==id})
}
