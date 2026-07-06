//! A `Layer` that enables or disables spans and events based on a set of
//! filtering directives.

// these are publicly re-exported, but the compiler doesn't realize
// that for some reason.
pub use self::{builder::Builder, directive::Directive, field::BadName as BadFieldName};
/// Builder for `EnvFilter` values.
mod builder;
/// Directive parsing and matching.
mod directive;
/// Field value parsing and matching.
mod field;

use crate::{
    filter::{LevelFilter, ParseError},
    layer::{Context, Layer},
    RwLock,
};
use alloc::{fmt, str::FromStr, vec::Vec};
use core::cell::RefCell;
use std::{cmp, collections::HashMap, env, error::Error, iter};
use thread_local::ThreadLocal;
use tracing_core::{
    callsite,
    field::Field,
    span,
    subscriber::{Interest, Subscriber, SubscriberResult},
    Metadata,
};

/// A [`Layer`] which filters spans and events based on a set of filter
/// directives.
///
/// `EnvFilter` implements both the [`Layer`](#impl-Layer<S>) and [`Filter`] traits, so it may
/// be used for both [global filtering][global] and [per-layer filtering][plf],
/// respectively. See [the documentation on filtering with `Layer`s][filtering]
/// for details.
///
/// The [`Targets`] type implements a similar form of filtering, but without the
/// ability to dynamically enable events based on the current span context, and
/// without filtering on field values. When these features are not required,
/// [`Targets`] provides a lighter-weight alternative to [`EnvFilter`].
///
/// # Directives
///
/// A filter consists of one or more comma-separated directives which match on [`Span`]s and [`Event`]s.
/// Each directive may have a corresponding maximum verbosity [`level`] which
/// enables (e.g., _selects for_) spans and events that match. Like `log`,
/// `tracing` considers less exclusive levels (like `trace` or `info`) to be more
/// verbose than more exclusive levels (like `error` or `warn`).
///
/// The directive syntax is similar to that of [`env_logger`]'s. At a high level, the syntax for directives
/// consists of several parts:
///
/// ```text
/// target[span{field=value}]=level
/// ```
///
/// Each component (`target`, `span`, `field`, `value`, and `level`) will be covered in turn.
///
/// - `target` matches the event or span's target. In general, this is the module path and/or crate name.
///   Examples of targets `h2`, `tokio::net`, or `tide::server`. For more information on targets,
///   please refer to [`Metadata`]'s documentation.
/// - `span` matches on the span's name. If a `span` directive is provided alongside a `target`,
///   the `span` directive will match on spans _within_ the `target`.
/// - `field` matches on [fields] within spans. Field names can also be supplied without a `value`
///   and will match on any [`Span`] or [`Event`] that has a field with that name.
///   For example: `[span{field=\"value\"}]=debug`, `[{field}]=trace`.
/// - `value` matches on the value of a span's field. If a value is a numeric literal or a bool,
///   it will match _only_ on that value. Otherwise, this filter matches the
///   [`std::fmt::Debug`] output from the value.
/// - `level` sets a maximum verbosity level accepted by this directive.
///
/// When a field value directive (`[{<FIELD NAME>=<FIELD_VALUE>}]=...`) matches a
/// value's [`std::fmt::Debug`] output (i.e., the field value in the directive
/// is not a `bool`, `i64`, `u64`, or `f64` literal), the matched pattern may be
/// interpreted as either a regular expression or as the precise expected
/// output of the field's [`std::fmt::Debug`] implementation. By default, these
/// filters are interpreted as regular expressions, but this can be disabled
/// using the [`Builder::with_regex`] builder method to use precise matching
/// instead.
///
/// When field value filters are interpreted as regular expressions, the
/// [`regex` crate's regular expression syntax][re-syntax] is supported.
///
/// **Note**: When filters are constructed from potentially untrusted inputs,
/// [disabling regular expression matching](Builder::with_regex) is strongly
/// recommended.
///
/// ## Usage Notes
///
/// - The portion of the directive which is included within the square brackets is `tracing`-specific.
/// - Any portion of the directive can be omitted.
///     - The sole exception are the `field` and `value` directives. If a `value` is provided,
///       a `field` must _also_ be provided. However, the converse does not hold, as fields can
///       be matched without a value.
/// - If only a level is provided, it will set the maximum level for all `Span`s and `Event`s
///   that are not enabled by other filters.
/// - A directive without a level will enable anything that it matches. This is equivalent to `=trace`.
/// - When a crate has a dash in its name, the default target for events will be the
///   crate's module path as it appears in Rust. This means every dash will be replaced
///   with an underscore.
/// - A dash in a target will only appear when being specified explicitly:
///   `tracing::info!(target: "target-name", ...);`
///
/// ## Example Syntax
///
/// - `tokio::net=info` will enable all spans or events that:
///    - have the `tokio::net` target,
///    - at the level `info` or above.
/// - `warn,tokio::net=info` will enable all spans and events that:
///    - are at the level `warn` or above, *or*
///    - have the `tokio::net` target at the level `info` or above.
/// - `my_crate[span_a]=trace` will enable all spans and events that:
///    - are within the `span_a` span or named `span_a` _if_ `span_a` has the target `my_crate`,
///    - at the level `trace` or above.
/// - `[span_b{name=\"bob\"}]` will enable all spans or event that:
///    - have _any_ target,
///    - are inside a span named `span_b`,
///    - which has a field named `name` with value `bob`,
///    - at _any_ level.
///
/// # Examples
///
/// Parsing an `EnvFilter` from the [default environment
/// variable](EnvFilter::from_default_env) (`RUST_LOG`):
///
/// ```
/// use tracing_subscriber::{EnvFilter, fmt, prelude::*};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
/// tracing_subscriber::registry()
///     .with(fmt::layer())
///     .with(EnvFilter::from_default_env())
///     .try_init()?;
/// # Ok(()) }
/// ```
///
/// Parsing an `EnvFilter` [from a user-provided environment
/// variable](EnvFilter::from_env):
///
/// ```
/// use tracing_subscriber::{EnvFilter, fmt, prelude::*};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
/// tracing_subscriber::registry()
///     .with(fmt::layer())
///     .with(EnvFilter::from_env("MYAPP_LOG"))
///     .try_init()?;
/// # Ok(()) }
/// ```
///
/// Using `EnvFilter` as a [per-layer filter][plf] to filter only a single
/// [`Layer`]:
///
/// ```
/// use tracing_subscriber::{EnvFilter, fmt, prelude::*};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
/// // Parse an `EnvFilter` configuration from the `RUST_LOG`
/// // environment variable.
/// let filter = EnvFilter::from_default_env();
///
/// // Apply the filter to this layer *only*.
/// let filtered_layer = fmt::layer().with_filter(filter);
///
/// // Some other layer, whose output we don't want to filter.
/// let unfiltered_layer = // ...
///     # fmt::layer();
///
/// tracing_subscriber::registry()
///     .with(filtered_layer)
///     .with(unfiltered_layer)
///     .try_init()?;
/// # Ok(()) }
/// ```
/// # Constructing `EnvFilter`s
///
/// An `EnvFilter` is be constructed by parsing a string containing one or more
/// directives. The [`EnvFilter::new`] constructor parses an `EnvFilter` from a
/// string, ignoring any invalid directives, while [`EnvFilter::try_new`]
/// returns an error if invalid directives are encountered. Similarly, the
/// [`EnvFilter::from_env`] and [`EnvFilter::try_from_env`] constructors parse
/// an `EnvFilter` from the value of the provided environment variable, with
/// lossy and strict validation, respectively.
///
/// A [builder](EnvFilter::builder) interface is available to set additional
/// configuration options prior to parsing an `EnvFilter`. See the [`Builder`
/// type's documentation](Builder) for details on the options that can be
/// configured using the builder.
///
/// [`Span`]: tracing_core::span
/// [fields]: tracing_core::Field
/// [`Event`]: tracing_core::Event
/// [`level`]: tracing_core::Level
/// [`Metadata`]: tracing_core::Metadata
/// [`Targets`]: crate::filter::Targets
/// [`env_logger`]: https://crates.io/crates/env_logger
/// [`Filter`]: #impl-Filter<S>
/// [global]: crate::layer#global-filtering
/// [plf]: crate::layer#per-layer-filtering
/// [filtering]: crate::layer#filtering-with-layers
/// [re-syntax]: https://docs.rs/regex/1.11.1/regex/#syntax
#[cfg_attr(docsrs, doc(cfg(all(feature = "env-filter", feature = "std"))))]
#[derive(Debug)]
pub struct EnvFilter {
    /// Static directives cached by callsite metadata.
    statics: directive::Statics,
    /// Dynamic directives evaluated against span context.
    dynamics: directive::Dynamics,
    /// Whether any dynamic directives are configured.
    has_dynamics: bool,
    /// Dynamic span matchers keyed by span ID.
    by_id: RwLock<HashMap<span::Id, directive::SpanMatcher>>,
    /// Dynamic callsite matchers keyed by callsite identifier.
    by_cs: RwLock<HashMap<callsite::Identifier, directive::CallsiteMatcher>>,
    /// Stack of currently-entered dynamic span levels for each thread.
    scope: ThreadLocal<RefCell<Vec<LevelFilter>>>,
    /// Whether value matchers were parsed as regular expressions.
    regex: bool,
}

/// Creates an [`EnvFilter`] with the same directives as `self`.
///
/// This does *not* clone any of the dynamic state that [`EnvFilter`] acquires while attached to a
/// subscriber.
impl Clone for EnvFilter {
    fn clone(&self) -> Self {
        Self {
            statics: self.statics.clone(),
            dynamics: self.dynamics.clone(),
            has_dynamics: self.has_dynamics,
            by_id: RwLock::default(),
            by_cs: RwLock::default(),
            scope: ThreadLocal::new(),
            regex: self.regex,
        }
    }
}

/// Map from `tracing-core` field identifiers to filter-owned values.
type FieldMap<T> = HashMap<Field, T>;

/// Indicates that an error occurred while parsing a `EnvFilter` from an
/// environment variable.
#[cfg_attr(docsrs, doc(cfg(all(feature = "env-filter", feature = "std"))))]
#[derive(Debug)]
pub struct FromEnvError {
    /// The underlying environment parsing failure.
    kind: ErrorKind,
}

/// The underlying failure kind for environment parsing.
#[derive(Debug)]
enum ErrorKind {
    /// Parsing the environment variable value failed.
    Parse(ParseError),
    /// Reading the environment variable failed.
    Env(env::VarError),
}

impl EnvFilter {
    /// `RUST_LOG` is the default environment variable used by
    /// [`EnvFilter::from_default_env`] and [`EnvFilter::try_from_default_env`].
    ///
    /// [`EnvFilter::from_default_env`]: EnvFilter::from_default_env()
    /// [`EnvFilter::try_from_default_env`]: EnvFilter::try_from_default_env()
    pub const DEFAULT_ENV: &'static str = "RUST_LOG";

    // === constructors, etc ===

    /// Returns a [builder] that can be used to configure a new [`EnvFilter`]
    /// instance.
    ///
    /// The [`Builder`] type is used to set additional configurations, such as
    /// [whether regular expressions are enabled](Builder::with_regex) or [the
    /// default directive](Builder::with_default_directive) before parsing an
    /// [`EnvFilter`] from a string or environment variable.
    ///
    /// [builder]: https://rust-unofficial.github.io/patterns/patterns/creational/builder.html
    pub fn builder() -> Builder {
        Builder::default()
    }

    /// Returns a new `EnvFilter` from the value of the `RUST_LOG` environment
    /// variable, ignoring any invalid filter directives.
    ///
    /// If the environment variable is empty or not set, or if it contains only
    /// invalid directives, a default directive enabling the [`ERROR`] level is
    /// added.
    ///
    /// To set additional configuration options prior to parsing the filter, use
    /// the [`Builder`] type instead.
    ///
    /// This function is equivalent to the following:
    ///
    /// ```rust
    /// use tracing_subscriber::filter::{EnvFilter, LevelFilter};
    ///
    /// # fn docs() -> EnvFilter {
    /// EnvFilter::builder()
    ///     .with_default_directive(LevelFilter::ERROR.into())
    ///     .parse_env_lossy()
    /// # }
    /// ```
    ///
    /// [`ERROR`]: tracing::Level::ERROR
    #[must_use]
    #[allow(
        clippy::single_call_fn,
        reason = "public constructor is part of the documented `EnvFilter` API"
    )]
    pub fn from_default_env() -> Self {
        Self::builder()
            .with_default_directive(LevelFilter::ERROR.into())
            .parse_env_lossy()
    }

    /// Returns a new `EnvFilter` from the value of the given environment
    /// variable, ignoring any invalid filter directives.
    ///
    /// If the environment variable is empty or not set, or if it contains only
    /// invalid directives, a default directive enabling the [`ERROR`] level is
    /// added.
    ///
    /// To set additional configuration options prior to parsing the filter, use
    /// the [`Builder`] type instead.
    ///
    /// This function is equivalent to the following:
    ///
    /// ```rust
    /// use tracing_subscriber::filter::{EnvFilter, LevelFilter};
    ///
    /// # fn docs() -> EnvFilter {
    /// # let env = "";
    /// EnvFilter::builder()
    ///     .with_default_directive(LevelFilter::ERROR.into())
    ///     .with_env_var(env)
    ///     .parse_env_lossy()
    /// # }
    /// ```
    ///
    /// [`ERROR`]: tracing::Level::ERROR
    #[allow(
        clippy::single_call_fn,
        reason = "public constructor is part of the documented `EnvFilter` API"
    )]
    pub fn from_env<A: AsRef<str>>(env: A) -> Self {
        Self::builder()
            .with_default_directive(LevelFilter::ERROR.into())
            .with_env_var(env.as_ref())
            .parse_env_lossy()
    }

    /// Returns a new `EnvFilter` from the directives in the given string,
    /// ignoring any that are invalid.
    ///
    /// If the string is empty or contains only invalid directives, a default
    /// directive enabling the [`ERROR`] level is added.
    ///
    /// To set additional configuration options prior to parsing the filter, use
    /// the [`Builder`] type instead.
    ///
    /// This function is equivalent to the following:
    ///
    /// ```rust
    /// use tracing_subscriber::filter::{EnvFilter, LevelFilter};
    ///
    /// # fn docs() -> EnvFilter {
    /// # let directives = "";
    /// EnvFilter::builder()
    ///     .with_default_directive(LevelFilter::ERROR.into())
    ///     .parse_lossy(directives)
    /// # }
    /// ```
    ///
    /// [`ERROR`]: tracing::Level::ERROR
    #[allow(
        clippy::single_call_fn,
        reason = "public constructor is part of the documented `EnvFilter` API"
    )]
    pub fn new<S: AsRef<str>>(directives: S) -> Self {
        Self::builder()
            .with_default_directive(LevelFilter::ERROR.into())
            .parse_lossy(directives)
    }

    /// Returns a new `EnvFilter` from the directives in the given string,
    /// or an error if any are invalid.
    ///
    /// If the string is empty, a default directive enabling the [`ERROR`] level
    /// is added.
    ///
    /// To set additional configuration options prior to parsing the filter, use
    /// the [`Builder`] type instead.
    ///
    /// This function is equivalent to the following:
    ///
    /// ```rust
    /// use tracing_subscriber::filter::{EnvFilter, LevelFilter};
    ///
    /// # fn docs() -> Result<EnvFilter, tracing_subscriber::filter::ParseError> {
    /// # let directives = "";
    /// EnvFilter::builder()
    ///     .with_default_directive(LevelFilter::ERROR.into())
    ///     .parse(directives)
    /// # }
    /// ```
    ///
    /// [`ERROR`]: tracing::Level::ERROR
    ///
    /// # Errors
    ///
    /// Returns an error if any non-empty directive cannot be parsed.
    #[allow(
        clippy::single_call_fn,
        reason = "public fallible constructor is part of the documented `EnvFilter` API"
    )]
    pub fn try_new<S: AsRef<str>>(dirs: S) -> Result<Self, ParseError> {
        Self::builder().parse(dirs)
    }

    /// Returns a new `EnvFilter` from the value of the `RUST_LOG` environment
    /// variable, or an error if the environment variable is unset or contains
    /// any invalid filter directives.
    ///
    /// To set additional configuration options prior to parsing the filter, use
    /// the [`Builder`] type instead.
    ///
    /// This function is equivalent to the following:
    ///
    /// ```rust
    /// use tracing_subscriber::EnvFilter;
    ///
    /// # fn docs() -> Result<EnvFilter, tracing_subscriber::filter::FromEnvError> {
    /// EnvFilter::builder().try_parse_env()
    /// # }
    /// ```
    /// # Errors
    ///
    /// Returns an error if `RUST_LOG` is unset or contains invalid directives.
    pub fn try_from_default_env() -> Result<Self, FromEnvError> {
        Self::builder().try_parse_env()
    }

    /// Returns a new `EnvFilter` from the value of the given environment
    /// variable, or an error if the environment variable is unset or contains
    /// any invalid filter directives.
    ///
    /// To set additional configuration options prior to parsing the filter, use
    /// the [`Builder`] type instead.
    ///
    /// This function is equivalent to the following:
    ///
    /// ```rust
    /// use tracing_subscriber::EnvFilter;
    ///
    /// # fn docs() -> Result<EnvFilter, tracing_subscriber::filter::FromEnvError> {
    /// # let env = "";
    /// EnvFilter::builder().with_env_var(env).try_parse_env()
    /// # }
/// ```
    /// # Errors
    ///
    /// Returns an error if the provided environment variable is unset or
    /// contains invalid directives.
    pub fn try_from_env<A: AsRef<str>>(env: A) -> Result<Self, FromEnvError> {
        Self::builder().with_env_var(env.as_ref()).try_parse_env()
    }

    /// Add a filtering directive to this `EnvFilter`.
    ///
    /// The added directive will be used in addition to any previously set
    /// directives, either added using this method or provided when the filter
    /// is constructed.
    ///
    /// Filters may be created from [`LevelFilter`] or [`Level`], which will
    /// enable all traces at or below a certain verbosity level, or
    /// parsed from a string specifying a directive.
    ///
    /// If a filter directive is inserted that matches exactly the same spans
    /// and events as a previous filter, but sets a different level for those
    /// spans and events, the previous directive is overwritten.
    ///
    /// [`LevelFilter`]: super::LevelFilter
    /// [`Level`]: tracing_core::Level
    ///
    /// # Examples
    ///
    /// From [`LevelFilter`]:
    ///
    /// ```rust
    /// use tracing_subscriber::filter::{EnvFilter, LevelFilter};
    /// let mut filter = EnvFilter::from_default_env()
    ///     .add_directive(LevelFilter::INFO.into());
    /// ```
    ///
    /// Or from [`Level`]:
    ///
    /// ```rust
    /// # use tracing_subscriber::filter::{EnvFilter, LevelFilter};
    /// # use tracing::Level;
    /// let mut filter = EnvFilter::from_default_env()
    ///     .add_directive(Level::INFO.into());
    /// ```
    ///
    /// Parsed from a string:
    ///
    /// ```rust
    /// use tracing_subscriber::filter::{EnvFilter, Directive};
    ///
    /// # fn try_mk_filter() -> Result<(), Box<dyn ::std::error::Error>> {
    /// let mut filter = EnvFilter::try_from_default_env()?
    ///     .add_directive("my_crate::module=trace".parse()?)
    ///     .add_directive("my_crate::my_other_module::something=info".parse()?);
    /// # Ok(())
    /// # }
    /// ```
    /// In the above example, substitute `my_crate`, `module`, etc. with the
    /// name your target crate/module is imported with. This might be
    /// different from the package name in Cargo.toml (`-` is replaced by `_`).
    /// Example, if the package name in your Cargo.toml is `MY-FANCY-LIB`, then
    /// the corresponding Rust identifier would be `MY_FANCY_LIB`:
    #[must_use]
    pub fn add_directive(mut self, mut directive: Directive) -> Self {
        if !self.regex {
            directive.deregexify();
        }
        if let Some(stat) = directive.to_static() {
            self.statics.add(stat);
        } else {
            self.has_dynamics = true;
            self.dynamics.add(directive);
        }
        self
    }

    // === filtering methods ===

    /// Returns `true` if this `EnvFilter` would enable the provided `metadata`
    /// in the current context.
    ///
    /// This is equivalent to calling the [`Layer::enabled`] or
    /// [`Filter::enabled`] methods on `EnvFilter`'s implementations of those
    /// traits, but it does not require the trait to be in scope.
    fn enabled_for<S>(&self, metadata: &Metadata<'_>, _: Context<'_, S>) -> bool {
        let level = metadata.level();

        // is it possible for a dynamic filter directive to enable this event?
        // if not, we can avoid the thread local access + iterating over the
        // spans in the current scope.
        if self.has_dynamics && self.dynamics.max_level >= *level {
            let enabled_by_callsite = metadata.is_span()
                && try_lock!(self.by_cs.read(), else false).contains_key(&metadata.callsite());
            if enabled_by_callsite {
                return true;
            }

            let Ok(scope) = self.scope.get_or_default().try_borrow() else {
                return false;
            };
            if scope.iter().any(|filter| filter >= level) {
                return true;
            }
        }

        // is it possible for a static filter directive to enable this event?
        if self.statics.max_level >= *level {
            // Otherwise, fall back to checking if the callsite is
            // statically enabled.
            return self.statics.enabled(metadata);
        }

        false
    }

    /// Returns an optional hint of the highest [verbosity level][level] that
    /// this `EnvFilter` will enable.
    ///
    /// This is equivalent to calling the [`Layer::max_level_hint`] or
    /// [`Filter::max_level_hint`] methods on `EnvFilter`'s implementations of those
    /// traits, but it does not require the trait to be in scope.
    ///
    /// [level]: tracing_core::metadata::Level
    fn max_level_hint_for_filter(&self) -> Option<LevelFilter> {
        if self.dynamics.has_value_filters() {
            // If we perform any filtering on span field *values*, we will
            // enable *all* spans, because their field values are not known
            // until recording.
            return Some(LevelFilter::TRACE);
        }
        cmp::max(self.statics.max_level.into(), self.dynamics.max_level.into())
    }

    /// Informs the filter that a new span was created.
    ///
    /// This is equivalent to calling the [`Layer::on_new_span`] or
    /// [`Filter::on_new_span`] methods on `EnvFilter`'s implementations of those
    /// traits, but it does not require the trait to be in scope.
    fn observe_new_span<S>(&self, attrs: &span::Attributes<'_>, id: span::Id, _: Context<'_, S>) {
        if !self.has_dynamics {
            return;
        }
        let by_cs = try_lock!(self.by_cs.read());
        if let Some(cs) = by_cs.get(&attrs.metadata().callsite()) {
            let span = cs.to_span_match(attrs);
            let _previous = try_lock!(self.by_id.write()).insert(id, span);
        }
    }

    /// Informs the filter that the span with the provided `id` was entered.
    ///
    /// This is equivalent to calling the [`Layer::on_enter`] or
    /// [`Filter::on_enter`] methods on `EnvFilter`'s implementations of those
    /// traits, but it does not require the trait to be in scope.
    fn observe_enter<S>(&self, id: span::Id, _: Context<'_, S>) {
        if !self.has_dynamics {
            return;
        }
        // XXX: This is where _we_ could push IDs to the stack instead, and use
        // that to allow changing the filter while a span is already entered.
        // But that might be much less efficient...
        if let Some(level) = self.span_level(id)
            && let Ok(mut scope) = self.scope.get_or_default().try_borrow_mut()
        {
            scope.push(level);
        }
    }

    /// Informs the filter that the span with the provided `id` was exited.
    ///
    /// This is equivalent to calling the [`Layer::on_exit`] or
    /// [`Filter::on_exit`] methods on `EnvFilter`'s implementations of those
    /// traits, but it does not require the trait to be in scope.
    fn observe_exit<S>(&self, id: span::Id, _: Context<'_, S>) {
        if !self.has_dynamics {
            return;
        }
        if self.cares_about_span(id)
            && let Ok(mut scope) = self.scope.get_or_default().try_borrow_mut()
        {
            let _exited = scope.pop();
        }
    }

    /// Informs the filter that the span with the provided `id` was closed.
    ///
    /// This is equivalent to calling the [`Layer::on_close`] or
    /// [`Filter::on_close`] methods on `EnvFilter`'s implementations of those
    /// traits, but it does not require the trait to be in scope.
    fn observe_close<S>(&self, id: span::Id, _: Context<'_, S>) {
        if !self.has_dynamics {
            return;
        }
        self.remove_span(id);
    }

    /// Informs the filter that the span with the provided `id` recorded the
    /// provided field `values`.
    ///
    /// This is equivalent to calling the [`Layer::on_record`] or
    /// [`Filter::on_record`] methods on `EnvFilter`'s implementations of those
    /// traits, but it does not require the trait to be in scope
    fn observe_record<S>(&self, id: span::Id, values: &span::Record<'_>, _: Context<'_, S>) {
        if !self.has_dynamics {
            return;
        }
        self.record_span(id, values);
    }

    /// Returns whether the dynamic matcher map contains the span.
    fn cares_about_span(&self, id: span::Id) -> bool {
        let spans = try_lock!(self.by_id.read(), else return false);
        spans.contains_key(&id)
    }

    /// Returns the dynamic level currently enabled by a span.
    fn span_level(&self, id: span::Id) -> Option<LevelFilter> {
        let spans = try_lock!(self.by_id.read(), else return None);
        spans.get(&id).map(directive::SpanMatcher::level)
    }

    /// Records span fields into a dynamic matcher.
    fn record_span(&self, id: span::Id, values: &span::Record<'_>) {
        let spans = try_lock!(self.by_id.read());
        if let Some(span_matcher) = spans.get(&id) {
            span_matcher.record_update(values);
        }
    }

    /// Removes dynamic state for a closed span.
    fn remove_span(&self, id: span::Id) {
        // If we don't need to acquire a write lock, avoid doing so.
        if !self.cares_about_span(id) {
            return;
        }

        let mut spans = try_lock!(self.by_id.write());
        let _removed = spans.remove(&id);
    }

    /// Returns the base interest used when dynamic directives are present.
    const fn base_interest(&self) -> Interest {
        if self.has_dynamics {
            Interest::sometimes()
        } else {
            Interest::never()
        }
    }

    /// Returns the interest determined by static directives, or `disabled` when none match.
    fn static_interest_or(&self, metadata: &Metadata<'_>, disabled: Interest) -> Interest {
        if self.statics.enabled(metadata) {
            Interest::always()
        } else {
            disabled
        }
    }

    /// Registers a callsite with this filter's dynamic and static tables.
    fn register_callsite_for_filter(&self, metadata: &'static Metadata<'static>) -> Interest {
        if self.has_dynamics && metadata.is_span() {
            // If this metadata describes a span, first, check if there is a
            // dynamic filter that should be constructed for it. If so, it
            // should always be enabled, since it influences filtering.
            match self.dynamics.matcher(metadata) {
                directive::CallsiteMatchResult::Matched(matcher) => {
                    let mut by_cs =
                        try_lock!(self.by_cs.write(), else return self.base_interest());
                    let _previous = by_cs.insert(metadata.callsite(), *matcher);
                    drop(by_cs);
                    return Interest::always();
                }
                directive::CallsiteMatchResult::Rejected => {
                    return self.static_interest_or(metadata, Interest::never());
                }
                directive::CallsiteMatchResult::Unmatched => {}
            }
        }

        // Otherwise, check if any of our static filters enable this metadata.
        self.static_interest_or(metadata, self.base_interest())
    }
}

impl<S: Subscriber> Layer<S> for EnvFilter {
    #[inline]
    fn register_callsite(
        &self,
        metadata: &'static Metadata<'static>,
    ) -> SubscriberResult<Interest> {
        Ok(self.register_callsite_for_filter(metadata))
    }

    #[inline]
    fn max_level_hint(&self) -> SubscriberResult<Option<LevelFilter>> {
        Ok(self.max_level_hint_for_filter())
    }

    #[inline]
    fn enabled(&self, metadata: &Metadata<'_>, ctx: Context<'_, S>) -> SubscriberResult<bool> {
        Ok(self.enabled_for(metadata, ctx))
    }

    #[inline]
    fn on_new_span(
        &self,
        attrs: &span::Attributes<'_>,
        id: span::Id,
        ctx: Context<'_, S>,
    ) -> SubscriberResult<()> {
        self.observe_new_span(attrs, id, ctx);
        Ok(())
    }

    #[inline]
    fn on_record(
        &self,
        id: span::Id,
        values: &span::Record<'_>,
        ctx: Context<'_, S>,
    ) -> SubscriberResult<()> {
        self.observe_record(id, values, ctx);
        Ok(())
    }

    #[inline]
    fn on_enter(&self, id: span::Id, ctx: Context<'_, S>) -> SubscriberResult<()> {
        self.observe_enter(id, ctx);
        Ok(())
    }

    #[inline]
    fn on_exit(&self, id: span::Id, ctx: Context<'_, S>) -> SubscriberResult<()> {
        self.observe_exit(id, ctx);
        Ok(())
    }

    #[inline]
    fn on_close(&self, id: span::Id, ctx: Context<'_, S>) -> SubscriberResult<()> {
        self.observe_close(id, ctx);
        Ok(())
    }
}

feature! {
    #![all(feature = "registry", feature = "std")]
    use crate::layer::Filter;

    impl<S> Filter<S> for EnvFilter {
        #[inline]
        fn enabled(&self, meta: &Metadata<'_>, ctx: &Context<'_, S>) -> SubscriberResult<bool> {
            Ok(self.enabled_for(meta, ctx.clone()))
        }

        #[inline]
        fn callsite_enabled(
            &self,
            meta: &'static Metadata<'static>,
        ) -> SubscriberResult<Interest> {
            Ok(self.register_callsite_for_filter(meta))
        }

        #[inline]
        fn max_level_hint(&self) -> SubscriberResult<Option<LevelFilter>> {
            Ok(self.max_level_hint_for_filter())
        }

        #[inline]
        fn on_new_span(
            &self,
            attrs: &span::Attributes<'_>,
            id: span::Id,
            ctx: Context<'_, S>,
        ) -> SubscriberResult<()> {
            self.observe_new_span(attrs, id, ctx);
            Ok(())
        }

        #[inline]
        fn on_record(
            &self,
            id: span::Id,
            values: &span::Record<'_>,
            ctx: Context<'_, S>,
        ) -> SubscriberResult<()> {
            self.observe_record(id, values, ctx);
            Ok(())
        }

        #[inline]
        fn on_enter(&self, id: span::Id, ctx: Context<'_, S>) -> SubscriberResult<()> {
            self.observe_enter(id, ctx);
            Ok(())
        }

        #[inline]
        fn on_exit(&self, id: span::Id, ctx: Context<'_, S>) -> SubscriberResult<()> {
            self.observe_exit(id, ctx);
            Ok(())
        }

        #[inline]
        fn on_close(&self, id: span::Id, ctx: Context<'_, S>) -> SubscriberResult<()> {
            self.observe_close(id, ctx);
            Ok(())
        }
    }
}

impl FromStr for EnvFilter {
    type Err = ParseError;

    fn from_str(spec: &str) -> Result<Self, Self::Err> {
        Self::try_new(spec)
    }
}

impl<Source> From<Source> for EnvFilter
where
    Source: AsRef<str>,
{
    fn from(source: Source) -> Self {
        Self::new(source)
    }
}

impl Default for EnvFilter {
    fn default() -> Self {
        Builder::default().build_from_directives(iter::empty())
    }
}

impl fmt::Display for EnvFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut statics = self.statics.iter();
        let wrote_statics = if let Some(next) = statics.next() {
            fmt::Display::fmt(next, f)?;
            for directive in statics {
                write!(f, ",{directive}")?;
            }
            true
        } else {
            false
        };

        let mut dynamics = self.dynamics.iter();
        if let Some(next) = dynamics.next() {
            if wrote_statics {
                f.write_str(",")?;
            }
            fmt::Display::fmt(next, f)?;
            for directive in dynamics {
                write!(f, ",{directive}")?;
            }
        }
        Ok(())
    }
}

// ===== impl FromEnvError =====

impl From<ParseError> for FromEnvError {
    fn from(parse_error: ParseError) -> Self {
        Self {
            kind: ErrorKind::Parse(parse_error),
        }
    }
}

impl From<env::VarError> for FromEnvError {
    fn from(var_error: env::VarError) -> Self {
        Self {
            kind: ErrorKind::Env(var_error),
        }
    }
}

impl fmt::Display for FromEnvError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            ErrorKind::Parse(ref parse_error) => parse_error.fmt(formatter),
            ErrorKind::Env(ref var_error) => var_error.fmt(formatter),
        }
    }
}

impl Error for FromEnvError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self.kind {
            ErrorKind::Parse(ref parse_error) => Some(parse_error),
            ErrorKind::Env(ref var_error) => Some(var_error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{format, string::ToString as _};
    use core::mem::size_of_val;
    use core::num::NonZeroU64;
    use strict_test_support::{TestFailure, ensure, ensure_eq, ensure_ok, ensure_some};
    use tracing_core::field::FieldSet;
    use tracing_core::metadata::SourceLocation;
    use tracing_core::*;

    const NO_SUBSCRIBER_SPAN_ID: span::Id = match span::Id::try_from_u64(0xDEAD) {
        Some(id) => id,
        None => span::Id::from_non_zero_u64(NonZeroU64::MIN),
    };

    struct NoSubscriber;
    impl Subscriber for NoSubscriber {
        #[inline]
        fn register_callsite(&self, _: &'static Metadata<'static>) -> SubscriberResult<Interest> {
            Ok(Interest::always())
        }
        fn new_span(&self, _: &span::Attributes<'_>) -> SubscriberResult<span::Id> {
            Ok(NO_SUBSCRIBER_SPAN_ID)
        }
        fn event(&self, _event: &Event<'_>) -> SubscriberResult {
            Ok(())
        }
        fn record(&self, _span: span::Id, _values: &span::Record<'_>) -> SubscriberResult {
            Ok(())
        }
        fn record_follows_from(&self, _span: span::Id, _follows: span::Id) -> SubscriberResult {
            Ok(())
        }

        #[inline]
        fn enabled(&self, _metadata: &Metadata<'_>) -> SubscriberResult<bool> {
            Ok(true)
        }
        fn enter(&self, _span: span::Id) -> SubscriberResult {
            Ok(())
        }
        fn exit(&self, _span: span::Id) -> SubscriberResult {
            Ok(())
        }
    }

    struct Cs;
    impl Callsite for Cs {
        fn set_interest(&self, _interest: Interest) {}
        fn metadata(&self) -> &Metadata<'_> {
            static META: Metadata<'static> = Metadata::new(
                "test",
                "test",
                Level::TRACE,
                &SourceLocation::empty(),
                &FieldSet::new(&[], identify_callsite!(&Cs)),
                Kind::SPAN,
            );
            &META
        }
    }

    /// Static span metadata at `TRACE` with no fields, shared by the callsite tests.
    static SPAN_META_TRACE: Metadata<'static> = Metadata::new(
        "mySpan",
        "app",
        Level::TRACE,
        &SourceLocation::empty(),
        &FieldSet::new(&[], identify_callsite!(&Cs)),
        Kind::SPAN,
    );

    /// Static span metadata at `ERROR` with no fields for the off-directive test.
    static SPAN_META_ERROR: Metadata<'static> = Metadata::new(
        "mySpan",
        "app",
        Level::ERROR,
        &SourceLocation::empty(),
        &FieldSet::new(&[], identify_callsite!(&Cs)),
        Kind::SPAN,
    );

    /// Static span metadata at `TRACE` carrying one field for the field-directive tests.
    static SPAN_META_TRACE_FIELD: Metadata<'static> = Metadata::new(
        "mySpan",
        "app",
        Level::TRACE,
        &SourceLocation::empty(),
        &FieldSet::new(&["field"], identify_callsite!(&Cs)),
        Kind::SPAN,
    );

    /// Registers `metadata` against a filter built from `directive` and checks the interest verdict.
    fn ensure_callsite_interest(
        metadata: &'static Metadata<'static>,
        directive: &str,
        expect_enabled: bool,
        message: &'static str,
    ) -> Result<(), TestFailure> {
        let filter = EnvFilter::new(directive).with_subscriber(NoSubscriber);
        let interest = ensure_ok(
            filter.register_callsite(metadata),
            "callsite registration succeeds",
        )?;
        let enabled = if expect_enabled {
            interest.is_always()
        } else {
            interest.is_never()
        };
        ensure(enabled, message)
    }

    #[test]
    fn callsite_enabled_no_span_directive() -> Result<(), TestFailure> {
        ensure_callsite_interest(
            &SPAN_META_TRACE,
            "app=debug",
            false,
            "span directive should not enable callsite",
        )
    }

    #[test]
    fn callsite_off() -> Result<(), TestFailure> {
        ensure_callsite_interest(
            &SPAN_META_ERROR,
            "app=off",
            false,
            "off directive should disable callsite",
        )
    }

    #[test]
    fn callsite_enabled_includes_span_directive() -> Result<(), TestFailure> {
        ensure_callsite_interest(
            &SPAN_META_TRACE,
            "app[mySpan]=debug",
            true,
            "matching span directive should enable callsite",
        )
    }

    #[test]
    fn callsite_enabled_includes_span_directive_field() -> Result<(), TestFailure> {
        ensure_callsite_interest(
            &SPAN_META_TRACE_FIELD,
            "app[mySpan{field=\"value\"}]=debug",
            true,
            "matching span field directive should enable callsite",
        )
    }

    #[test]
    fn callsite_enabled_includes_span_directive_multiple_fields() -> Result<(), TestFailure> {
        ensure_callsite_interest(
            &SPAN_META_TRACE_FIELD,
            "app[mySpan{field=\"value\",field2=2}]=debug",
            false,
            "multi-field directive should not enable single-field callsite",
        )
    }

    #[test]
    fn roundtrip() -> Result<(), TestFailure> {
        let first_filter: EnvFilter = ensure_ok(
            "[span1{foo=1}]=error,[span2{bar=2 baz=false}],crate2[{quux=\"quuux\"}]=debug"
                .parse(),
            "source env filter should parse",
        )?;
        let second_filter: EnvFilter = ensure_ok(
            format!("{first_filter}").parse(),
            "formatted env filter should parse",
        )?;
        ensure(
            first_filter.statics == second_filter.statics,
            "static directives roundtrip",
        )?;
        ensure(
            first_filter.dynamics == second_filter.dynamics,
            "dynamic directives roundtrip",
        )
    }

    #[test]
    fn size_of_filters() -> Result<(), TestFailure> {
        fn ensure_filter_has_size(source: &str) -> Result<(), TestFailure> {
            let filter = ensure_ok(source.parse::<EnvFilter>(), "filter should parse")?;
            ensure(size_of_val(&filter) > 0, "parsed filter has a positive size")
        }

        ensure_filter_has_size("info")?;

        ensure_filter_has_size("foo=debug")?;

        ensure_filter_has_size(
            "crate1::mod1=error,crate1::mod2=warn,crate1::mod2::mod3=info,\
            crate2=debug,crate3=trace,crate3::mod2::mod1=off",
        )?;

        ensure_filter_has_size(
            "[span1{foo=1}]=error,[span2{bar=2 baz=false}],crate2[{quux=\"quuux\"}]=debug",
        )?;

        ensure_filter_has_size(
            "crate1::mod1=error,crate1::mod2=warn,crate1::mod2::mod3=info,\
            crate2=debug,crate3=trace,crate3::mod2::mod1=off,[span1{foo=1}]=error,\
            [span2{bar=2 baz=false}],crate2[{quux=\"quuux\"}]=debug",
        )
    }

    #[test]
    fn parse_empty_string() -> Result<(), TestFailure> {
        // There is no corresponding test for [`Builder::parse_lossy`] as failed
        // parsing does not produce any observable side effects. If this test fails
        // check that [`Builder::parse_lossy`] is behaving correctly as well.
        ensure(
            EnvFilter::builder().parse("").is_ok(),
            "empty env filter should parse",
        )
    }

    #[test]
    fn constructors_apply_lossy_and_strict_parse_contracts() -> Result<(), TestFailure> {
        let lossy = EnvFilter::new("app=info,broken[");
        ensure_eq(
            &lossy.to_string().as_str(),
            &"app=info",
            "lossy constructor drops invalid directives and keeps valid ones",
        )?;

        let strict_error = ensure_some(
            EnvFilter::try_new("app=info,broken[").err(),
            "strict constructor rejects invalid directives",
        )?;
        let strict_message = strict_error.to_string();
        ensure(
            strict_message.contains("invalid filter directive"),
            "strict constructor reports invalid directive category",
        )?;

        let from_str = ensure_ok(
            "app=warn".parse::<EnvFilter>(),
            "FromStr delegates to strict parsing",
        )?;
        ensure_eq(
            &from_str.to_string().as_str(),
            &"app=warn",
            "FromStr preserves strict directive text",
        )?;

        let default_filter = EnvFilter::default();
        ensure_eq(
            &default_filter.to_string().as_str(),
            &"",
            "default filter has no directives",
        )
    }

    #[test]
    fn default_directive_is_used_when_lossy_input_has_no_valid_directives() -> Result<(), TestFailure> {
        let defaulted = EnvFilter::new("broken[");

        ensure_eq(
            &defaulted.to_string().as_str(),
            &"error",
            "lossy constructor falls back to the error directive",
        )
    }

    #[test]
    fn from_env_errors_preserve_parse_and_environment_sources() -> Result<(), TestFailure> {
        let parse_error = ensure_some(
            EnvFilter::try_new("broken[").err(),
            "parse error is available",
        )?;
        let parse_env_error = FromEnvError::from(parse_error);
        let parse_display = parse_env_error.to_string();
        ensure(
            parse_display.contains("invalid filter directive"),
            "parse-backed environment error displays parse message",
        )?;
        ensure(
            parse_env_error.source().is_some(),
            "parse-backed environment error exposes source",
        )?;

        let missing_env_error = FromEnvError::from(env::VarError::NotPresent);
        ensure_eq(
            &missing_env_error.to_string().as_str(),
            &"environment variable not found",
            "environment-backed error displays variable failure",
        )?;
        ensure(
            missing_env_error.source().is_some(),
            "environment-backed error exposes source",
        )
    }

    #[test]
    fn static_dynamic_and_value_directives_report_level_hints() -> Result<(), TestFailure> {
        let static_filter = EnvFilter::new("app=info");
        ensure(
            static_filter.base_interest().is_never(),
            "static-only filters have no dynamic base interest",
        )?;
        ensure(
            static_filter.max_level_hint_for_filter() == Some(LevelFilter::INFO),
            "static filter reports its maximum level hint",
        )?;

        let dynamic_filter = EnvFilter::new("[mySpan]=debug");
        ensure(
            dynamic_filter.base_interest().is_sometimes(),
            "dynamic filters use sometimes interest as the base",
        )?;
        ensure(
            dynamic_filter.max_level_hint_for_filter() == Some(LevelFilter::DEBUG),
            "dynamic filter reports its maximum level hint",
        )?;

        let value_filter = EnvFilter::new("[mySpan{field=\"value\"}]=debug");
        ensure(
            value_filter.max_level_hint_for_filter() == Some(LevelFilter::TRACE),
            "value filters report trace because span values are known after registration",
        )
    }
}
