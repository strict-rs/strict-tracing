use alloc::{
    borrow::ToOwned as _,
    boxed::Box,
    string::String,
    sync::Arc,
};
use core::{
    cmp::Ordering,
    fmt,
    str::FromStr,
    sync::atomic::{
        AtomicBool,
        Ordering::{Acquire, Release},
    },
};
use matchers::Pattern;
use std::error::Error;

use super::{FieldMap, LevelFilter};
use tracing_core::field::{Field, Visit};

/// A parsed field matcher.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Match {
    /// The field name matched by this directive.
    pub(crate) name: String, // TODO: allow match patterns for names?
    /// The optional value matcher for this field.
    pub(crate) value: Option<ValueMatch>,
}

/// Field-value matchers associated with a callsite.
#[derive(Debug, Eq, PartialEq)]
pub(super) struct CallsiteMatch {
    /// The values expected for fields on this callsite.
    pub(crate) fields: FieldMap<ValueMatch>,
    /// The level enabled if all values match.
    pub(crate) level: LevelFilter,
}

/// Field-value matchers associated with a span instance.
#[derive(Debug)]
pub(super) struct SpanMatch {
    /// The values expected for fields on this span.
    fields: FieldMap<(ValueMatch, AtomicBool)>,
    /// The level enabled if all values match.
    level: LevelFilter,
    /// Cached indicator that every field has matched.
    has_matched: AtomicBool,
}

/// Visitor that records field values into a span matcher.
pub(super) struct MatchVisitor<'a> {
    /// The span matcher updated by this visitor.
    inner: &'a SpanMatch,
}

/// A parsed field-value matcher.
#[derive(Debug, Clone)]
pub(super) enum ValueMatch {
    /// Matches a specific `bool` value.
    Bool(bool),
    /// Matches a specific `f64` value.
    F64(f64),
    /// Matches a specific `u64` value.
    U64(u64),
    /// Matches a specific `i64` value.
    I64(i64),
    /// Matches any `NaN` `f64` value.
    NaN,
    /// Matches any field whose `fmt::Debug` output is equal to a fixed string.
    Debug(Box<MatchDebug>),
    /// Matches any field whose `fmt::Debug` output matches a regular expression
    /// pattern.
    Pat(Box<MatchPattern>),
}

impl Eq for ValueMatch {}

impl PartialEq for ValueMatch {
    fn eq(&self, other: &Self) -> bool {
        if self.sort_rank() != other.sort_rank() {
            return false;
        }

        match *self {
            Self::Bool(this) => matches!(*other, Self::Bool(that) if this.eq(&that)),
            Self::F64(this) => matches!(*other, Self::F64(that) if this.eq(&that)),
            Self::U64(this) => matches!(*other, Self::U64(that) if this.eq(&that)),
            Self::I64(this) => matches!(*other, Self::I64(that) if this.eq(&that)),
            Self::NaN => true,
            Self::Debug(ref this) => {
                matches!(*other, Self::Debug(ref that) if this.eq(that))
            }
            Self::Pat(ref this) => matches!(*other, Self::Pat(ref that) if this.eq(that)),
        }
    }
}

impl Ord for ValueMatch {
    fn cmp(&self, other: &Self) -> Ordering {
        let rank_ordering = self.sort_rank().cmp(&other.sort_rank());
        if !rank_ordering.is_eq() {
            return rank_ordering;
        }

        match *self {
            Self::Bool(this) => {
                if let Self::Bool(that) = *other {
                    this.cmp(&that)
                } else {
                    Ordering::Equal
                }
            }
            Self::F64(this) => {
                if let Self::F64(that) = *other {
                    this.partial_cmp(&that).unwrap_or(Ordering::Equal)
                } else {
                    Ordering::Equal
                }
            }
            Self::NaN => Ordering::Equal,
            Self::U64(this) => {
                if let Self::U64(that) = *other {
                    this.cmp(&that)
                } else {
                    Ordering::Equal
                }
            }
            Self::I64(this) => {
                if let Self::I64(that) = *other {
                    this.cmp(&that)
                } else {
                    Ordering::Equal
                }
            }
            Self::Debug(ref this) => {
                if let Self::Debug(ref that) = *other {
                    this.cmp(that)
                } else {
                    Ordering::Equal
                }
            }
            Self::Pat(ref this) => {
                if let Self::Pat(ref that) = *other {
                    this.cmp(that)
                } else {
                    Ordering::Equal
                }
            }
        }
    }
}

impl PartialOrd for ValueMatch {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Matches a field's `fmt::Debug` output against a regular expression pattern.
///
/// This is used for matching all non-literal field value filters when regular
/// expressions are enabled.
#[derive(Debug, Clone)]
pub(super) struct MatchPattern {
    /// The compiled regular expression matcher.
    pub(crate) matcher: Pattern,
    /// The source pattern used for display and ordering.
    pattern: Arc<str>,
}

/// Matches a field's `fmt::Debug` output against a fixed string pattern.
///
/// This is used for matching all non-literal field value filters when regular
/// expressions are disabled.
#[derive(Debug, Clone)]
pub(super) struct MatchDebug {
    /// The escaped exact matcher used against debug output.
    matcher: Box<Pattern>,
    /// The exact debug-output pattern used for display and ordering.
    pattern: Arc<str>,
}

/// Indicates that a field name specified in a filter directive was invalid.
#[derive(Clone, Debug)]
#[cfg_attr(docsrs, doc(cfg(feature = "env-filter")))]
pub struct BadName {
    /// The invalid field name.
    name: String,
}

// === impl Match ===

impl Match {
    /// Returns whether this field matcher includes a value matcher.
    #[allow(
        clippy::single_call_fn,
        reason = "directive classification names the value-matcher predicate"
    )]
    pub(crate) const fn has_value(&self) -> bool {
        self.value.is_some()
    }

    // TODO: reference count these strings?
    /// Returns a cloned field name for static directive construction.
    #[allow(
        clippy::single_call_fn,
        reason = "static directive construction keeps field-name cloning behind a named query"
    )]
    pub(crate) fn name(&self) -> String {
        self.name.clone()
    }

    /// Parses a field matcher from a directive field component.
    #[allow(
        clippy::single_call_fn,
        reason = "directive parsing keeps field matcher parsing as a named validation boundary"
    )]
    pub(crate) fn parse(
        source: &str,
        regex: bool,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let mut parts = source.split('=');
        let name = parts
            .next()
            .ok_or_else(|| BadName {
                name: String::new(),
            })?
            .to_owned();
        let parsed = parts
            .next()
            .map(|part| {
                if regex {
                    ValueMatch::parse_regex(part)
                } else {
                    ValueMatch::parse_non_regex(part)
                }
            })
            .transpose()?;
        Ok(Self { name, value: parsed })
    }
}

impl fmt::Display for Match {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.name, f)?;
        if let Some(ref value_match) = self.value {
            write!(f, "={value_match}")?;
        }
        Ok(())
    }
}

impl Ord for Match {
    fn cmp(&self, other: &Self) -> Ordering {
        // Ordering for `Match` directives is based first on _whether_ a value
        // is matched or not. This is semantically meaningful --- we would
        // prefer to check directives that match values first as they are more
        // specific.
        let has_value = match (self.value.as_ref(), other.value.as_ref()) {
            (Some(_), None) => Ordering::Greater,
            (None, Some(_)) => Ordering::Less,
            _ => Ordering::Equal,
        };
        // If both directives match a value, we fall back to the field names in
        // length + lexicographic ordering, and if these are equal as well, we
        // compare the match directives.
        //
        // This ordering is no longer semantically meaningful but is necessary
        // so that the directives can be stored in the `BTreeMap` in a defined
        // order.
        has_value
            .then_with(|| self.name.cmp(&other.name))
            .then_with(|| self.value.cmp(&other.value))
    }
}

impl PartialOrd for Match {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// === impl ValueMatch ===

/// Converts a parsed `f64` into the appropriate value matcher.
const fn value_match_f64(number: f64) -> ValueMatch {
    if number.is_nan() {
        ValueMatch::NaN
    } else {
        ValueMatch::F64(number)
    }
}

impl ValueMatch {
    /// Returns this matcher's ordering group.
    const fn sort_rank(&self) -> u8 {
        match *self {
            Self::Bool(_) => 0,
            Self::F64(_) => 1,
            Self::NaN => 2,
            Self::U64(_) => 3,
            Self::I64(_) => 4,
            Self::Debug(_) => 5,
            Self::Pat(_) => 6,
        }
    }

    /// Parse a `ValueMatch` that will match `fmt::Debug` fields using regular
    /// expressions.
    ///
    /// This returns an error if the string didn't contain a valid `bool`,
    /// `u64`, `i64`, or `f64` literal, and couldn't be parsed as a regular
    /// expression.
    #[allow(
        clippy::single_call_fn,
        reason = "regex value parsing is a distinct environment-filter matching mode"
    )]
    fn parse_regex(source: &str) -> Result<Self, Box<dyn Error + Send + Sync>> {
        if let Ok(parsed_bool) = source.parse::<bool>() {
            return Ok(Self::Bool(parsed_bool));
        }
        if let Ok(parsed_u64) = source.parse::<u64>() {
            return Ok(Self::U64(parsed_u64));
        }
        if let Ok(parsed_i64) = source.parse::<i64>() {
            return Ok(Self::I64(parsed_i64));
        }
        if let Ok(parsed_f64) = source.parse::<f64>() {
            return Ok(value_match_f64(parsed_f64));
        }

        source
            .parse::<MatchPattern>()
            .map(|pattern| Self::Pat(Box::new(pattern)))
            .map_err(|error| {
                let boxed_error: Box<dyn Error + Send + Sync> = Box::new(error);
                boxed_error
            })
    }

    /// Parse a `ValueMatch` that will match `fmt::Debug` against a fixed
    /// string.
    ///
    /// Any string that isn't a valid `bool`, `u64`, `i64`, or `f64` literal is
    /// treated as expected `fmt::Debug` output.
    #[allow(
        clippy::single_call_fn,
        reason = "non-regex value parsing is a distinct environment-filter matching mode"
    )]
    fn parse_non_regex(source: &str) -> Result<Self, Box<dyn Error + Send + Sync>> {
        if let Ok(parsed_bool) = source.parse::<bool>() {
            return Ok(Self::Bool(parsed_bool));
        }
        if let Ok(parsed_u64) = source.parse::<u64>() {
            return Ok(Self::U64(parsed_u64));
        }
        if let Ok(parsed_i64) = source.parse::<i64>() {
            return Ok(Self::I64(parsed_i64));
        }
        if let Ok(parsed_f64) = source.parse::<f64>() {
            return Ok(value_match_f64(parsed_f64));
        }

        MatchDebug::new(source).map(|matcher| Self::Debug(Box::new(matcher)))
    }

    /// Returns whether this matcher accepts an `f64` field value.
    fn matches_f64(&self, field_value: f64) -> bool {
        match *self {
            Self::NaN => field_value.is_nan(),
            Self::F64(expected) => field_value.total_cmp(&expected).is_eq(),
            Self::Bool(_) | Self::U64(_) | Self::I64(_) | Self::Debug(_) | Self::Pat(_) => false,
        }
    }

    /// Returns whether this matcher accepts an `i64` field value.
    fn matches_i64(&self, field_value: i64) -> bool {
        use std::convert::TryFrom as _;

        match *self {
            Self::I64(expected) => field_value == expected,
            Self::U64(expected) => u64::try_from(field_value).is_ok_and(|actual| actual == expected),
            Self::Bool(_) | Self::F64(_) | Self::NaN | Self::Debug(_) | Self::Pat(_) => false,
        }
    }

    /// Returns whether this matcher accepts a `u64` field value.
    const fn matches_u64(&self, field_value: u64) -> bool {
        match *self {
            Self::U64(expected) => field_value == expected,
            Self::Bool(_) | Self::F64(_) | Self::I64(_) | Self::NaN | Self::Debug(_) | Self::Pat(_) => false,
        }
    }

    /// Returns whether this matcher accepts a `bool` field value.
    const fn matches_bool(&self, field_value: bool) -> bool {
        match *self {
            Self::Bool(expected) => field_value == expected,
            Self::F64(_) | Self::U64(_) | Self::I64(_) | Self::NaN | Self::Debug(_) | Self::Pat(_) => false,
        }
    }

    /// Returns whether this matcher accepts a string field value.
    fn matches_str(&self, field_value: &str) -> bool {
        match *self {
            Self::Pat(ref expected) => expected.str_matches(&field_value),
            Self::Debug(ref expected) => expected.debug_matches(&field_value),
            Self::Bool(_) | Self::F64(_) | Self::U64(_) | Self::I64(_) | Self::NaN => false,
        }
    }

    /// Returns whether this matcher accepts a debug field value.
    fn matches_debug(&self, field_value: &dyn fmt::Debug) -> bool {
        match *self {
            Self::Pat(ref expected) => expected.debug_matches(&field_value),
            Self::Debug(ref expected) => expected.debug_matches(&field_value),
            Self::Bool(_) | Self::F64(_) | Self::U64(_) | Self::I64(_) | Self::NaN => false,
        }
    }
}

impl fmt::Display for ValueMatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Bool(inner) => fmt::Display::fmt(&inner, f),
            Self::F64(inner) => fmt::Display::fmt(&inner, f),
            Self::NaN => fmt::Display::fmt(&f64::NAN, f),
            Self::I64(inner) => fmt::Display::fmt(&inner, f),
            Self::U64(inner) => fmt::Display::fmt(&inner, f),
            Self::Debug(ref inner) => fmt::Display::fmt(inner, f),
            Self::Pat(ref inner) => fmt::Display::fmt(inner, f),
        }
    }
}

// === impl MatchPattern ===

impl FromStr for MatchPattern {
    type Err = matchers::BuildError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let matcher = s.parse::<Pattern>()?;
        Ok(Self {
            matcher,
            pattern: s.to_owned().into(),
        })
    }
}

impl fmt::Display for MatchPattern {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&*self.pattern, f)
    }
}

impl AsRef<str> for MatchPattern {
    #[inline]
    fn as_ref(&self) -> &str {
        self.pattern.as_ref()
    }
}

impl MatchPattern {
    /// Returns whether this pattern matches a string value.
    #[inline]
    fn str_matches(&self, field_value: &impl AsRef<str>) -> bool {
        self.matcher.matches(field_value)
    }

    /// Returns whether this pattern matches debug output.
    #[inline]
    fn debug_matches(&self, field_value: &impl fmt::Debug) -> bool {
        self.matcher.debug_matches(field_value)
    }

    /// Converts this regex matcher into an exact debug-output matcher.
    pub(super) fn into_debug_match(self) -> MatchDebug {
        let exact_pattern = exact_debug_pattern(&self.pattern);
        let matcher = match Pattern::new_anchored(&exact_pattern) {
            Ok(matcher) => matcher,
            Err(_error) => self.matcher,
        };
        MatchDebug {
            matcher: Box::new(matcher),
            pattern: self.pattern,
        }
    }
}

impl PartialEq for MatchPattern {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.pattern == other.pattern
    }
}

impl Eq for MatchPattern {}

impl PartialOrd for MatchPattern {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MatchPattern {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        self.pattern.cmp(&other.pattern)
    }
}

// === impl MatchDebug ===

impl MatchDebug {
    /// Creates a matcher for exact debug output.
    #[allow(
        clippy::single_call_fn,
        reason = "debug-output matcher construction centralizes exact-pattern escaping"
    )]
    fn new(pattern: &str) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let exact_pattern = exact_debug_pattern(pattern);
        let matcher = Pattern::new_anchored(&exact_pattern).map_err(|error| {
            let boxed_error: Box<dyn Error + Send + Sync> = Box::new(error);
            boxed_error
        })?;
        Ok(Self {
            matcher: Box::new(matcher),
            pattern: pattern.to_owned().into(),
        })
    }

    /// Returns whether this matcher exactly matches debug output.
    #[inline]
    fn debug_matches(&self, field_value: &impl fmt::Debug) -> bool {
        self.matcher.debug_matches(field_value)
    }
}

/// Builds an anchored regular expression that exactly matches debug output.
fn exact_debug_pattern(pattern: &str) -> String {
    let mut escaped = String::with_capacity(pattern.len());
    for character in pattern.chars() {
        if matches!(
            character,
            '\\' | '.'
                | '+'
                | '*'
                | '?'
                | '('
                | ')'
                | '|'
                | '['
                | ']'
                | '{'
                | '}'
                | '^'
                | '$'
                | '#'
                | '&'
                | '-'
                | '~'
        ) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped.push_str("\\z");
    escaped
}

impl fmt::Display for MatchDebug {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&*self.pattern, f)
    }
}

impl AsRef<str> for MatchDebug {
    #[inline]
    fn as_ref(&self) -> &str {
        self.pattern.as_ref()
    }
}

impl PartialEq for MatchDebug {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.pattern == other.pattern
    }
}

impl Eq for MatchDebug {}

impl PartialOrd for MatchDebug {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for MatchDebug {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        self.pattern.cmp(&other.pattern)
    }
}

// === impl BadName ===

impl Error for BadName {}

impl fmt::Display for BadName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid field name `{}`", self.name)
    }
}

impl CallsiteMatch {
    /// Creates a per-span matcher from this callsite matcher.
    pub(crate) fn to_span_match(&self) -> SpanMatch {
        let fields = self
            .fields
            .iter()
            .map(|(field, value_match)| (*field, (value_match.clone(), AtomicBool::new(false))))
            .collect();
        SpanMatch {
            fields,
            level: self.level,
            has_matched: AtomicBool::new(false),
        }
    }
}

impl SpanMatch {
    /// Returns a visitor that records fields into this matcher.
    pub(crate) const fn visitor(&self) -> MatchVisitor<'_> {
        MatchVisitor { inner: self }
    }

    #[inline]
    /// Returns whether every field value has matched.
    pub(crate) fn is_matched(&self) -> bool {
        if self.has_matched.load(Acquire) {
            return true;
        }
        self.is_matched_slow()
    }

    #[inline(never)]
    /// Computes whether every field value has matched.
    fn is_matched_slow(&self) -> bool {
        let matched = self
            .fields
            .values()
            .all(|entry| entry.1.load(Acquire));
        if matched {
            self.has_matched.store(true, Release);
        }
        matched
    }

    #[inline]
    /// Returns the enabled level if every field value has matched.
    #[allow(
        clippy::single_call_fn,
        reason = "span field matching exposes the level query used by dynamic directive evaluation"
    )]
    pub(crate) fn filter(&self) -> Option<LevelFilter> {
        self.is_matched().then_some(self.level)
    }
}

impl Visit for MatchVisitor<'_> {
    fn record_f64(&mut self, field: &Field, field_value: f64) {
        let Some(entry) = self.inner.fields.get(field) else {
            return;
        };
        let expected = &entry.0;
        let matched = &entry.1;
        if expected.matches_f64(field_value) {
            matched.store(true, Release);
        }
    }

    fn record_i64(&mut self, field: &Field, field_value: i64) {
        let Some(entry) = self.inner.fields.get(field) else {
            return;
        };
        let expected = &entry.0;
        let matched = &entry.1;
        if expected.matches_i64(field_value) {
            matched.store(true, Release);
        }
    }

    fn record_u64(&mut self, field: &Field, field_value: u64) {
        let Some(entry) = self.inner.fields.get(field) else {
            return;
        };
        let expected = &entry.0;
        let matched = &entry.1;
        if expected.matches_u64(field_value) {
            matched.store(true, Release);
        }
    }

    fn record_bool(&mut self, field: &Field, field_value: bool) {
        let Some(entry) = self.inner.fields.get(field) else {
            return;
        };
        let expected = &entry.0;
        let matched = &entry.1;
        if expected.matches_bool(field_value) {
            matched.store(true, Release);
        }
    }

    fn record_str(&mut self, field: &Field, field_value: &str) {
        let Some(entry) = self.inner.fields.get(field) else {
            return;
        };
        let expected = &entry.0;
        let matched = &entry.1;
        if expected.matches_str(field_value) {
            matched.store(true, Release);
        }
    }

    fn record_debug(&mut self, field: &Field, field_value: &dyn fmt::Debug) {
        let Some(entry) = self.inner.fields.get(field) else {
            return;
        };
        let expected = &entry.0;
        let matched = &entry.1;
        if expected.matches_debug(&field_value) {
            matched.store(true, Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use strict_test_support::{TestFailure, ensure, ensure_some};
    use tracing_core::callsite::Callsite;
    use tracing_core::field::FieldSet;
    use tracing_core::metadata::Kind;
    use tracing_core::metadata::Metadata;
    use tracing_core::metadata::SourceLocation;
    use tracing_core::subscriber::Interest;
    use tracing_core::Level;

    struct MyStruct {
        answer: usize,
        question: &'static str,
    }

    impl fmt::Debug for MyStruct {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("MyStruct")
                .field("answer", &self.answer)
                .field("question", &self.question)
                .finish()
        }
    }

    struct FieldMatcherCallsite;

    static FIELD_MATCHER_CALLSITE: FieldMatcherCallsite = FieldMatcherCallsite;
    static FIELD_MATCHER_FIELDS: FieldSet = FieldSet::new(
        &[
            "enabled", "signed", "converted", "count", "float", "nan", "name", "debug",
        ],
        tracing_core::identify_callsite!(&FIELD_MATCHER_CALLSITE),
    );

    impl Callsite for FieldMatcherCallsite {
        fn set_interest(&self, _: Interest) {}

        fn metadata(&self) -> &Metadata<'_> {
            static META: Metadata<'static> = Metadata::new(
                "field_matcher_test",
                "field_matcher_target",
                Level::INFO,
                &SourceLocation::empty(),
                &FIELD_MATCHER_FIELDS,
                Kind::SPAN,
            );
            &META
        }
    }

    fn matcher_field(name: &str) -> Result<Field, TestFailure> {
        ensure_some(
            FIELD_MATCHER_FIELDS.field(name),
            "field matcher fixture should contain requested field",
        )
    }

    fn parse_field_matcher(source: &str, regex: bool) -> Result<Match, TestFailure> {
        ensure_some(
            Match::parse(source, regex).ok(),
            "field matcher source should parse",
        )
    }

    fn parse_regex_matcher(source: &str) -> Result<ValueMatch, TestFailure> {
        ensure_some(
            ValueMatch::parse_regex(source).ok(),
            "regex value matcher should parse",
        )
    }

    fn parse_exact_matcher(source: &str) -> Result<ValueMatch, TestFailure> {
        ensure_some(
            ValueMatch::parse_non_regex(source).ok(),
            "exact value matcher should parse",
        )
    }

    fn insert_matcher(
        field_matchers: &mut FieldMap<ValueMatch>,
        field: &Field,
        matcher: ValueMatch,
    ) -> Result<(), TestFailure> {
        ensure(
            field_matchers.insert(*field, matcher).is_none(),
            "field matcher fixtures should use each field only once",
        )
    }

    #[test]
    fn match_parser_orders_value_matchers_after_name_only_matchers() -> Result<(), TestFailure> {
        let plain_matcher = parse_field_matcher("plain_field", true)?;
        let bool_matcher = parse_field_matcher("plain_field=true", true)?;

        ensure(!plain_matcher.has_value(), "plain field matcher has no value matcher")?;
        ensure(bool_matcher.has_value(), "field value matcher reports its value matcher")?;
        ensure(
            plain_matcher < bool_matcher,
            "value-bearing field matchers should sort after name-only matchers",
        )?;
        ensure(
            format!("{bool_matcher}") == "plain_field=true",
            "field matcher display should preserve field name and value",
        )
    }

    #[test]
    fn value_matchers_accept_matching_literals_and_reject_wrong_types() -> Result<(), TestFailure> {
        let bool_matcher = parse_regex_matcher("true")?;
        ensure(bool_matcher.matches_bool(true), "bool matcher accepts the matching bool")?;
        ensure(!bool_matcher.matches_i64(1), "bool matcher rejects numeric fields")?;

        let unsigned_matcher = parse_regex_matcher("7")?;
        ensure(unsigned_matcher.matches_u64(7), "unsigned matcher accepts matching u64")?;
        ensure(
            unsigned_matcher.matches_i64(7),
            "unsigned matcher accepts non-negative signed values with the same magnitude",
        )?;
        ensure(
            !unsigned_matcher.matches_i64(-7),
            "unsigned matcher rejects negative signed values",
        )?;

        let signed_matcher = parse_regex_matcher("-7")?;
        ensure(signed_matcher.matches_i64(-7), "signed matcher accepts matching i64")?;
        ensure(!signed_matcher.matches_u64(7), "signed matcher rejects unsigned fields")?;

        let float_matcher = parse_regex_matcher("1.5")?;
        ensure(float_matcher.matches_f64(1.5), "float matcher accepts matching f64")?;
        ensure(!float_matcher.matches_f64(2.5), "float matcher rejects another f64")?;

        let nan_matcher = parse_regex_matcher("NaN")?;
        ensure(nan_matcher.matches_f64(f64::NAN), "NaN matcher accepts NaN")?;
        ensure(!nan_matcher.matches_f64(1.0), "NaN matcher rejects finite f64 fields")
    }

    #[test]
    fn regex_and_exact_debug_matchers_have_distinct_string_polarity() -> Result<(), TestFailure> {
        let regex_matcher = parse_regex_matcher("alice.*")?;
        let exact_matcher = parse_exact_matcher("alice.*")?;

        ensure(
            regex_matcher.matches_str("alice-bob"),
            "regex matcher should accept matching string values",
        )?;
        ensure(
            !exact_matcher.matches_str("alice-bob"),
            "exact debug matcher should reject regex-only string values",
        )?;

        let debug_matcher = ensure_some(
            MatchDebug::new("MyStruct { answer: 42, question: \"life\" }").ok(),
            "exact debug matcher should compile",
        )?;
        let matching_struct = MyStruct {
            answer: 42,
            question: "life",
        };
        ensure(
            debug_matcher.debug_matches(&matching_struct),
            "exact debug matcher should accept matching debug output",
        )?;

        let exact_pattern = exact_debug_pattern("a.b#c");
        ensure(
            exact_pattern == "a\\.b\\#c\\z",
            "exact debug patterns should escape regex metacharacters and anchor the end",
        )
    }

    #[test]
    fn span_match_records_all_expected_fields_before_releasing_level() -> Result<(), TestFailure> {
        let enabled_field = matcher_field("enabled")?;
        let signed_field = matcher_field("signed")?;
        let converted_field = matcher_field("converted")?;
        let count_field = matcher_field("count")?;
        let float_field = matcher_field("float")?;
        let nan_field = matcher_field("nan")?;
        let name_field = matcher_field("name")?;
        let debug_field = matcher_field("debug")?;

        let mut field_matchers = FieldMap::default();
        insert_matcher(&mut field_matchers, &enabled_field, ValueMatch::Bool(true))?;
        insert_matcher(&mut field_matchers, &signed_field, ValueMatch::I64(-3))?;
        insert_matcher(&mut field_matchers, &converted_field, ValueMatch::U64(7))?;
        insert_matcher(&mut field_matchers, &count_field, ValueMatch::U64(11))?;
        insert_matcher(&mut field_matchers, &float_field, ValueMatch::F64(1.5))?;
        insert_matcher(&mut field_matchers, &nan_field, ValueMatch::NaN)?;
        insert_matcher(
            &mut field_matchers,
            &name_field,
            parse_regex_matcher("alice.*")?,
        )?;
        insert_matcher(
            &mut field_matchers,
            &debug_field,
            parse_exact_matcher("MyStruct { answer: 42, question: \"life\" }")?,
        )?;
        let callsite_match = CallsiteMatch {
            fields: field_matchers,
            level: LevelFilter::DEBUG,
        };
        let span_match = callsite_match.to_span_match();

        ensure(!span_match.is_matched(), "new span matcher should not be matched")?;
        ensure(
            span_match.filter().is_none(),
            "unmatched span matcher should not release a level",
        )?;

        let mut visitor = span_match.visitor();
        visitor.record_bool(&enabled_field, false);
        visitor.record_i64(&signed_field, -3);
        visitor.record_i64(&converted_field, 7);
        visitor.record_u64(&count_field, 11);
        visitor.record_f64(&float_field, 1.5);
        visitor.record_f64(&nan_field, f64::NAN);
        visitor.record_str(&name_field, "alice-bob");
        let matching_struct = MyStruct {
            answer: 42,
            question: "life",
        };
        visitor.record_debug(&debug_field, &matching_struct);
        ensure(
            !span_match.is_matched(),
            "one wrong field should keep the span matcher unmatched",
        )?;

        visitor.record_bool(&enabled_field, true);
        ensure(span_match.is_matched(), "all matching fields should satisfy the span matcher")?;
        ensure(
            span_match.filter() == Some(LevelFilter::DEBUG),
            "matched span matcher should release its configured level",
        )
    }

    #[test]
    fn value_match_ordering_and_equality_are_stable_by_type_then_value() -> Result<(), TestFailure> {
        let bool_false = ValueMatch::Bool(false);
        let bool_true = ValueMatch::Bool(true);
        let f64_one = ValueMatch::F64(1.0);
        let f64_two = ValueMatch::F64(2.0);
        let unsigned_one = ValueMatch::U64(1);
        let unsigned_two = ValueMatch::U64(2);
        let signed_negative = ValueMatch::I64(-1);
        let signed_positive = ValueMatch::I64(1);
        let debug_alpha = parse_exact_matcher("alpha")?;
        let debug_beta = parse_exact_matcher("beta")?;
        let pattern_alpha = parse_regex_matcher("alpha.*")?;
        let pattern_beta = parse_regex_matcher("beta.*")?;

        ensure(bool_false < bool_true, "bool matchers order by bool value")?;
        ensure(bool_true < f64_one, "bool matchers sort before f64 matchers")?;
        ensure(f64_one < f64_two, "f64 matchers order by numeric value")?;
        ensure(f64_two < ValueMatch::NaN, "finite f64 matchers sort before NaN")?;
        ensure(ValueMatch::NaN < unsigned_one, "NaN matcher sorts before unsigned matchers")?;
        ensure(unsigned_one < unsigned_two, "unsigned matchers order by numeric value")?;
        ensure(unsigned_two < signed_negative, "unsigned matchers sort before signed matchers")?;
        ensure(signed_negative < signed_positive, "signed matchers order by numeric value")?;
        ensure(signed_positive < debug_alpha, "signed matchers sort before exact debug matchers")?;
        ensure(debug_alpha < debug_beta, "exact debug matchers order by pattern")?;
        ensure(debug_beta < pattern_alpha, "exact debug matchers sort before regex matchers")?;
        ensure(pattern_alpha < pattern_beta, "regex matchers order by pattern")?;
        ensure(ValueMatch::NaN == ValueMatch::NaN, "NaN matchers compare equal by matcher kind")?;
        ensure(
            ValueMatch::Bool(true) != ValueMatch::U64(1),
            "matchers of different kinds are not equal",
        )
    }

    #[test]
    fn field_match_and_value_displays_preserve_user_directive_text() -> Result<(), TestFailure> {
        let exact = ensure_some(
            MatchDebug::new("alpha.*").ok(),
            "exact debug matcher should compile",
        )?;
        let pattern = ensure_some(
            "alpha.*".parse::<MatchPattern>().ok(),
            "regex matcher should compile",
        )?;

        ensure(format!("{exact}") == "alpha.*", "exact debug display preserves the pattern")?;
        ensure(exact.as_ref() == "alpha.*", "exact debug as_ref exposes the pattern")?;
        ensure(format!("{pattern}") == "alpha.*", "regex display preserves the pattern")?;
        ensure(pattern.as_ref() == "alpha.*", "regex as_ref exposes the pattern")?;

        let exact_beta = ensure_some(MatchDebug::new("beta").ok(), "second exact matcher compiles")?;
        let pattern_beta = ensure_some(
            "beta.*".parse::<MatchPattern>().ok(),
            "second regex matcher compiles",
        )?;
        ensure(exact < exact_beta, "exact debug matchers order by pattern")?;
        ensure(pattern < pattern_beta, "regex matchers order by pattern")?;

        let nan_matcher = parse_regex_matcher("NaN")?;
        ensure(format!("{nan_matcher}") == "NaN", "NaN display round-trips through f64 display")?;

        let field_matcher = Match {
            name: "field_name".to_owned(),
            value: Some(ValueMatch::Debug(Box::new(exact_beta))),
        };
        ensure(
            field_matcher.name() == "field_name",
            "field matcher name clones the configured field name",
        )?;
        ensure(
            format!("{field_matcher}") == "field_name=beta",
            "field matcher display combines field name and value",
        )
    }

    #[test]
    fn field_matcher_parsing_reports_invalid_regex_and_bad_names() -> Result<(), TestFailure> {
        ensure(
            Match::parse("field=[", true).is_err(),
            "regex field matchers reject invalid regex patterns",
        )?;

        let bad_name = BadName {
            name: "bad field".to_owned(),
        };
        ensure(
            format!("{bad_name}") == "invalid field name `bad field`",
            "bad field names have a stable diagnostic",
        )
    }

    #[test]
    fn span_match_visitors_ignore_unregistered_fields_and_wrong_value_types() -> Result<(), TestFailure> {
        let enabled_field = matcher_field("enabled")?;
        let count_field = matcher_field("count")?;
        let unused_field = matcher_field("signed")?;

        let mut field_matchers = FieldMap::default();
        insert_matcher(&mut field_matchers, &enabled_field, ValueMatch::Bool(true))?;
        insert_matcher(&mut field_matchers, &count_field, ValueMatch::U64(5))?;
        let span_match = CallsiteMatch {
            fields: field_matchers,
            level: LevelFilter::TRACE,
        }
        .to_span_match();

        let mut visitor = span_match.visitor();
        visitor.record_bool(&unused_field, true);
        visitor.record_f64(&unused_field, 5.0);
        visitor.record_i64(&unused_field, 5);
        visitor.record_u64(&unused_field, 5);
        visitor.record_str(&unused_field, "true");
        visitor.record_debug(&unused_field, &true);
        visitor.record_str(&enabled_field, "true");
        visitor.record_debug(&count_field, &5_u64);
        ensure(
            !span_match.is_matched(),
            "unregistered fields and wrong value types do not satisfy matchers",
        )?;

        visitor.record_bool(&enabled_field, true);
        ensure(!span_match.is_matched(), "one matching field is insufficient")?;
        visitor.record_u64(&count_field, 5);
        ensure(span_match.is_matched(), "matching registered fields satisfy the span matcher")
    }

    #[test]
    fn debug_struct_match() -> Result<(), TestFailure> {
        let my_struct = MyStruct {
            answer: 42,
            question: "life, the universe, and everything",
        };

        let pattern = "MyStruct { answer: 42, question: \"life, the universe, and everything\" }";

        ensure(
            format!("{my_struct:?}") == pattern,
            "`MyStruct`'s `Debug` impl outputs the expected matching string",
        )?;

        let matcher = ensure_some(MatchDebug::new(pattern).ok(), "debug pattern should compile")?;
        ensure(
            matcher.debug_matches(&my_struct),
            "debug matcher accepts matching struct",
        )
    }

    #[test]
    fn debug_struct_not_match() -> Result<(), TestFailure> {
        let my_struct = MyStruct {
            answer: 42,
            question: "what shall we have for lunch?",
        };

        let pattern = "MyStruct { answer: 42, question: \"life, the universe, and everything\" }";

        ensure(
            format!("{my_struct:?}")
                == "MyStruct { answer: 42, question: \"what shall we have for lunch?\" }",
            "`MyStruct`'s `Debug` impl outputs the expected non-matching string",
        )?;

        let matcher = ensure_some(MatchDebug::new(pattern).ok(), "debug pattern should compile")?;
        ensure(
            !matcher.debug_matches(&my_struct),
            "debug matcher rejects non-matching struct",
        )
    }
}
