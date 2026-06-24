#![doc = include_str!("../README.md")]
#![cfg_attr(
    docsrs,
    // Allows displaying cfgs/feature flags in the documentation.
    feature(doc_cfg),
    // Fail the docs build if any intra-docs links are broken
    deny(rustdoc::broken_intra_doc_links),
)]
#![doc(
    html_logo_url = "https://raw.githubusercontent.com/tokio-rs/tracing/main/assets/logo-type.png",
    html_favicon_url = "https://raw.githubusercontent.com/tokio-rs/tracing/main/assets/favicon.ico",
    issue_tracker_base_url = "https://github.com/strict-rs/strict-tracing/issues/"
)]
pub mod ancestry;
pub mod event;
pub mod expect;
pub mod field;
mod metadata;
pub mod span;
pub mod subscriber;

#[cfg(feature = "tracing-subscriber")]
pub mod layer;
