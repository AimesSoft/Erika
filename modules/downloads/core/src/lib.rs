//! Download UI simulation. It never fetches a URL or writes media files.
use bern::{CommandError, media::MediaSource, resources::Resources};
use erika_downloads_protocol::downloads::{self, DownloadTab, DownloadTask, State};

pub struct DownloadsEngine {
    state: State,
    tasks: Vec<DownloadTask>,
    next_id: i64,
}
impl Default for DownloadsEngine {
    fn default() -> Self {
        let fixtures = [
            (
                "[LoliHouse] Sousou no Frieren S2 - 04 [1080p].mkv",
                "1.2 GB",
                0.64,
                8.4,
                true,
            ),
            (
                "[Nekomoe] Kaguya-sama - 01 [1080p].mkv",
                "860 MB",
                0.92,
                6.1,
                true,
            ),
            (
                "[ANi] Jujutsu Kaisen - 05 [1080p].mp4",
                "540 MB",
                0.22,
                0.0,
                false,
            ),
            (
                "[Sakurato] Oshi no Ko S3 - 03 [1080p].mkv",
                "980 MB",
                1.0,
                0.0,
                false,
            ),
        ];
        let mut engine = Self {
            state: State {
                tab: DownloadTab::Active,
                active: vec![],
                completed: vec![],
                active_count: 0,
                completed_count: 0,
                running: false,
                notice: String::new(),
                notice_revision: 0,
            },
            tasks: fixtures
                .into_iter()
                .enumerate()
                .map(
                    |(i, (title, size, progress, speed, running))| DownloadTask {
                        id: i as i64 + 1,
                        title: title.into(),
                        size: size.into(),
                        progress,
                        speed,
                        running,
                        completed: progress >= 1.0,
                        percent: (progress * 100.0).round() as i64,
                        first: false,
                    },
                )
                .collect(),
            next_id: 5,
        };
        engine.refresh();
        engine
    }
}
impl DownloadsEngine {
    fn refresh(&mut self) {
        self.state.active = self
            .tasks
            .iter()
            .filter(|t| !t.completed)
            .cloned()
            .collect();
        self.state.completed = self.tasks.iter().filter(|t| t.completed).cloned().collect();
        if let Some(first) = self.state.active.first_mut() {
            first.first = true;
        }
        if let Some(first) = self.state.completed.first_mut() {
            first.first = true;
        }
        self.state.active_count = self.state.active.len() as i64;
        self.state.completed_count = self.state.completed.len() as i64;
        self.state.running = self.tasks.iter().any(|t| t.running && !t.completed);
    }
}
impl downloads::Commands for DownloadsEngine {
    fn set_tab(&mut self, tab: DownloadTab) -> Result<(), CommandError> {
        self.state.tab = tab;
        Ok(())
    }
    fn add_task(&mut self) -> Result<(), CommandError> {
        self.tasks.insert(
            0,
            DownloadTask {
                id: self.next_id,
                title: "新任务 · 正在获取元数据…".into(),
                size: "—".into(),
                progress: 0.02,
                speed: 9.0,
                running: true,
                completed: false,
                percent: 2,
                first: false,
            },
        );
        self.next_id += 1;
        self.state.tab = DownloadTab::Active;
        self.refresh();
        Ok(())
    }
    fn toggle(&mut self, id: i64) -> Result<(), CommandError> {
        let task = self
            .tasks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or_else(|| CommandError::new("unknown_task", "unknown download"))?;
        if task.completed {
            return Err(CommandError::new(
                "completed_task",
                "download already completed",
            ));
        }
        task.running = !task.running;
        if task.running && task.speed == 0.0 {
            task.speed = 4.2;
        }
        self.refresh();
        Ok(())
    }
    fn play(&mut self, id: i64) -> Result<(), CommandError> {
        let task = self
            .tasks
            .iter()
            .find(|t| t.id == id)
            .ok_or_else(|| CommandError::new("unknown_task", "unknown download"))?;
        if !task.completed {
            return Err(CommandError::new(
                "incomplete_task",
                "download is not complete",
            ));
        }
        let still = MediaSource::resource(&Resources::host(), "design/assets/still1.jpg")
            .unwrap_or_else(|_| MediaSource::new(""));
        downloads::emit::play(task.title.clone(), "已下载 · 本地文件".into(), still, 0.0);
        Ok(())
    }
    fn tick(&mut self) -> Result<(), CommandError> {
        for task in &mut self.tasks {
            if !task.running || task.completed {
                continue;
            }
            task.progress = (task.progress + task.speed / 6000.0).min(1.0);
            task.percent = (task.progress * 100.0).round() as i64;
            if task.progress >= 1.0 {
                task.completed = true;
                task.running = false;
                task.speed = 0.0;
                self.state.notice = format!("下载完成：{}", task.title);
                self.state.notice_revision += 1;
            }
        }
        self.refresh();
        Ok(())
    }
}
impl downloads::Engine for DownloadsEngine {
    fn state(&self) -> &State {
        &self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use downloads::Commands;
    #[test]
    fn paused_tasks_stay_put_and_resume_at_a_nonzero_speed() {
        let mut e = DownloadsEngine::default();
        e.tick().unwrap();
        assert_eq!(e.tasks[2].progress, 0.22);
        e.toggle(3).unwrap();
        e.tick().unwrap();
        assert!(e.tasks[2].progress > 0.22);
        assert_eq!(e.tasks[2].speed, 4.2);
        e.toggle(3).unwrap();
        let before = e.tasks[2].progress;
        e.tick().unwrap();
        assert_eq!(e.tasks[2].progress, before);
    }
    #[test]
    fn completing_moves_a_task_once_and_updates_counts() {
        let mut e = DownloadsEngine::default();
        e.tasks[1].progress = 0.9999;
        e.tick().unwrap();
        assert_eq!((e.state.active_count, e.state.completed_count), (2, 2));
        assert_eq!(e.state.notice_revision, 1);
        assert_eq!(e.tasks[1].progress, 1.0);
        e.tick().unwrap();
        assert_eq!(e.state.notice_revision, 1);
        assert!(e.toggle(2).is_err());
        assert!(e.play(3).is_err());
        assert!(e.play(2).is_ok());
    }
    #[test]
    fn adding_on_completed_tab_returns_to_active_with_stable_ids() {
        let mut e = DownloadsEngine::default();
        e.set_tab(DownloadTab::Completed).unwrap();
        e.add_task().unwrap();
        e.add_task().unwrap();
        assert_eq!(e.state.tab, DownloadTab::Active);
        assert_eq!(e.state.active_count, 5);
        assert_eq!(e.state.active[0].id, 6);
        assert!(e.state.active[0].first);
        assert!(!e.state.active[1].first);
    }
}
