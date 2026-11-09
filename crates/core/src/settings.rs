//! User preferences and the recent-studies list (persisted by the UI).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use summrise_dicom::StudyIndex;
use summrise_i18n::{tr, tr_n};

use crate::view::Layout;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Appearance {
    #[default]
    System,
    Dark,
    Light,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub appearance: Appearance,
    /// Corner annotations, orientation labels and scale bar.
    pub show_overlays: bool,
    /// Hide patient name, ID and birth date (e.g. for screenshots).
    pub hide_patient_details: bool,
    /// Bilinear display interpolation (off = show raw pixels).
    pub smooth_interpolation: bool,
    pub default_layout: Layout,
    /// Upper bound for decoded series kept in memory.
    pub memory_budget_mb: u32,
    /// Remember recently opened folders (stores folder path and study
    /// description/date only — never patient names or IDs).
    pub remember_recent: bool,
    pub show_browser: bool,
    pub show_inspector: bool,
    /// Plain-language explanations: the description of each series, the
    /// explanations of parameters and presets and the position figure.
    pub show_explanations: bool,
    /// The language of the interface (a code of `summrise_i18n::LANGUAGES`,
    /// such as `"de"`); `None` follows the language of the system.
    pub language: Option<String>,
    /// The notice that measurements are approximate and not for diagnosis was
    /// shown. Nothing about the measurements themselves is ever stored.
    pub measure_notice_shown: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            appearance: Appearance::System,
            show_overlays: true,
            hide_patient_details: false,
            smooth_interpolation: true,
            default_layout: Layout::Single,
            memory_budget_mb: 4096,
            remember_recent: true,
            show_browser: true,
            show_inspector: true,
            show_explanations: true,
            language: None,
            measure_notice_shown: false,
        }
    }
}

impl Settings {
    pub fn memory_budget_bytes(&self) -> usize {
        self.memory_budget_mb as usize * 1024 * 1024
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentEntry {
    pub path: PathBuf,
    pub label: String,
    pub opened_unix: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecentStudies {
    pub entries: Vec<RecentEntry>,
}

impl RecentStudies {
    pub const MAX: usize = 8;

    pub fn add(&mut self, path: &Path, label: String) {
        let opened_unix =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or_default();
        self.entries.retain(|e| e.path != path);
        self.entries.insert(0, RecentEntry { path: path.to_path_buf(), label, opened_unix });
        self.entries.truncate(Self::MAX);
    }

    pub fn remove(&mut self, path: &Path) {
        self.entries.retain(|e| e.path != path);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// A privacy-minimal label for a recent entry: study description, date and
/// modalities — no patient-identifying attributes. It is stored, so it stays
/// in the language it was made in.
pub fn recent_label(index: &StudyIndex) -> String {
    let Some(study) = index.studies.first() else {
        return tr!("DICOM folder").to_owned();
    };
    let mut parts = vec![study.title()];
    if let Some(date) = study.date.as_deref().and_then(summrise_dicom::format::date) {
        parts.push(date);
    }
    let modalities: Vec<&str> = index
        .studies
        .iter()
        .flat_map(|s| s.modalities())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    parts.push(modalities.join(", "));
    if index.studies.len() > 1 {
        parts.push(tr_n!("{n} study", "{n} studies", index.studies.len()));
    }
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_list_is_deduplicated_and_capped() {
        let mut r = RecentStudies::default();
        for i in 0..10 {
            r.add(Path::new(&format!("/cd/{i}")), format!("study {i}"));
        }
        assert_eq!(r.entries.len(), RecentStudies::MAX);
        r.add(Path::new("/cd/5"), "again".into());
        assert_eq!(r.entries[0].label, "again");
        assert_eq!(r.entries.iter().filter(|e| e.path == Path::new("/cd/5")).count(), 1);
        r.remove(Path::new("/cd/5"));
        assert!(r.entries.iter().all(|e| e.path != Path::new("/cd/5")));
    }

    #[test]
    fn explanations_are_on_by_default_also_for_settings_stored_before() {
        use serde::de::value::{Error, MapDeserializer};

        assert!(Settings::default().show_explanations);
        // Settings stored by a version that did not have the field.
        let stored = [("show_overlays", false), ("show_inspector", false)];
        let settings = Settings::deserialize(MapDeserializer::<_, Error>::new(stored.into_iter())).expect("settings");
        assert!(settings.show_explanations, "explanations are on when the field is missing");
        assert!(!settings.show_overlays && !settings.show_inspector, "stored fields are kept");
        // Switched off, it stays off.
        let stored = [("show_explanations", false)];
        let settings = Settings::deserialize(MapDeserializer::<_, Error>::new(stored.into_iter())).expect("settings");
        assert!(!settings.show_explanations && settings.show_overlays);
    }

    /// Settings stored before the language setting existed follow the
    /// system language, as new ones do.
    #[test]
    fn the_language_follows_the_system_by_default_also_for_settings_stored_before() {
        use serde::de::value::{Error, MapDeserializer};

        assert_eq!(Settings::default().language, None);
        let stored = [("show_overlays", false)];
        let settings = Settings::deserialize(MapDeserializer::<_, Error>::new(stored.into_iter())).expect("settings");
        assert_eq!(settings.language, None, "stored before the setting existed: the system language");
        assert!(!settings.show_overlays, "stored fields are kept");
    }

    #[test]
    fn recent_label_contains_no_patient_identifiers() {
        let dir = tempfile::tempdir().expect("tempdir");
        summrise_testkit::write_phantom_study(dir.path(), &summrise_testkit::PhantomOptions::small()).expect("fixture");
        let index = summrise_dicom::scan(dir.path(), &summrise_dicom::Progress::new()).expect("scan");
        let label = recent_label(&index);
        assert_eq!(label, "Synthetic phantom study · 2026-01-15 · CT, MR, OT");
        assert!(!label.contains("PHANTOM^") && !label.contains(summrise_testkit::PATIENT_ID));
    }
}
