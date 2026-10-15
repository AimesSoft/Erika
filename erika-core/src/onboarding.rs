//! Onboarding edits a draft; only finishing starts/stops optional modules.
use super::*;
const NAMES: [&str; 8] = [
    "弹幕",
    "画质增强",
    "媒体库",
    "追番",
    "账号同步",
    "网络媒体源",
    "局域网与远程",
    "下载",
];
const ANIME: [&str; 3] = ["danmaku", "library", "anime"];
pub fn initial() -> Onboarding {
    let mut draft = Onboarding {
        visible: true,
        step: 0,
        preview_revision: 0,
        preset: OnboardingPreset::Anime,
        features: settings::feature_selection(&ANIME.map(String::from)),
        modules: settings::ALL_MODULES
            .into_iter()
            .zip(NAMES)
            .map(|(id, name)| OnboardingModule {
                id: id.into(),
                name: name.into(),
                enabled: ANIME.contains(&id),
            })
            .collect(),
        count: 3,
        summary: String::new(),
    };
    refresh(&mut draft);
    draft
}
fn refresh(draft: &mut Onboarding) {
    let enabled: Vec<String> = draft
        .modules
        .iter()
        .filter(|m| m.enabled)
        .map(|m| m.id.clone())
        .collect();
    draft.features = settings::feature_selection(&enabled);
    draft.count = enabled.len() as i64;
    let name = if enabled.is_empty() {
        draft.preset = OnboardingPreset::Pure;
        "纯粹播放"
    } else if enabled.len() == 8 {
        draft.preset = OnboardingPreset::All;
        "全都要"
    } else if enabled.len() == 3 && ANIME.iter().all(|id| enabled.iter().any(|m| m == id)) {
        draft.preset = OnboardingPreset::Anime;
        "看番"
    } else {
        draft.preset = OnboardingPreset::Custom;
        ""
    };
    draft.summary = if name.is_empty() {
        format!("已开启 {} 个模块", draft.count)
    } else {
        format!("已开启 {} 个模块 · 正好是「{}」", draft.count, name)
    };
}
impl ErikaEngine {
    pub(crate) fn select_onboarding_preset(&mut self, preset: OnboardingPreset) {
        if preset == OnboardingPreset::Custom {
            self.state.onboarding.preset = preset;
            return;
        }
        for module in &mut self.state.onboarding.modules {
            module.enabled = preset == OnboardingPreset::All
                || (preset == OnboardingPreset::Anime && ANIME.contains(&module.id.as_str()));
        }
        refresh(&mut self.state.onboarding);
    }
    pub(crate) fn toggle_onboarding_module(&mut self, id: &str) -> Result<(), CommandError> {
        let draft = &mut self.state.onboarding;
        let module = draft
            .modules
            .iter_mut()
            .find(|m| m.id == id)
            .ok_or_else(|| CommandError::new("unknown_module", "unknown module"))?;
        module.enabled = !module.enabled;
        let enabled = module.enabled;
        for other in &mut draft.modules {
            match (id, enabled, other.id.as_str()) {
                ("sources" | "remote", true, "library") | ("sync", true, "anime") => {
                    other.enabled = true
                }
                ("library", false, "sources" | "remote") | ("anime", false, "sync") => {
                    other.enabled = false
                }
                _ => {}
            }
        }
        refresh(draft);
        Ok(())
    }
    pub(crate) fn advance_onboarding(&mut self) -> Result<(), CommandError> {
        if !self.state.onboarding.visible {
            return Ok(());
        }
        if self.state.onboarding.step < 3 {
            self.state.onboarding.step += 1;
            return Ok(());
        }
        let enabled: Vec<String> = self
            .state
            .onboarding
            .modules
            .iter()
            .filter(|m| m.enabled)
            .map(|m| m.id.clone())
            .collect();
        self.write_welcome_preferences(&enabled)?;
        for module in self
            .state
            .onboarding
            .modules
            .iter()
            .rev()
            .filter(|m| !m.enabled)
        {
            bern::modules::set_enabled(&module.id, false);
        }
        for module in self.state.onboarding.modules.iter().filter(|m| m.enabled) {
            bern::modules::set_enabled(&module.id, true);
        }
        self.saved_modules = Some(enabled);
        self.onboarding_completed = true;
        self.state.onboarding.visible = false;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use erika::Commands;
    #[test]
    fn preview_is_a_draft_and_dependencies_work_in_both_directions() {
        let mut e = ErikaEngine::default();
        let original = e.state.features.clone();
        e.onboarding_preset(OnboardingPreset::Pure).unwrap();
        e.onboarding_toggle("remote".into()).unwrap();
        assert!(e.state.onboarding.features.library && e.state.onboarding.features.remote);
        e.onboarding_toggle("sources".into()).unwrap();
        e.onboarding_toggle("library".into()).unwrap();
        assert!(!e.state.onboarding.features.remote && !e.state.onboarding.features.sources);
        e.onboarding_toggle("sync".into()).unwrap();
        assert!(e.state.onboarding.features.anime && e.state.onboarding.features.sync);
        e.onboarding_toggle("anime".into()).unwrap();
        assert!(!e.state.onboarding.features.sync);
        assert_eq!(e.state.features, original);
    }
    #[test]
    fn skip_goes_to_review_with_no_modules_and_finish_closes() {
        let mut e = ErikaEngine::default();
        e.onboarding_skip().unwrap();
        assert_eq!(e.state.onboarding.step, 3);
        assert!(e.state.onboarding.visible);
        assert_eq!(e.state.onboarding.count, 0);
        e.onboarding_next().unwrap();
        assert!(!e.state.onboarding.visible);
    }
    #[test]
    fn back_keeps_the_draft_and_custom_keeps_current_selection() {
        let mut e = ErikaEngine::default();
        e.onboarding_next().unwrap();
        e.onboarding_preset(OnboardingPreset::All).unwrap();
        e.onboarding_preset(OnboardingPreset::Custom).unwrap();
        e.onboarding_next().unwrap();
        e.onboarding_back().unwrap();
        assert_eq!(e.state.onboarding.step, 1);
        assert_eq!(e.state.onboarding.preset, OnboardingPreset::Custom);
        assert_eq!(e.state.onboarding.count, 8);
    }
}

impl ErikaEngine {
    pub(crate) fn restore_welcome_preferences(&mut self) {
        let Ok(Some(bytes)) = bern::storage::Store::host().read("welcome-v1") else {
            return;
        };
        let Ok(text) = String::from_utf8(bytes) else {
            return;
        };
        let mut lines = text.lines();
        if lines.next() != Some("erika-welcome-1") {
            return;
        }
        let Some(modules) = lines.next() else {
            return;
        };
        let enabled: Vec<String> = modules
            .split(',')
            .filter(|id| settings::ALL_MODULES.contains(id))
            .map(String::from)
            .collect();
        self.state.accent = match lines.next() {
            Some("Ultramarine") => Accent::Ultramarine,
            Some("Neon") => Accent::Neon,
            _ => Accent::Crimson,
        };
        self.state.appearance = match lines.next() {
            Some("Light") => Appearance::Light,
            Some("Dark") => Appearance::Dark,
            _ => Appearance::System,
        };
        self.saved_modules = Some(enabled);
        self.onboarding_completed = true;
        self.state.onboarding.visible = false;
    }
    pub(crate) fn save_welcome_preferences(&self) -> Result<(), CommandError> {
        if !self.onboarding_completed {
            return Ok(());
        }
        self.write_welcome_preferences(self.saved_modules.as_deref().unwrap_or(&[]))
    }
    fn write_welcome_preferences(&self, enabled: &[String]) -> Result<(), CommandError> {
        let modules = enabled.join(",");
        let text = format!(
            "erika-welcome-1\n{}\n{:?}\n{:?}\n",
            modules, self.state.accent, self.state.appearance
        );
        bern::storage::Store::host().write("welcome-v1", text.as_bytes())
    }
}
