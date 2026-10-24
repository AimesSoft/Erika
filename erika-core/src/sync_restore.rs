//! The account sync wrote what came from another device or a file into
//! this device's storage: the shell reads its part anew and asks the
//! library and the anime pages to do the same.
use super::*;

impl ErikaEngine {
    pub(crate) fn take_synced(
        &mut self,
        settings: bool,
        accounts: bool,
        library: bool,
        history: bool,
        sources: Vec<i64>,
    ) {
        if settings {
            let before = self.saved_modules.clone();
            self.restore_welcome_preferences();
            let after = self.saved_modules.clone();
            if after != before
                && let Some(selected) = after
            {
                // Start and stop the modules from what ran before.
                self.saved_modules = before;
                if let Err(error) = self.apply_feature_selection(selected) {
                    eprintln!("erika: apply synced features: {}", error.message);
                }
            }
            self.restore_playback_preferences();
            self.state.player.subtitle_supersampling =
                self.db.setting("subtitle_supersampling").as_deref() != Some("false");
            self.state.player.skip_intro =
                self.db.setting("skip_segments").as_deref() != Some("false");
            self.state.danmaku_settings.supersampling =
                self.db.setting("danmaku_supersampling").as_deref() != Some("false");
            self.apply_enhancement();
            self.apply_danmaku();
            self.plugins_restored();
        }
        #[cfg(feature = "bern-module-account")]
        if accounts && self.state.features.account {
            self.set_account_enabled(true);
        }
        #[cfg(not(feature = "bern-module-account"))]
        let _ = accounts;
        if history {
            self.show_recents();
        }
        #[cfg(feature = "bern-module-library")]
        if library || history {
            erika_library_protocol::library::calls::history_changed();
            for source in sources {
                erika_library_protocol::library::calls::rescan(source);
            }
        }
        #[cfg(not(feature = "bern-module-library"))]
        let _ = sources;
        #[cfg(feature = "bern-module-anime")]
        if library || history {
            erika_anime_protocol::anime::calls::history_changed();
        }
    }
}
