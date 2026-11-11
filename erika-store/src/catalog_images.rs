//! One persisted, validated TMDB rendition configuration for all consumers.
use crate::{Db, catalog::Result};
use erika_catalog::artwork::{Asset, ImageConfiguration};

const KEY: &str = "catalog.tmdb.images";
#[derive(Default)]
pub struct Images(Option<ImageConfiguration>);
impl Images {
    pub fn url(&self, asset: Option<&Asset>, width: u32) -> String {
        asset
            .and_then(|a| a.url(self.0.as_ref(), width).ok())
            .unwrap_or_default()
    }
}
impl Db {
    pub fn catalog_images(&self) -> Images {
        Images(
            self.setting(KEY)
                .and_then(|v| serde_json::from_str(&v).ok())
                .and_then(|v| ImageConfiguration::parse(&v).ok()),
        )
    }
    pub fn catalog_images_fresh(&self, now: i64) -> bool {
        self.setting(KEY)
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .filter(|body| ImageConfiguration::parse(body).is_ok())
            .and_then(|body| body["fetched_at"].as_i64())
            .is_some_and(|fetched| fetched <= now && now.saturating_sub(fetched) < 24 * 60 * 60)
    }
    pub fn catalog_save_images(&self, config: &ImageConfiguration) -> Result<()> {
        let mut body = serde_json::to_value(&config.sizes)?;
        body["secure_base_url"] = config.secure_base_url.clone().into();
        ImageConfiguration::parse(&body)?;
        body["fetched_at"] = crate::now().into();
        self.set_setting(KEY, Some(&serde_json::to_string(&body)?));
        Ok(())
    }
}
