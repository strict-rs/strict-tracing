//! Internal implementation engine for the `tracing-attributes` `#[instrument]` macro.
//!
//! This crate is an ordinary library (not a `proc-macro` crate) that houses the argument parser
//! ([`attr`]), the code generator ([`expand`]), and the parse-and-expand entry pipeline
//! ([`entry`]) behind the `#[instrument]` attribute. The published `tracing-attributes` crate is a
//! thin `proc-macro` shim whose `#[proc_macro_attribute]` entry point forwards to [`instrument`],
//! which takes and returns [`proc_macro2::TokenStream`] values.
//!
//! Housing the implementation outside the `proc-macro` crate lets these modules export ordinary
//! `pub` items across module boundaries — something a `proc-macro` crate cannot do, because it may
//! only export its macro entry points.

pub mod attr;
pub mod entry;
pub mod expand;

pub use crate::entry::instrument;
