//! Onboarding edits a draft; only finishing starts/stops optional modules.
use super::*;
use settings::ANIME_MODULES;
pub fn initial() -> Onboarding {
    let mut draft = Onboarding {
        visible: true,
        step: 0,
        preview_revision: 0,
        preset: OnboardingPreset::Anime,
        features: settings::feature_selection(
            &ANIME_MODULES
                .iter()
                .copied()
                .map(String::from)
                .collect::<Vec<_>>(),
        ),
        modules: settings::ALL_MODULES
            .iter()
            .copied()
            .filter(|id| *id != "downloads" || !settings::downloader_hidden())
            .map(|id| OnboardingModule {
                id: id.into(),
                module: settings::feature_module(id),
                enabled: ANIME_MODULES.contains(&id),
            })
            .collect(),
        count: ANIME_MODULES.len() as i64,
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
    draft.preset = settings::preset_of(&enabled);
}
impl ErikaEngine {
    pub(crate) fn select_onboarding_preset(&mut self, preset: OnboardingPreset) {
        if preset == OnboardingPreset::Custom {
            self.state.onboarding.preset = preset;
            return;
        }
        for module in &mut self.state.onboarding.modules {
            module.enabled = preset == OnboardingPreset::All
                || (preset == OnboardingPreset::Anime
                    && ANIME_MODULES.contains(&module.id.as_str()));
        }
        refresh(&mut self.state.onboarding);
    }
    pub(crate) fn toggle_onboarding_module(&mut self, id: &str) -> Result<(), CommandError> {
        let draft = &mut self.state.onboarding;
        let module = draft
            .modules
            .iter()
            .find(|m| m.id == id)
            .ok_or_else(|| CommandError::new("unknown_module", "unknown module"))?;
        let enabled = !module.enabled;
        let mut selected = draft
            .modules
            .iter()
            .filter(|m| m.enabled)
            .map(|m| m.id.clone())
            .collect();
        settings::change_feature(&mut selected, id, enabled);
        for module in &mut draft.modules {
            module.enabled = selected.contains(&module.id);
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
        self.apply_feature_selection(enabled)?;
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
        assert!(e.state.onboarding.features.account);
        e.onboarding_toggle("account".into()).unwrap();
        assert!(!e.state.onboarding.features.account && !e.state.onboarding.features.sync);
        e.onboarding_toggle("sync".into()).unwrap();
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
    fn library_only_onboarding_starts_details_without_selecting_new_shows() {
        use erika::Engine;
        let mut engine = ErikaEngine::default();
        engine.onboarding_skip().unwrap();
        engine.onboarding_toggle("library".into()).unwrap();
        assert!(!engine.state.onboarding.features.anime);
        engine.onboarding_next().unwrap();
        let runtime = engine.initial_modules().unwrap();
        assert!(runtime.iter().any(|id| id == "anime"));
        engine.modules_changed(&runtime);
        assert!(engine.state.features.library);
        assert!(!engine.state.features.anime);
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
        assert_eq!(e.state.onboarding.count, settings::ALL_MODULES.len() as i64);
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
        let Some(preferences) = parse_welcome_preferences(&text) else {
            return;
        };
        self.state.accent = preferences.accent;
        self.state.appearance = preferences.appearance;
        self.saved_modules = Some(preferences.modules);
        self.onboarding_completed = true;
        self.state.onboarding.visible = false;
    }
    pub(crate) fn save_welcome_preferences(&self) -> Result<(), CommandError> {
        if !self.onboarding_completed {
            return Ok(());
        }
        self.write_welcome_preferences(self.saved_modules.as_deref().unwrap_or(&[]))
    }
    pub(crate) fn write_welcome_preferences(&self, enabled: &[String]) -> Result<(), CommandError> {
        let text = welcome_preferences_text(enabled, &self.state.accent, &self.state.appearance);
        bern::storage::Store::host().write("welcome-v1", text.as_bytes())
    }
}

struct WelcomePreferences {
    modules: Vec<String>,
    accent: Accent,
    appearance: Appearance,
}

fn parse_welcome_preferences(text: &str) -> Option<WelcomePreferences> {
    let mut lines = text.lines();
    let version = lines.next()?;
    if version != "erika-welcome-1" && version != "erika-welcome-2" {
        return None;
    }
    let saved = lines.next()?;
    let mut modules: Vec<String> = saved
        .split(',')
        .filter(|id| settings::ALL_MODULES.contains(id))
        .map(String::from)
        .collect();
    // Accounts were implicit in v1. Preserve them for existing non-pure
    // setups, while v2 records an explicit account switch like every module.
    if version == "erika-welcome-1"
        && !saved.is_empty()
        && cfg!(feature = "bern-module-account")
        && !modules.iter().any(|id| id == "account")
    {
        modules.insert(0, "account".into());
    }
    let accent = match lines.next() {
        Some("Ultramarine") => Accent::Ultramarine,
        Some("Neon") => Accent::Neon,
        Some("Ribbon") => Accent::Ribbon,
        _ => Accent::Crimson,
    };
    let appearance = match lines.next() {
        Some("Light") => Appearance::Light,
        Some("Dark") => Appearance::Dark,
        _ => Appearance::System,
    };
    Some(WelcomePreferences {
        modules,
        accent,
        appearance,
    })
}

fn welcome_preferences_text(
    modules: &[String],
    accent: &Accent,
    appearance: &Appearance,
) -> String {
    format!(
        "erika-welcome-2\n{}\n{accent:?}\n{appearance:?}\n",
        modules.join(",")
    )
}

#[cfg(test)]
mod persistence_tests {
    use super::*;

    #[test]
    fn saved_library_only_choices_restore_details_without_enabling_new_shows() {
        for version in ["erika-welcome-1", "erika-welcome-2"] {
            let saved =
                parse_welcome_preferences(&format!("{version}\nlibrary\nCrimson\nSystem\n"))
                    .unwrap();
            let runtime = settings::runtime_modules(&saved.modules);
            assert!(runtime.iter().any(|id| id == "anime"));
            assert!(!settings::feature_selection(&saved.modules).anime);
            let text = welcome_preferences_text(&saved.modules, &saved.accent, &saved.appearance);
            let restored = parse_welcome_preferences(&text).unwrap();
            assert_eq!(restored.modules, saved.modules);
        }
    }

    #[test]
    fn old_pure_mode_has_no_account_and_old_nonpure_retains_it() {
        let pure = parse_welcome_preferences("erika-welcome-1\n\nCrimson\nSystem\n").unwrap();
        assert!(pure.modules.is_empty());
        let old = parse_welcome_preferences(
            "erika-welcome-1\ndan m aku,danmaku,library,anime\nNeon\nDark\n",
        )
        .unwrap();
        assert_eq!(
            old.modules.iter().any(|id| id == "account"),
            cfg!(feature = "bern-module-account")
        );
        assert!(!old.modules.iter().any(|id| id == "dan m aku"));
        assert_eq!(old.accent, Accent::Neon);
        assert_eq!(old.appearance, Appearance::Dark);
    }

    #[test]
    fn explicit_account_off_survives_saving_and_restoring() {
        let modules: Vec<String> = settings::ALL_MODULES
            .iter()
            .filter(|id| **id != "account" && **id != "sync")
            .map(|id| (*id).into())
            .collect();
        let saved = welcome_preferences_text(&modules, &Accent::Ribbon, &Appearance::Light);
        let restored = parse_welcome_preferences(&saved).unwrap();
        assert_eq!(restored.modules, modules);
        assert!(!settings::feature_selection(&restored.modules).account);
        assert_eq!(restored.accent, Accent::Ribbon);
        assert_eq!(restored.appearance, Appearance::Light);
    }
}
