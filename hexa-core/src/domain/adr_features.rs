//! The features an ADR names, and where each stands (ADR-2611250845).
//!
//! A feature's state is the board's reading of its ADR (`adr_board::analyze`),
//! never a second opinion: this module reads only the `## Features` section
//! itself. Everything here is a pure function of the ADR's text.

use serde::Serialize;

use super::adr_board::Units;

/// Where a feature stands (decision 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FeatureState {
    /// What builds the feature is built.
    Completed,
    /// The ADR is accepted; what builds the feature is not built, or only part of it.
    Accepted,
    /// The ADR is proposed; what builds the feature is not built.
    Proposed,
    /// The ADR is superseded, rejected, deprecated or abandoned.
    Retired,
    /// The ADR's status could not be read, or the row names a phase the ADR does not have.
    Unknown,
}

impl FeatureState {
    pub fn name(self) -> &'static str {
        match self {
            FeatureState::Completed => "completed",
            FeatureState::Accepted => "accepted",
            FeatureState::Proposed => "proposed",
            FeatureState::Retired => "retired",
            FeatureState::Unknown => "unknown",
        }
    }
}

/// One row of the table: a feature, as one ADR gives it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Feature {
    /// The feature's name, as a person would say it.
    pub feature: String,
    /// One sentence, or empty when the row gives none.
    pub what: String,
    pub state: FeatureState,
    /// The ADR's id, `ADR-2611250845`.
    pub adr: String,
    pub adr_title: String,
    /// The phases of the ADR that build the feature; empty means the whole ADR.
    pub phases: Vec<String>,
    /// How much of what builds the feature is built. `phases` inside is empty: the ids are in `Feature::phases`.
    pub built: Units,
    /// False when the ADR has no `## Features` section and its title stands in (decision 3).
    pub declared: bool,
    /// `unknown-phase`, `duplicate-feature` (decision 8).
    pub warnings: Vec<String>,
}

/// What one ADR gives.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AdrFeatures {
    /// True when the ADR has a `## Features` section, with or without rows.
    pub declared: bool,
    pub features: Vec<Feature>,
    /// Findings about the section as a whole: `no-feature-column`, `empty-feature` (decision 8).
    pub warnings: Vec<String>,
}

/// Read one ADR. `id` and `title` are the ADR's own; `status` is the
/// normalised status word the adapter already parsed, as `adr_board::analyze`
/// takes it; `content` is the whole file.
pub fn features_of(id: &str, title: &str, status: &str, content: &str) -> AdrFeatures {
    let _ = (id, title, status, content);
    unimplemented!("ADR-2611250845 F1: written after its sealed tests")
}

/// The order of the table: by state as `FeatureState` orders them, then by
/// feature name without regard to case, then by ADR id (decision 5).
pub fn sort_features(rows: &mut [Feature]) {
    let _ = rows;
    unimplemented!("ADR-2611250845 F1: written after its sealed tests")
}
