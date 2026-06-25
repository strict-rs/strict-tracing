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
        let value = parts
            .next()
            .map(|part| {
                if regex {
                    ValueMatch::parse_regex(part)
                } else {
                    ValueMatch::parse_non_regex(part)
                }
            })
            .transpose()?;
        Ok(Self { name, value })
    }
}

impl fmt::Display for Match {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.name, f)?;
        if let Some(ref value) = self.value {
            write!(f, "={value}")?;
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
    fn matches_f64(&self, value: f64) -> bool {
        match *self {
            Self::NaN => value.is_nan(),
            Self::F64(expected) => value.total_cmp(&expected).is_eq(),
            Self::Bool(_) | Self::U64(_) | Self::I64(_) | Self::Debug(_) | Self::Pat(_) => false,
        }
    }

    /// Returns whether this matcher accepts an `i64` field value.
    fn matches_i64(&self, value: i64) -> bool {
        use std::convert::TryFrom as _;

        match *self {
            Self::I64(expected) => value == expected,
            Self::U64(expected) => u64::try_from(value).is_ok_and(|actual| actual == expected),
            Self::Bool(_) | Self::F64(_) | Self::NaN | Self::Debug(_) | Self::Pat(_) => false,
        }
    }

    /// Returns whether this matcher accepts a `u64` field value.
    const fn matches_u64(&self, value: u64) -> bool {
        match *self {
            Self::U64(expected) => value == expected,
            Self::Bool(_) | Self::F64(_) | Self::I64(_) | Self::NaN | Self::Debug(_) | Self::Pat(_) => false,
        }
    }

    /// Returns whether this matcher accepts a `bool` field value.
    const fn matches_bool(&self, value: bool) -> bool {
        match *self {
            Self::Bool(expected) => value == expected,
            Self::F64(_) | Self::U64(_) | Self::I64(_) | Self::NaN | Self::Debug(_) | Self::Pat(_) => false,
        }
    }

    /// Returns whether this matcher accepts a string field value.
    fn matches_str(&self, value: &str) -> bool {
        match *self {
            Self::Pat(ref expected) => expected.str_matches(&value),
            Self::Debug(ref expected) => expected.debug_matches(&value),
            Self::Bool(_) | Self::F64(_) | Self::U64(_) | Self::I64(_) | Self::NaN => false,
        }
    }

    /// Returns whether this matcher accepts a debug field value.
    fn matches_debug(&self, value: &dyn fmt::Debug) -> bool {
        match *self {
            Self::Pat(ref expected) => expected.debug_matches(&value),
            Self::Debug(ref expected) => expected.debug_matches(&value),
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
    fn str_matches(&self, value: &impl AsRef<str>) -> bool {
        self.matcher.matches(value)
    }

    /// Returns whether this pattern matches debug output.
    #[inline]
    fn debug_matches(&self, value: &impl fmt::Debug) -> bool {
        self.matcher.debug_matches(value)
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
    fn debug_matches(&self, value: &impl fmt::Debug) -> bool {
        self.matcher.debug_matches(value)
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
            .map(|(field, value)| (*field, (value.clone(), AtomicBool::new(false))))
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
    fn record_f64(&mut self, field: &Field, value: f64) {
        let Some(entry) = self.inner.fields.get(field) else {
            return;
        };
        let expected = &entry.0;
        let matched = &entry.1;
        if expected.matches_f64(value) {
            matched.store(true, Release);
        }
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        let Some(entry) = self.inner.fields.get(field) else {
            return;
        };
        let expected = &entry.0;
        let matched = &entry.1;
        if expected.matches_i64(value) {
            matched.store(true, Release);
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        let Some(entry) = self.inner.fields.get(field) else {
            return;
        };
        let expected = &entry.0;
        let matched = &entry.1;
        if expected.matches_u64(value) {
            matched.store(true, Release);
        }
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        let Some(entry) = self.inner.fields.get(field) else {
            return;
        };
        let expected = &entry.0;
        let matched = &entry.1;
        if expected.matches_bool(value) {
            matched.store(true, Release);
        }
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        let Some(entry) = self.inner.fields.get(field) else {
            return;
        };
        let expected = &entry.0;
        let matched = &entry.1;
        if expected.matches_str(value) {
            matched.store(true, Release);
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        let Some(entry) = self.inner.fields.get(field) else {
            return;
        };
        let expected = &entry.0;
        let matched = &entry.1;
        if expected.matches_debug(&value) {
            matched.store(true, Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use strict_test_support::{TestFailure, ensure, ensure_some};

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
