//! What `hexa adr features` is bound by (ADR-2611250845).
//!
//! The rules are the domain's (`domain::adr_features`); the adapter reaches them here.

pub use crate::domain::adr_features::{
    built_commits, draft_rows, features_of, has_features_section, insert_section, lowest_version, render_section,
    sort_features, summarize_features, AdrFeatures, Feature, FeatureRow, FeatureState, FeatureSummary,
};
