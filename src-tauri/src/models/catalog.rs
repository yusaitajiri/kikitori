//! Embedded model catalog (section 12). Sizes and hashes come from the Hugging Face LFS
//! metadata of each pinned revision.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};

pub const DEFAULT_MODEL_ID: &str = "turbo-q5";
pub const LIGHT_MODEL_ID: &str = "small-q5";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub id: String,
    pub name_ja: String,
    pub name_en: String,
    pub description_ja: String,
    pub description_en: String,
    pub repo: String,
    pub file: String,
    pub revision: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub license: String,
    pub speed_tier: String,
    pub hidden: bool,
}

impl CatalogEntry {
    pub fn url(&self) -> String {
        format!("https://huggingface.co/{}/resolve/{}/{}", self.repo, self.revision, self.file)
    }

    pub fn path_in(&self, models_dir: &Path) -> PathBuf {
        models_dir.join(&self.file)
    }

    pub fn part_path_in(&self, models_dir: &Path) -> PathBuf {
        models_dir.join(format!("{}.part", self.file))
    }
}

static CATALOG: LazyLock<Vec<CatalogEntry>> =
    LazyLock::new(|| serde_json::from_str(include_str!("catalog.json")).expect("catalog.json is valid"));

pub fn all() -> &'static [CatalogEntry] {
    &CATALOG
}

pub fn find(id: &str) -> Option<&'static CatalogEntry> {
    CATALOG.iter().find(|e| e.id == id)
}

/// Section 12: `turbo-q5`, or `small-q5` under 8 GB of RAM.
pub fn recommend(total_ram_bytes: u64) -> &'static str {
    if total_ram_bytes > 0 && total_ram_bytes < 8 * 1024 * 1024 * 1024 { LIGHT_MODEL_ID } else { DEFAULT_MODEL_ID }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_complete_and_pinned() {
        let ids: Vec<&str> = all().iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["turbo-q5", "kotoba-v2-q5", "turbo-q8", "large-v3-q5", "small-q5", "base-q5"]);
        for e in all() {
            assert_eq!(e.sha256.len(), 64, "{}", e.id);
            assert!(e.sha256.chars().all(|c| c.is_ascii_hexdigit()));
            assert_eq!(e.revision.len(), 40, "{}", e.id);
            assert!(e.size_bytes > 50_000_000);
        }
        assert!(find("base-q5").unwrap().hidden);
        assert_eq!(
            find("turbo-q5").unwrap().url(),
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-large-v3-turbo-q5_0.bin"
        );
    }

    #[test]
    fn recommendation_by_ram() {
        assert_eq!(recommend(16 * 1024 * 1024 * 1024), "turbo-q5");
        assert_eq!(recommend(4 * 1024 * 1024 * 1024), "small-q5");
    }
}
