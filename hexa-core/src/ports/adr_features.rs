//! What `hexa adr features` is bound by (ADR-2611250845).
//!
//! The rules are the domain's (`domain::adr_features`); the adapter reaches them here.

pub use crate::domain::adr_features::{features_of, sort_features, AdrFeatures, Feature, FeatureState};
