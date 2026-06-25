//! A [filter] that enables or disables spans and events based on their [target] and [level].
//!
//! See [`Targets`] for details.
//!
//! [target]: tracing_core::Metadata::target
//! [level]: tracing_core::Level
//! [filter]: crate::layer#filtering-with-layers

use crate::{
    filter::{
        directive::{DirectiveSet, ParseError, StaticDirective},
        LevelFilter,
    },
    layer,
};
use alloc::{string::String, vec::Vec};
use core::{
    fmt,
    iter::{Extend, FilterMap, FromIterator},
    slice,
    str::FromStr,
};
use tracing_core::{
    Level, Metadata,
    subscriber::{Interest, Subscriber, SubscriberResult},
};

/// A filter that enables or disables spans and events based on their [target]
/// and [level].
///
/// Targets are typically equal to the Rust module path of the code where the
/// span or event was recorded, although they may be overridden.
///
/// This type can be used for both [per-layer filtering][plf] (using its
/// [`Filter`] implementation) and [global filtering][global] (using its
/// [`Layer`] implementation).
///
/// See the [documentation on filtering with layers][filtering] for details.
///
/// # Filtering With `Targets`
///
/// A `Targets` filter consists of one or more [target] prefixes, paired with
/// [`LevelFilter`]s. If a span or event's [target] begins with one of those
/// prefixes, and its [level] is at or below the [`LevelFilter`] enabled for
/// that prefix, then the span or event will be enabled.
///
/// This is similar to the behavior implemented by the [`env_logger` crate] in
/// the `log` ecosystem.
///
/// The [`EnvFilter`] type also provided by this crate is very similar to `Targets`,
/// but is capable of a more sophisticated form of filtering where events may
/// also be enabled or disabled based on the span they are recorded in.
/// `Targets` can be thought of as a lighter-weight form of [`EnvFilter`] that
/// can be used instead when this dynamic filtering is not required.
///
/// # Examples
///
/// A `Targets` filter can be constructed by programmatically adding targets and
/// levels to enable:
///
/// ```
/// use tracing_subscriber::{filter, prelude::*};
/// use tracing_core::Level;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
/// let filter = filter::Targets::new()
///     // Enable the `INFO` level for anything in `my_crate`
///     .with_target("my_crate", Level::INFO)
///     // Enable the `DEBUG` level for a specific module.
///     .with_target("my_crate::interesting_module", Level::DEBUG);
///
/// // Build a new subscriber with the `fmt` layer using the `Targets`
/// // filter we constructed above.
/// tracing_subscriber::registry()
///     .with(tracing_subscriber::fmt::layer())
///     .with(filter)
///     .try_init()?;
/// # Ok(()) }
/// ```
///
/// [`LevelFilter::OFF`] can be used to disable a particular target:
/// ```
/// use tracing_subscriber::filter::{Targets, LevelFilter};
/// use tracing_core::Level;
///
/// let filter = Targets::new()
///     .with_target("my_crate", Level::INFO)
///     // Disable all traces from `annoying_module`.
///     .with_target("my_crate::annoying_module", LevelFilter::OFF);
/// # drop(filter);
/// ```
///
/// Alternatively, `Targets` implements [`std::str::FromStr`], allowing it to be
/// parsed from a comma-delimited list of `target=level` pairs. For example:
///
/// ```rust
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// use tracing_subscriber::filter;
/// use tracing_core::Level;
///
/// let filter = "my_crate=info,my_crate::interesting_module=trace,other_crate=debug"
///     .parse::<filter::Targets>()?;
///
/// // The parsed filter is identical to a filter constructed using `with_target`:
/// let expected = filter::Targets::new()
///     .with_target("my_crate", Level::INFO)
///     .with_target("my_crate::interesting_module", Level::TRACE)
///     .with_target("other_crate", Level::DEBUG);
/// if filter != expected {
///     return Err("parsed targets should match the builder form".into());
/// }
/// # Ok(()) }
/// ```
///
/// This is particularly useful when the list of enabled targets is configurable
/// by the user at runtime.
///
/// The `Targets` filter can be used as a [per-layer filter][plf] *and* as a
/// [global filter][global]:
///
/// ```rust
/// use tracing_subscriber::{
///     fmt,
///     filter::{Targets, LevelFilter},
///     prelude::*,
/// };
/// use tracing_core::Level;
/// use std::{sync::Arc, fs::File};
/// # fn docs() -> Result<(), Box<dyn std::error::Error>> {
///
/// // A layer that logs events to stdout using the human-readable "pretty"
/// // format.
/// let stdout_log = fmt::layer().pretty();
///
/// // A layer that logs events to a file, using the JSON format.
/// let file = File::create("debug_log.json")?;
/// let debug_log = fmt::layer()
///     .with_writer(Arc::new(file))
///     .json();
///
/// tracing_subscriber::registry()
///     // Only log INFO and above to stdout, unless the span or event
///     // has the `my_crate::cool_module` target prefix.
///     .with(stdout_log
///         .with_filter(
///             Targets::default()
///                 .with_target("my_crate::cool_module", Level::DEBUG)
///                 .with_default(Level::INFO)
///        )
///     )
///     // Log everything enabled by the global filter to `debug_log.json`.
///     .with(debug_log)
///     // Configure a global filter for the whole subscriber stack. This will
///     // control what spans and events are recorded by both the `debug_log`
///     // and the `stdout_log` layers, and `stdout_log` will *additionally* be
///     // filtered by its per-layer filter.
///     .with(
///         Targets::default()
///             .with_target("my_crate", Level::TRACE)
///             .with_target("other_crate", Level::INFO)
///             .with_target("other_crate::annoying_module", LevelFilter::OFF)
///             .with_target("third_crate", Level::DEBUG)
///     ).try_init()?;
/// # Ok(()) }
///```
///
/// [target]: tracing_core::Metadata::target
/// [level]: tracing_core::Level
/// [`Filter`]: crate::layer::Filter
/// [`Layer`]: crate::layer::Layer
/// [plf]: crate::layer#per-layer-filtering
/// [global]: crate::layer#global-filtering
/// [filtering]: crate::layer#filtering-with-layers
/// [`env_logger` crate]: https://docs.rs/env_logger/0.9.0/env_logger/index.html#enabling-logging
/// [`EnvFilter`]: crate::filter::EnvFilter
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Targets(DirectiveSet<StaticDirective>);

impl Targets {
    /// Returns a new `Targets` filter.
    ///
    /// This filter will enable no targets. Call [`with_target`] or [`with_targets`]
    /// to add enabled targets, and [`with_default`] to change the default level
    /// enabled for spans and events that didn't match any of the provided targets.
    ///
    /// [`with_target`]: Targets::with_target
    /// [`with_targets`]: Targets::with_targets
    /// [`with_default`]: Targets::with_default
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Enables spans and events with [target]s starting with the provided target
    /// prefix if they are at or below the provided [`LevelFilter`].
    ///
    /// # Examples
    ///
    /// ```
    /// use tracing_subscriber::filter;
    /// use tracing_core::Level;
    ///
    /// let filter = filter::Targets::new()
    ///     // Enable the `INFO` level for anything in `my_crate`
    ///     .with_target("my_crate", Level::INFO)
    ///     // Enable the `DEBUG` level for a specific module.
    ///     .with_target("my_crate::interesting_module", Level::DEBUG);
    /// # drop(filter);
    /// ```
    ///
    /// [`LevelFilter::OFF`] can be used to disable a particular target:
    /// ```
    /// use tracing_subscriber::filter::{Targets, LevelFilter};
    /// use tracing_core::Level;
    ///
    /// let filter = Targets::new()
    ///     .with_target("my_crate", Level::INFO)
    ///     // Disable all traces from `annoying_module`.
    ///     .with_target("my_crate::interesting_module", LevelFilter::OFF);
    /// # drop(filter);
    /// ```
    ///
    /// [target]: tracing_core::Metadata::target
    #[must_use]
    pub fn with_target(mut self, target: impl Into<String>, level: impl Into<LevelFilter>) -> Self {
        self.0.add(StaticDirective::new(
            Some(target.into()),
            Vec::default(),
            level.into(),
        ));
        self
    }
    /// Adds [target]s from an iterator of [target]-[`LevelFilter`] pairs to this filter.
    ///
    /// # Examples
    ///
    /// ```
    /// use tracing_subscriber::filter;
    /// use tracing_core::Level;
    ///
    /// let filter = filter::Targets::new()
    ///     .with_targets(vec![
    ///         ("my_crate", Level::INFO),
    ///         ("my_crate::some_module", Level::DEBUG),
    ///         ("my_crate::other_module::cool_stuff", Level::TRACE),
    ///         ("other_crate", Level::WARN)
    ///     ]);
    /// # drop(filter);
    /// ```
    ///
    /// [`LevelFilter::OFF`] can be used to disable a particular target:
    /// ```
    /// use tracing_subscriber::filter::{Targets, LevelFilter};
    /// use tracing_core::Level;
    ///
    /// let filter = Targets::new()
    ///     .with_target("my_crate", Level::INFO)
    ///     // Disable all traces from `annoying_module`.
    ///     .with_target("my_crate::interesting_module", LevelFilter::OFF);
    /// # drop(filter);
    /// ```
    ///
    /// [target]: tracing_core::Metadata::target
    #[must_use]
    pub fn with_targets<T, L>(mut self, targets: impl IntoIterator<Item = (T, L)>) -> Self
    where
        String: From<T>,
        LevelFilter: From<L>,
    {
        self.extend(targets);
        self
    }

    /// Sets the default level to enable for spans and events whose targets did
    /// not match any of the configured prefixes.
    ///
    /// By default, this is [`LevelFilter::OFF`]. This means that spans and
    /// events will only be enabled if they match one of the configured target
    /// prefixes. If this is changed to a different [`LevelFilter`], spans and
    /// events with targets that did not match any of the configured prefixes
    /// will be enabled if their level is at or below the provided level.
    #[must_use]
    pub fn with_default(mut self, level: impl Into<LevelFilter>) -> Self {
        self.0
            .add(StaticDirective::new(None, Vec::default(), level.into()));
        self
    }

    /// Returns the default level for this filter, if one is set.
    ///
    /// The default level is used to filter any spans or events with targets
    /// that do not match any of the configured set of prefixes.
    ///
    /// The default level can be set for a filter either by using
    /// [`with_default`](Self::with_default) or when parsing from a filter string that includes a
    /// level without a target (e.g. `"trace"`).
    ///
    /// # Examples
    ///
    /// ```
    /// use tracing_subscriber::filter::{LevelFilter, Targets};
    ///
    /// let filter = Targets::new().with_default(LevelFilter::INFO);
    /// if filter.default_level() != Some(LevelFilter::INFO) {
    ///     return Err("default level should be INFO".into());
    /// }
    ///
    /// let filter: Targets = "info".parse()?;
    /// if filter.default_level() != Some(LevelFilter::INFO) {
    ///     return Err("parsed default level should be INFO".into());
    /// }
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// The default level is `None` if no default is set:
    ///
    /// ```
    /// use tracing_subscriber::filter::Targets;
    ///
    /// let filter = Targets::new();
    /// if filter.default_level().is_some() {
    ///     return Err("unset default level should be None".into());
    /// }
    ///
    /// let filter: Targets = "my_crate=info".parse()?;
    /// if filter.default_level().is_some() {
    ///     return Err("target-specific directives should not set a default level".into());
    /// }
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// Note that an unset default level (`None`) behaves like [`LevelFilter::OFF`] when the filter is
    /// used, but it could also be set explicitly which may be useful to distinguish (such as when
    /// merging multiple `Targets`).
    ///
    /// ```
    /// use tracing_subscriber::filter::{LevelFilter, Targets};
    ///
    /// let filter = Targets::new().with_default(LevelFilter::OFF);
    /// if filter.default_level() != Some(LevelFilter::OFF) {
    ///     return Err("default level should be OFF".into());
    /// }
    ///
    /// let filter: Targets = "off".parse()?;
    /// if filter.default_level() != Some(LevelFilter::OFF) {
    ///     return Err("parsed default level should be OFF".into());
    /// }
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn default_level(&self) -> Option<LevelFilter> {
        self.0
            .directives()
            .find_map(|directive| directive.target.is_none().then_some(directive.level))
    }

    /// Returns an iterator over the [target]-[`LevelFilter`] pairs in this filter.
    ///
    /// The order of iteration is undefined.
    ///
    /// # Examples
    ///
    /// ```
    /// use tracing_subscriber::filter::{Targets, LevelFilter};
    /// use tracing_core::Level;
    ///
    /// let filter = Targets::new()
    ///     .with_target("my_crate", Level::INFO)
    ///     .with_target("my_crate::interesting_module", Level::DEBUG);
    ///
    /// let mut targets: Vec<_> = filter.iter().collect();
    /// targets.sort();
    ///
    /// let expected = vec![
    ///     ("my_crate", LevelFilter::INFO),
    ///     ("my_crate::interesting_module", LevelFilter::DEBUG),
    /// ];
    /// if targets != expected {
    ///     return Err("targets should iterate in sorted order after sorting".into());
    /// }
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// [target]: tracing_core::Metadata::target
    #[must_use]
    pub fn iter(&self) -> Iter<'_> {
        self.into_iter()
    }

    /// Returns the callsite interest for the provided metadata.
    #[inline]
    fn interested(&self, metadata: &'static Metadata<'static>) -> Interest {
        if self.0.enabled(metadata) {
            Interest::always()
        } else {
            Interest::never()
        }
    }

    /// Returns whether a [target]-[`Level`] pair would be enabled
    /// by this `Targets`.
    ///
    /// This method can be used with [`module_path!`] from `std` as the target
    /// in order to emulate the behavior of the [`tracing::event!`] and [`tracing::span!`]
    /// macros.
    ///
    /// # Examples
    ///
    /// ```
    /// use tracing_subscriber::filter::{Targets, LevelFilter};
    /// use tracing_core::Level;
    ///
    /// let filter = Targets::new()
    ///     .with_target("my_crate", Level::INFO)
    ///     .with_target("my_crate::interesting_module", Level::DEBUG);
    ///
    /// if !filter.would_enable("my_crate", Level::INFO) {
    ///     return Err("my_crate INFO should be enabled".into());
    /// }
    /// if filter.would_enable("my_crate::interesting_module", Level::TRACE) {
    ///     return Err("interesting_module TRACE should be disabled".into());
    /// }
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// [target]: tracing_core::Metadata::target
    /// [`module_path!`]: std::module_path!
    #[must_use]
    pub fn would_enable(&self, target: &str, level: Level) -> bool {
        // "Correct" to call because `Targets` only produces `StaticDirective`'s with NO
        // fields
        self.0.target_enabled(target, level)
    }
}

impl<T, L> Extend<(T, L)> for Targets
where
    T: Into<String>,
    L: Into<LevelFilter>,
{
    fn extend<I: IntoIterator<Item = (T, L)>>(&mut self, directives: I) {
        let parsed_directives = directives.into_iter().map(|(target, level)| {
            StaticDirective::new(Some(target.into()), Vec::default(), level.into())
        });
        self.0.extend(parsed_directives);
    }
}

impl<T, L> FromIterator<(T, L)> for Targets
where
    T: Into<String>,
    L: Into<LevelFilter>,
{
    fn from_iter<I: IntoIterator<Item = (T, L)>>(iter: I) -> Self {
        let mut this = Self::default();
        this.extend(iter);
        this
    }
}

impl FromStr for Targets {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.split(',')
            .map(StaticDirective::from_str)
            .collect::<Result<_, _>>()
            .map(Self)
    }
}

impl<S> layer::Layer<S> for Targets
where
    S: Subscriber,
{
    fn enabled(
        &self,
        metadata: &Metadata<'_>,
        _: layer::Context<'_, S>,
    ) -> SubscriberResult<bool> {
        Ok(self.0.enabled(metadata))
    }

    fn register_callsite(
        &self,
        metadata: &'static Metadata<'static>,
    ) -> SubscriberResult<Interest> {
        Ok(self.interested(metadata))
    }

    fn max_level_hint(&self) -> SubscriberResult<Option<LevelFilter>> {
        Ok(Some(self.0.max_level))
    }
}

#[cfg(feature = "registry")]
#[cfg_attr(docsrs, doc(cfg(feature = "registry")))]
impl<S> layer::Filter<S> for Targets {
    fn enabled(
        &self,
        metadata: &Metadata<'_>,
        _: &layer::Context<'_, S>,
    ) -> SubscriberResult<bool> {
        Ok(self.0.enabled(metadata))
    }

    fn callsite_enabled(
        &self,
        metadata: &'static Metadata<'static>,
    ) -> SubscriberResult<Interest> {
        Ok(self.interested(metadata))
    }

    fn max_level_hint(&self) -> SubscriberResult<Option<LevelFilter>> {
        Ok(Some(self.0.max_level))
    }
}

impl IntoIterator for Targets {
    type Item = (String, LevelFilter);

    type IntoIter = IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        IntoIter(self.0.into_iter().filter_map(|directive| {
            let level = directive.level;
            directive.target.map(|target| (target, level))
        }))
    }
}

impl<'a> IntoIterator for &'a Targets {
    type Item = (&'a str, LevelFilter);

    type IntoIter = Iter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        Iter(self.0.iter().filter_map(|directive| {
            directive
                .target
                .as_deref()
                .map(|target| (target, directive.level))
        }))
    }
}

impl fmt::Display for Targets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut directives = self.0.directives();
        if let Some(directive) = directives.next() {
            write!(f, "{directive}")?;
            for next_directive in directives {
                write!(f, ",{next_directive}")?;
            }
        }

        Ok(())
    }
}

/// Inner iterator type for [`IntoIter`].
type IntoTargetIter = FilterMap<
    <DirectiveSet<StaticDirective> as IntoIterator>::IntoIter,
    fn(StaticDirective) -> Option<(String, LevelFilter)>,
>;

/// An owning iterator over the [target]-[level] pairs of a `Targets` filter.
///
/// This struct is created by the `IntoIterator` trait implementation of [`Targets`].
///
/// # Examples
///
/// Merge the targets from one `Targets` with another:
///
/// ```
/// use tracing_subscriber::filter::Targets;
/// use tracing_core::Level;
///
/// let mut filter = Targets::new().with_target("my_crate", Level::INFO);
/// let overrides = Targets::new().with_target("my_crate::interesting_module", Level::DEBUG);
///
/// filter.extend(overrides);
/// # drop(filter);
/// ```
///
/// [target]: tracing_core::Metadata::target
/// [level]: tracing_core::Level
#[derive(Debug)]
pub struct IntoIter(IntoTargetIter);

impl Iterator for IntoIter {
    type Item = (String, LevelFilter);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

/// Inner iterator type for [`Iter`].
type TargetIter<'a> = FilterMap<
    slice::Iter<'a, StaticDirective>,
    fn(&'a StaticDirective) -> Option<(&'a str, LevelFilter)>,
>;

/// A borrowing iterator over the [target]-[level] pairs of a `Targets` filter.
///
/// This struct is created by [`iter`] method of [`Targets`], or from the `IntoIterator`
/// implementation for `&Targets`.
///
/// [target]: tracing_core::Metadata::target
/// [level]: tracing_core::Level
/// [`iter`]: Targets::iter
#[derive(Debug)]
pub struct Iter<'a>(TargetIter<'a>);

impl<'a> Iterator for Iter<'a> {
    type Item = (&'a str, LevelFilter);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{
        string::{String, ToString as _},
        vec,
        vec::Vec,
    };
    use strict_test_support::{TestFailure, ensure, ensure_ok};

    struct TargetExpectation {
        target: &'static str,
        level: LevelFilter,
    }

    fn expect_parse(source: &str) -> Result<Targets, TestFailure> {
        ensure_ok(source.parse::<Targets>(), "target filter should parse")
    }

    fn ensure_targets(
        dirs: &[StaticDirective],
        expected: &[TargetExpectation],
    ) -> Result<(), TestFailure> {
        ensure(dirs.len() == expected.len(), "parsed target count matches")?;
        for (directive, expectation) in dirs.iter().zip(expected) {
            ensure(
                directive.target.as_deref() == Some(expectation.target),
                "parsed target name matches",
            )?;
            ensure(
                directive.level == expectation.level,
                "parsed target level matches",
            )?;
            ensure(
                directive.field_names.is_empty(),
                "parsed target has no field filters",
            )?;
        }
        Ok(())
    }

    fn expect_parse_ralith(source: &str) -> Result<(), TestFailure> {
        let dirs = expect_parse(source)?.0.into_vec();
        ensure_targets(
            &dirs,
            &[
                TargetExpectation {
                    target: "server",
                    level: LevelFilter::DEBUG,
                },
                TargetExpectation {
                    target: "common",
                    level: LevelFilter::INFO,
                },
            ],
        )
    }

    fn expect_parse_level_directives(source: &str) -> Result<(), TestFailure> {
        let dirs = expect_parse(source)?.0.into_vec();
        ensure_targets(
            &dirs,
            &[
                TargetExpectation {
                    target: "crate3::mod2::mod1",
                    level: LevelFilter::OFF,
                },
                TargetExpectation {
                    target: "crate1::mod2::mod3",
                    level: LevelFilter::INFO,
                },
                TargetExpectation {
                    target: "crate1::mod2",
                    level: LevelFilter::WARN,
                },
                TargetExpectation {
                    target: "crate1::mod1",
                    level: LevelFilter::ERROR,
                },
                TargetExpectation {
                    target: "crate3",
                    level: LevelFilter::TRACE,
                },
                TargetExpectation {
                    target: "crate2",
                    level: LevelFilter::DEBUG,
                },
            ],
        )
    }

    #[test]
    fn parse_ralith() -> Result<(), TestFailure> {
        expect_parse_ralith("common=info,server=debug")
    }

    #[test]
    fn parse_ralith_uc() -> Result<(), TestFailure> {
        expect_parse_ralith("common=INFO,server=DEBUG")
    }

    #[test]
    fn parse_ralith_mixed() -> Result<(), TestFailure> {
        let _targets = expect_parse("common=iNfo,server=dEbUg")?;
        Ok(())
    }

    #[test]
    fn expect_parse_valid() -> Result<(), TestFailure> {
        let dirs = expect_parse("crate1::mod1=error,crate1::mod2,crate2=debug,crate3=off")?
            .0
            .into_vec();
        ensure_targets(
            &dirs,
            &[
                TargetExpectation {
                    target: "crate1::mod2",
                    level: LevelFilter::TRACE,
                },
                TargetExpectation {
                    target: "crate1::mod1",
                    level: LevelFilter::ERROR,
                },
                TargetExpectation {
                    target: "crate3",
                    level: LevelFilter::OFF,
                },
                TargetExpectation {
                    target: "crate2",
                    level: LevelFilter::DEBUG,
                },
            ],
        )
    }

    #[test]
    fn parse_level_directives() -> Result<(), TestFailure> {
        expect_parse_level_directives(
            "crate1::mod1=error,crate1::mod2=warn,crate1::mod2::mod3=info,\
             crate2=debug,crate3=trace,crate3::mod2::mod1=off",
        )
    }

    #[test]
    fn parse_uppercase_level_directives() -> Result<(), TestFailure> {
        expect_parse_level_directives(
            "crate1::mod1=ERROR,crate1::mod2=WARN,crate1::mod2::mod3=INFO,\
             crate2=DEBUG,crate3=TRACE,crate3::mod2::mod1=OFF",
        )
    }

    #[test]
    fn parse_numeric_level_directives() -> Result<(), TestFailure> {
        expect_parse_level_directives(
            "crate1::mod1=1,crate1::mod2=2,crate1::mod2::mod3=3,crate2=4,\
             crate3=5,crate3::mod2::mod1=0",
        )
    }

    #[test]
    fn targets_iter() -> Result<(), TestFailure> {
        let filter = expect_parse("crate1::mod1=error,crate1::mod2,crate2=debug,crate3=off")?
            .with_default(LevelFilter::WARN);

        let mut targets: Vec<_> = filter.iter().collect();
        targets.sort();

        ensure(
            targets
                == vec![
                    ("crate1::mod1", LevelFilter::ERROR),
                    ("crate1::mod2", LevelFilter::TRACE),
                    ("crate2", LevelFilter::DEBUG),
                    ("crate3", LevelFilter::OFF),
                ],
            "borrowed targets iterator yields sorted target levels",
        )
    }

    #[test]
    fn targets_into_iter() -> Result<(), TestFailure> {
        let filter = expect_parse("crate1::mod1=error,crate1::mod2,crate2=debug,crate3=off")?
            .with_default(LevelFilter::WARN);

        let mut targets: Vec<_> = filter.into_iter().collect();
        targets.sort();

        ensure(
            targets
                == vec![
                    (String::from("crate1::mod1"), LevelFilter::ERROR),
                    (String::from("crate1::mod2"), LevelFilter::TRACE),
                    (String::from("crate2"), LevelFilter::DEBUG),
                    (String::from("crate3"), LevelFilter::OFF),
                ],
            "owned targets iterator yields sorted target levels",
        )
    }

    #[test]
    fn targets_default_level() -> Result<(), TestFailure> {
        let initial_filter =
            expect_parse("crate1::mod1=error,crate1::mod2,crate2=debug,crate3=off")?;
        ensure(
            initial_filter.default_level().is_none(),
            "default level starts empty",
        )?;

        let off_filter = expect_parse("crate1::mod1=error,crate1::mod2,crate2=debug,crate3=off")?
            .with_default(LevelFilter::OFF);
        ensure(
            off_filter.default_level() == Some(LevelFilter::OFF),
            "default level can be set to off",
        )?;

        let info_filter = expect_parse("crate1::mod1=error,crate1::mod2,crate2=debug,crate3=off")?
            .with_default(LevelFilter::OFF)
            .with_default(LevelFilter::INFO);
        ensure(
            info_filter.default_level() == Some(LevelFilter::INFO),
            "default level can be overwritten",
        )
    }

    #[test]
    #[cfg(feature = "std")]
    fn size_of_filters() -> Result<(), TestFailure> {
        use std::mem::size_of_val;

        fn ensure_filter_has_size(source: &str) -> Result<(), TestFailure> {
            let filter = expect_parse(source)?;
            ensure(size_of_val(&filter) > 0, "parsed filter has a positive size")
        }

        ensure_filter_has_size("info")?;
        ensure_filter_has_size("foo=debug")?;
        ensure_filter_has_size(
            "crate1::mod1=error,crate1::mod2=warn,crate1::mod2::mod3=info,\
            crate2=debug,crate3=trace,crate3::mod2::mod1=off",
        )
    }

    /// Test that the `fmt::Display` implementation for `Targets` emits a string
    /// that can itself be parsed as a `Targets`, and that the parsed `Targets`
    /// is equivalent to the original one.
    #[test]
    fn display_roundtrips() -> Result<(), TestFailure> {
        fn test_roundtrip(source: &str) -> Result<(), TestFailure> {
            let filter = expect_parse(source)?;
            // we don't assert that the display output is equivalent to the
            // original parsed filter string, because the `Display` impl always
            // uses lowercase level names and doesn't use the
            // target-without-level shorthand syntax. while they may not be
            // textually equivalent, though, they should still *parse* to the
            // same filter.
            let formatted = filter.to_string();
            let parsed_filter = ensure_ok(
                formatted.parse::<Targets>(),
                "formatted target filter should parse",
            )?;
            ensure(filter == parsed_filter, "display roundtrip preserves targets")
        }

        test_roundtrip("crate1::mod1=error,crate1::mod2,crate2=debug,crate3=off")?;
        test_roundtrip(
            "crate1::mod1=ERROR,crate1::mod2=WARN,crate1::mod2::mod3=INFO,\
        crate2=DEBUG,crate3=TRACE,crate3::mod2::mod1=OFF",
        )?;
        test_roundtrip(
            "crate1::mod1=error,crate1::mod2=warn,crate1::mod2::mod3=info,\
             crate2=debug,crate3=trace,crate3::mod2::mod1=off",
        )?;
        test_roundtrip("crate1::mod1,crate1::mod2,info")?;
        test_roundtrip("crate1")?;
        test_roundtrip("info")
    }
}
