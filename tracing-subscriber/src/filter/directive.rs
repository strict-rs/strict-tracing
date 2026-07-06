use crate::filter::level::{self, LevelFilter};
#[cfg(feature = "std")]
use alloc::boxed::Box;
#[cfg(not(feature = "smallvec"))]
use alloc::vec::IntoIter;
use alloc::{string::String, vec::Vec};

use core::{cmp::Ordering, fmt, iter::FromIterator, slice, str::FromStr};
#[cfg(feature = "std")]
use std::error::Error;
use tracing_core::{Level, Metadata};

/// Indicates that a string could not be parsed as a filtering directive.
#[derive(Debug)]
pub struct ParseError {
    /// The specific reason directive parsing failed.
    kind: ParseErrorKind,
}

/// A directive which will statically enable or disable a given callsite.
///
/// Unlike a dynamic directive, this can be cached by the callsite.
#[derive(Debug, PartialEq, Eq, Clone)]
pub(in crate::filter) struct StaticDirective {
    /// The optional target prefix matched by this directive.
    pub(in crate::filter) target: Option<String>,
    /// Field names that must be present on matching event metadata.
    pub(in crate::filter) field_names: Vec<String>,
    /// The maximum level enabled by this directive.
    pub(in crate::filter) level: LevelFilter,
}

/// Storage used for directive lists.
#[cfg(feature = "smallvec")]
pub(in crate::filter) type FilterVec<T> = smallvec::SmallVec<[T; 8]>;
/// Storage used for directive lists.
#[cfg(not(feature = "smallvec"))]
pub(in crate::filter) type FilterVec<T> = Vec<T>;

/// A sorted set of directives with a precomputed maximum level.
#[derive(Debug, PartialEq, Clone)]
pub(in crate::filter) struct DirectiveSet<T> {
    /// Directives sorted from most-specific to least-specific.
    directives: FilterVec<T>,
    /// The highest verbosity enabled by any directive in the set.
    pub(in crate::filter) max_level: LevelFilter,
}

/// Common behavior for directive types that can match metadata.
pub(in crate::filter) trait Match {
    /// Returns whether this directive applies to the metadata.
    fn cares_about(&self, meta: &Metadata<'_>) -> bool;
    /// Returns the maximum level enabled by this directive.
    fn level(&self) -> &LevelFilter;
}

/// The reason directive parsing failed.
#[derive(Debug)]
enum ParseErrorKind {
    /// A field matcher could not be parsed.
    #[cfg(feature = "std")]
    Field(Box<dyn Error + Send + Sync>),
    /// The level component could not be parsed.
    Level(level::ParseError),
    /// A syntactic error occurred, with an optional message.
    Other(Option<&'static str>),
}

// === impl DirectiveSet ===

impl<T> DirectiveSet<T> {
    // this is only used by `env-filter`.
    #[cfg(all(feature = "std", feature = "env-filter"))]
    /// Returns whether the set contains no directives.
    pub(crate) fn is_empty(&self) -> bool {
        self.directives.is_empty()
    }

    /// Returns an iterator over all directives in specificity order.
    pub(crate) fn iter(&self) -> slice::Iter<'_, T> {
        self.directives.iter()
    }
}

impl<T: Ord> Default for DirectiveSet<T> {
    fn default() -> Self {
        Self {
            directives: FilterVec::new(),
            max_level: LevelFilter::OFF,
        }
    }
}

impl<T: Match + Ord> DirectiveSet<T> {
    /// Returns an iterator over all directives in specificity order.
    pub(crate) fn directives(&self) -> impl Iterator<Item = &T> {
        self.directives.iter()
    }

    /// Returns directives that match the provided metadata.
    pub(crate) fn directives_for<'a>(
        &'a self,
        metadata: &'a Metadata<'a>,
    ) -> impl Iterator<Item = &'a T> + 'a {
        self.directives()
            .filter(move |directive| directive.cares_about(metadata))
    }

    /// Adds or replaces a directive, preserving specificity order.
    pub(crate) fn add(&mut self, directive: T) {
        // does this directive enable a more verbose level than the current
        // max? if so, update the max level.
        let level = *directive.level();
        if level > self.max_level {
            self.max_level = level;
        }
        // insert the directive into the vec of directives, ordered by
        // specificity (length of target + number of field filters). this
        // ensures that, when finding a directive to match a span or event, we
        // search the directive set in most specific first order.
        if let Ok(index) = self.directives.binary_search(&directive) {
            if let Some(existing) = self.directives.get_mut(index) {
                *existing = directive;
            } else {
                self.directives.push(directive);
                self.directives.sort_unstable();
            }
        } else {
            self.directives.push(directive);
            self.directives.sort_unstable();
        }
    }

    #[cfg(test)]
    /// Returns the directive storage for tests.
    pub(in crate::filter) fn into_vec(self) -> FilterVec<T> {
        self.directives
    }
}

impl<T: Match + Ord> FromIterator<T> for DirectiveSet<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut this = Self::default();
        this.extend(iter);
        this
    }
}

impl<T: Match + Ord> Extend<T> for DirectiveSet<T> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        for directive in iter {
            self.add(directive);
        }
    }
}

impl<T> IntoIterator for DirectiveSet<T> {
    type Item = T;

    #[cfg(feature = "smallvec")]
    type IntoIter = smallvec::IntoIter<[T; 8]>;
    #[cfg(not(feature = "smallvec"))]
    type IntoIter = IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.directives.into_iter()
    }
}

// === impl Statics ===

impl DirectiveSet<StaticDirective> {
    /// Returns whether metadata is enabled by the static directive set.
    pub(crate) fn enabled(&self, meta: &Metadata<'_>) -> bool {
        let level = meta.level();
        self.directives_for(meta)
            .next()
            .is_some_and(|directive| directive.level >= *level)
    }

    /// Same as `enabled` above, but skips `Directive`'s with fields.
    pub(crate) fn target_enabled(&self, target: &str, level: Level) -> bool {
        self.directives_for_target(target)
            .next()
            .is_some_and(|directive| directive.level >= level)
    }

    /// Returns target-only directives that match the provided target.
    pub(crate) fn directives_for_target<'a>(
        &'a self,
        target: &'a str,
    ) -> impl Iterator<Item = &'a StaticDirective> + 'a {
        self.directives()
            .filter(move |directive| directive.cares_about_target(target))
    }
}

// === impl StaticDirective ===

impl StaticDirective {
    /// Constructs a static directive from its parsed components.
    pub(in crate::filter) const fn new(
        target: Option<String>,
        field_names: Vec<String>,
        level: LevelFilter,
    ) -> Self {
        Self {
            target,
            field_names,
            level,
        }
    }

    /// Returns whether this directive applies to the provided target.
    pub(in crate::filter) fn cares_about_target(&self, target_to_check: &str) -> bool {
        // Does this directive have a target filter, and does it match the
        // metadata's target?
        if let Some(target) = self.target.as_deref()
            && !target_to_check.starts_with(target)
        {
            return false;
        }

        if !self.field_names.is_empty() {
            return false;
        }

        true
    }
}

impl Ord for StaticDirective {
    fn cmp(&self, other: &Self) -> Ordering {
        // We attempt to order directives by how "specific" they are. This
        // ensures that we try the most specific directives first when
        // attempting to match a piece of metadata.

        // First, we compare based on whether a target is specified, and the
        // lengths of those targets if both have targets.
        self
            .target
            .as_ref()
            .map(String::len)
            .cmp(&other.target.as_ref().map(String::len))
            // Then we compare how many field names are matched by each directive.
            .then_with(|| self.field_names.len().cmp(&other.field_names.len()))
            // Finally, we fall back to lexicographical ordering if the directives are
            // equally specific. Although this is no longer semantically important,
            // we need to define a total ordering to determine the directive's place
            // in the BTreeMap.
            .then_with(|| {
                self.target
                    .cmp(&other.target)
                    .then_with(|| self.field_names[..].cmp(&other.field_names[..]))
            })
            .reverse()
    }
}

impl PartialOrd for StaticDirective {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Match for StaticDirective {
    fn cares_about(&self, meta: &Metadata<'_>) -> bool {
        // Does this directive have a target filter, and does it match the
        // metadata's target?
        if let Some(target) = self.target.as_deref()
            && !meta.target().starts_with(target)
        {
            return false;
        }

        if meta.is_event()
            && !self.field_names.is_empty()
            && !self
                .field_names
                .iter()
                .all(|field_name| meta.fields().field(field_name).is_some())
        {
            return false;
        }

        true
    }

    fn level(&self) -> &LevelFilter {
        &self.level
    }
}

impl Default for StaticDirective {
    fn default() -> Self {
        Self {
            target: None,
            field_names: Vec::new(),
            level: LevelFilter::ERROR,
        }
    }
}

impl fmt::Display for StaticDirective {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let wrote_target = if let Some(ref target) = self.target {
            fmt::Display::fmt(target, f)?;
            true
        } else {
            false
        };

        let wrote_fields = if self.field_names.is_empty() {
            false
        } else {
            f.write_str("[")?;

            let mut fields = self.field_names.iter();
            if let Some(field) = fields.next() {
                write!(f, "{{{field}")?;
                fields.try_for_each(|field_name| write!(f, ",{field_name}"))?;
                f.write_str("}")?;
            }

            f.write_str("]")?;
            true
        };

        if wrote_target || wrote_fields {
            f.write_str("=")?;
        }

        fmt::Display::fmt(&self.level, f)
    }
}

impl FromStr for StaticDirective {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // This method parses a filtering directive in one of the following
        // forms:
        //
        // * `foo=trace` (TARGET=LEVEL)
        // * `foo[{bar,baz}]=info` (TARGET[{FIELD,+}]=LEVEL)
        // * `trace` (bare LEVEL)
        // * `foo` (bare TARGET)
        let mut split = s.split('=');
        let part0 = split
            .next()
            .ok_or_else(|| ParseError::msg("string must not be empty"))?;

        // Directive includes an `=`:
        // * `foo=trace`
        // * `foo[{bar}]=trace`
        // * `foo[{bar,baz}]=trace`
        if let Some(part1) = split.next() {
            if split.next().is_some() {
                return Err(ParseError::msg(
                    "too many '=' in filter directive, expected 0 or 1",
                ));
            }

            let mut target_and_fields = part0.split("[{");
            let target = target_and_fields.next().map(String::from);
            let mut field_names = Vec::new();
            // Directive includes fields:
            // * `foo[{bar}]=trace`
            // * `foo[{bar,baz}]=trace`
            let maybe_field_list = target_and_fields.next();
            if target_and_fields.next().is_some() {
                return Err(ParseError::msg(
                    "too many '[{' in filter directive, expected 0 or 1",
                ));
            }

            match maybe_field_list {
                Some(field_list) if !field_list.ends_with("}]") => {
                    return Err(ParseError::msg("expected fields list to end with '}]'"));
                }
                Some(field_list) => {
                    let fields = field_list
                        .trim_end_matches("}]")
                        .split(',')
                        .filter(|field| !field.is_empty())
                        .map(String::from);
                    field_names.extend(fields);
                }
                None => {}
            }
            let level = part1.parse()?;
            return Ok(Self {
                target,
                field_names,
                level,
            });
        }

        // Okay, the part after the `=` was empty, the directive is either a
        // bare level or a bare target.
        // * `foo`
        // * `info`
        part0.parse::<LevelFilter>().map_or_else(
            |_| {
                Ok(Self {
                    target: Some(String::from(part0)),
                    field_names: Vec::new(),
                    level: LevelFilter::TRACE,
                })
            },
            |level| {
                Ok(Self {
                    target: None,
                    field_names: Vec::new(),
                    level,
                })
            },
        )
    }
}

// === impl ParseError ===

impl ParseError {
    #[cfg(all(feature = "std", feature = "env-filter"))]
    /// Constructs a generic directive parse error.
    pub(crate) const fn new() -> Self {
        Self {
            kind: ParseErrorKind::Other(None),
        }
    }

    /// Constructs a directive parse error with a static message.
    pub(crate) const fn msg(message: &'static str) -> Self {
        Self {
            kind: ParseErrorKind::Other(Some(message)),
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            ParseErrorKind::Other(None) => f.pad("invalid filter directive"),
            ParseErrorKind::Other(Some(msg)) => {
                write!(f, "invalid filter directive: {msg}")
            }
            ParseErrorKind::Level(ref level) => level.fmt(f),
            #[cfg(feature = "std")]
            ParseErrorKind::Field(ref error) => write!(f, "invalid field filter: {error}"),
        }
    }
}

#[cfg(feature = "std")]
impl Error for ParseError {
    fn description(&self) -> &'static str {
        "invalid filter directive"
    }

    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self.kind {
            ParseErrorKind::Other(_) => None,
            ParseErrorKind::Level(ref level) => Some(level),
            ParseErrorKind::Field(ref error) => Some(error.as_ref()),
        }
    }
}

#[cfg(feature = "std")]
impl From<Box<dyn Error + Send + Sync>> for ParseError {
    fn from(error: Box<dyn Error + Send + Sync>) -> Self {
        Self {
            kind: ParseErrorKind::Field(error),
        }
    }
}

impl From<level::ParseError> for ParseError {
    fn from(error: level::ParseError) -> Self {
        Self {
            kind: ParseErrorKind::Level(error),
        }
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
    use core::cmp::Ordering;
    #[cfg(feature = "std")]
    use std::error::Error;
    use strict_test_support::{TestFailure, ensure, ensure_contains, ensure_ok};
    use tracing_core::callsite::Callsite;
    use tracing_core::metadata::Kind;
    use tracing_core::subscriber::Interest;

    /// Callsite shared by directive metadata fixtures.
    struct DirectiveTestCallsite;

    /// Static callsite used by directive metadata fixtures.
    static DIRECTIVE_TEST_CALLSITE: DirectiveTestCallsite = DirectiveTestCallsite;

    /// Typed source error used to test field parse error forwarding.
    #[cfg(all(feature = "std", feature = "env-filter"))]
    #[derive(Debug)]
    struct FieldParseTestError;

    static EVENT_WITH_FIELDS: Metadata<'static> = tracing_core::metadata! {
        name: "directive_event",
        target: "app::module",
        level: Level::INFO,
        fields: &["field", "other"],
        callsite: &DIRECTIVE_TEST_CALLSITE,
        kind: Kind::EVENT,
    };

    static EVENT_WITHOUT_FIELD: Metadata<'static> = tracing_core::metadata! {
        name: "directive_event_without_field",
        target: "app::module",
        level: Level::INFO,
        fields: &["other"],
        callsite: &DIRECTIVE_TEST_CALLSITE,
        kind: Kind::EVENT,
    };

    static SPAN_WITHOUT_FIELD: Metadata<'static> = tracing_core::metadata! {
        name: "directive_span",
        target: "app::module",
        level: Level::DEBUG,
        fields: &[],
        callsite: &DIRECTIVE_TEST_CALLSITE,
        kind: Kind::SPAN,
    };

    static OTHER_TARGET_EVENT: Metadata<'static> = tracing_core::metadata! {
        name: "directive_other_event",
        target: "other::module",
        level: Level::INFO,
        fields: &["field"],
        callsite: &DIRECTIVE_TEST_CALLSITE,
        kind: Kind::EVENT,
    };

    impl Callsite for DirectiveTestCallsite {
        fn set_interest(&self, _: Interest) {}

        fn metadata(&self) -> &Metadata<'_> {
            &EVENT_WITH_FIELDS
        }
    }

    #[cfg(all(feature = "std", feature = "env-filter"))]
    impl fmt::Display for FieldParseTestError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("bad matcher")
        }
    }

    #[cfg(all(feature = "std", feature = "env-filter"))]
    impl Error for FieldParseTestError {}

    fn parse_static(source: &str) -> Result<StaticDirective, TestFailure> {
        ensure_ok(source.parse(), "static directive should parse")
    }

    #[test]
    fn directive_set_sorts_replaces_and_tracks_max_level() -> Result<(), TestFailure> {
        let mut directives = DirectiveSet::default();
        ensure(directives.iter().next().is_none(), "default set is empty")?;
        ensure(
            directives.max_level == LevelFilter::OFF,
            "default max level starts off",
        )?;

        directives.extend(vec![
            parse_static("app=info")?,
            parse_static("app::module=trace")?,
            parse_static("app[{field}]=debug")?,
            parse_static("app=warn")?,
        ]);

        ensure(
            directives.max_level == LevelFilter::TRACE,
            "max level tracks the most verbose directive",
        )?;
        ensure(
            directives
                .into_iter()
                .map(|directive| directive.to_string())
                .collect::<Vec<_>>()
                == vec![
                    String::from("app::module=trace"),
                    String::from("app[{field}]=debug"),
                    String::from("app=warn"),
                ],
            "directives sort by specificity and equal directives are replaced",
        )
    }

    #[test]
    fn directive_set_matches_metadata_and_target_only_queries() -> Result<(), TestFailure> {
        let directives = DirectiveSet::from_iter(vec![
            parse_static("app::module[{field}]=trace")?,
            parse_static("app::module=warn")?,
            parse_static("app=error")?,
        ]);

        ensure(
            directives.enabled(&EVENT_WITH_FIELDS),
            "field directive enables an event carrying the required field",
        )?;
        ensure(
            !directives.enabled(&EVENT_WITHOUT_FIELD),
            "more specific field directive blocks a matching event missing the field",
        )?;
        ensure(
            directives.enabled(&SPAN_WITHOUT_FIELD),
            "span metadata ignores field-name filters and matches by target",
        )?;
        ensure(
            !directives.enabled(&OTHER_TARGET_EVENT),
            "nonmatching target is rejected",
        )?;
        ensure(
            directives.target_enabled("app::module::child", Level::WARN),
            "target-only lookup uses matching target directives",
        )?;
        ensure(
            !directives.target_enabled("app::module::child", Level::DEBUG),
            "target-only lookup respects the matched directive level",
        )
    }

    #[test]
    fn static_directive_parse_display_default_and_ordering_contracts() -> Result<(), TestFailure> {
        let bare_target = parse_static("app::module")?;
        ensure(
            bare_target.target.as_deref() == Some("app::module"),
            "bare target parses target",
        )?;
        ensure(
            bare_target.level == LevelFilter::TRACE,
            "bare target defaults to trace",
        )?;

        let bare_level = parse_static("warn")?;
        ensure(bare_level.target.is_none(), "bare level has no target")?;
        ensure(
            bare_level.level == LevelFilter::WARN,
            "bare level parses the level",
        )?;

        let fields = parse_static("app[{field,other}]=debug")?;
        ensure(
            fields.field_names == vec![String::from("field"), String::from("other")],
            "field list preserves directive field order",
        )?;
        ensure(
            fields.to_string() == "app[{field,other}]=debug",
            "field directive display preserves target fields and level",
        )?;

        let default = StaticDirective::default();
        ensure(default.target.is_none(), "default directive has no target")?;
        ensure(
            default.field_names.is_empty(),
            "default directive has no field filters",
        )?;
        ensure(
            default.level == LevelFilter::ERROR,
            "default directive enables error",
        )?;

        let specific = parse_static("app::module=info")?;
        let general = parse_static("app=trace")?;
        ensure(
            specific < general,
            "longer target sorts ahead of shorter target",
        )?;
        ensure(
            fields.partial_cmp(&general) == Some(Ordering::Less),
            "field-specific directive participates in total ordering",
        )
    }

    #[test]
    fn static_directive_rejects_malformed_field_and_level_syntax() -> Result<(), TestFailure> {
        let too_many_equals = "app=info=debug".parse::<StaticDirective>();
        ensure(
            too_many_equals.is_err(),
            "directives reject more than one equals sign",
        )?;
        let Err(too_many_equals_error) = too_many_equals else {
            return ensure(false, "malformed directive should fail");
        };
        ensure_contains(
            &too_many_equals_error.to_string(),
            "too many '='",
            "equals error names the malformed separator",
        )?;

        let too_many_fields = "app[{one}[{two}]=info".parse::<StaticDirective>();
        ensure(
            too_many_fields.is_err(),
            "directives reject more than one field-list opener",
        )?;
        let Err(too_many_fields_error) = too_many_fields else {
            return ensure(false, "malformed field directive should fail");
        };
        ensure_contains(
            &too_many_fields_error.to_string(),
            "too many '[{'",
            "field opener error names the malformed opener",
        )?;

        let unclosed_fields = "app[{field=info".parse::<StaticDirective>();
        ensure(
            unclosed_fields.is_err(),
            "directives reject unclosed field lists",
        )?;
        let Err(unclosed_fields_error) = unclosed_fields else {
            return ensure(false, "unclosed field directive should fail");
        };
        ensure_contains(
            &unclosed_fields_error.to_string(),
            "expected fields list to end with '}]'",
            "field-list error names the missing terminator",
        )?;

        let invalid_level = "app=definitely-not-a-level".parse::<StaticDirective>();
        ensure(invalid_level.is_err(), "invalid levels are rejected")?;
        let Err(invalid_level_error) = invalid_level else {
            return ensure(false, "invalid level directive should fail");
        };
        ensure_contains(
            &invalid_level_error.to_string(),
            "level",
            "level parser error is surfaced",
        )
    }

    #[test]
    #[cfg(all(feature = "std", feature = "env-filter"))]
    fn parse_error_display_and_source_preserve_error_kinds() -> Result<(), TestFailure> {
        let generic = ParseError::new();
        ensure(
            generic.source().is_none(),
            "generic parse error has no source",
        )?;
        ensure(
            generic.to_string() == "invalid filter directive",
            "generic parse error display is stable",
        )?;

        let message = ParseError::msg("named reason");
        ensure(
            message.source().is_none(),
            "message parse error has no source",
        )?;
        ensure(
            message.to_string() == "invalid filter directive: named reason",
            "message parse error display includes the reason",
        )?;

        let field_error: Box<dyn Error + Send + Sync> = Box::new(FieldParseTestError);
        let field_parse_error = ParseError::from(field_error);
        ensure(
            field_parse_error.source().is_some(),
            "field parse error exposes the boxed matcher source",
        )?;
        ensure_contains(
            &field_parse_error.to_string(),
            "bad matcher",
            "field parse error display includes the source error",
        )
    }
}
