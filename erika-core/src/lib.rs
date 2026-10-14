//! The engine of Erika's shell. In this first phase nothing plays: the
//! player keeps what the interface needs to show, and the picture in the
//! frame is a still until the kernel draws video.

use bern::CommandError;
use bern::media::MediaSource;
use bern::resources::Resources;
use erika_protocol::erika::{self, Accent, Appearance, State};

/// An episode's length in seconds, until the kernel reports real ones.
const LENGTH: f64 = 1455.0;

/// The still the frame shows for a file, until the kernel draws video.
const STILL: &str = "design/assets/still1.jpg";

pub struct ErikaEngine {
    state: State,
}

impl Default for ErikaEngine {
    fn default() -> Self {
        Self {
            state: State {
                accent: Accent::Crimson,
                appearance: Appearance::System,
                playing: false,
                expanded: false,
                paused: false,
                title: String::new(),
                subtitle: String::new(),
                still: still(),
                progress: 0.0,
                elapsed: clock(0.0),
                duration: clock(LENGTH),
                user_name: "Nipa".into(),
            },
        }
    }
}

fn still() -> MediaSource {
    MediaSource::resource(&Resources::host(), STILL).unwrap_or_else(|_| MediaSource::new(""))
}

/// `m:ss` of a number of seconds.
fn clock(seconds: f64) -> String {
    let seconds = seconds.max(0.0).round() as i64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

impl ErikaEngine {
    fn start(&mut self, title: String, subtitle: String, still: MediaSource, progress: f64) {
        let progress = progress.clamp(0.0, 1.0);
        self.state.playing = true;
        self.state.expanded = true;
        self.state.paused = false;
        self.state.title = title;
        self.state.subtitle = subtitle;
        self.state.still = still;
        self.state.progress = progress;
        self.state.elapsed = clock(progress * LENGTH);
        self.state.duration = clock(LENGTH);
    }

    fn require_playing(&self) -> Result<(), CommandError> {
        if self.state.playing {
            Ok(())
        } else {
            Err(CommandError::new("nothing_playing", "nothing is playing"))
        }
    }
}

impl erika::Commands for ErikaEngine {
    fn open_file(&mut self, source: MediaSource) -> Result<(), CommandError> {
        let title = source.file_name().unwrap_or_else(|| source.url.clone());
        self.start(title, "本地文件".into(), still(), 0.0);
        Ok(())
    }

    fn play(
        &mut self,
        title: String,
        subtitle: String,
        still: MediaSource,
        progress: f64,
    ) -> Result<(), CommandError> {
        self.start(title, subtitle, still, progress);
        Ok(())
    }

    fn minimize(&mut self) -> Result<(), CommandError> {
        self.require_playing()?;
        self.state.expanded = false;
        Ok(())
    }

    fn expand(&mut self) -> Result<(), CommandError> {
        self.require_playing()?;
        self.state.expanded = true;
        Ok(())
    }

    fn close_player(&mut self) -> Result<(), CommandError> {
        self.state.playing = false;
        self.state.expanded = false;
        Ok(())
    }

    fn toggle_pause(&mut self) -> Result<(), CommandError> {
        self.require_playing()?;
        self.state.paused = !self.state.paused;
        Ok(())
    }

    fn set_accent(&mut self, accent: Accent) -> Result<(), CommandError> {
        self.state.accent = accent;
        Ok(())
    }

    fn set_appearance(&mut self, appearance: Appearance) -> Result<(), CommandError> {
        self.state.appearance = appearance;
        Ok(())
    }
}

impl erika::Engine for ErikaEngine {
    fn state(&self) -> &State {
        &self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use erika::Commands;

    #[test]
    fn the_player_shrinks_into_the_mini_player_and_grows_back() {
        let mut engine = ErikaEngine::default();
        assert_eq!(engine.minimize().unwrap_err().code, "nothing_playing");
        engine
            .play("葬送的芙莉莲".into(), "第 4 集".into(), still(), 0.35)
            .unwrap();
        assert!(engine.state.playing && engine.state.expanded);
        assert_eq!(engine.state.elapsed, "8:29");
        engine.minimize().unwrap();
        assert!(engine.state.playing && !engine.state.expanded);
        engine.expand().unwrap();
        assert!(engine.state.expanded);
        engine.close_player().unwrap();
        assert!(!engine.state.playing);
    }

    #[test]
    fn files_play_under_their_names() {
        let mut engine = ErikaEngine::default();
        engine
            .open_file(MediaSource::new("file:///Videos/%E5%BD%B1.mkv"))
            .unwrap();
        assert_eq!(engine.state.title, "影.mkv");
        assert_eq!(engine.state.subtitle, "本地文件");
    }
}
