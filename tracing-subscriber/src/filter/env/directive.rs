use crate::filter::directive::{FilterVec, ParseError, StaticDirective};
use crate::filter::{
    directive::{DirectiveSet, Match},
    env::{field, FieldMap},
    level::LevelFilter,
};
use alloc::{borrow::ToOwned as _, boxed::Box, string::String, vec::Vec};
use std::{
    cmp::Ordering,
    fmt,
    iter::FromIterator as _,
    str::{CharIndices, FromStr},
};
use tracing_core::{span, Level, Metadata};

/// A single filtering directive.
// TODO(eliza): add a builder for programmatically constructing directives?
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(docsrs, doc(cfg(feature = "env-filter")))]
pub struct Directive {
    /// The span name matched by this directive.
    in_span: Option<String>,
    /// Field matchers required by this directive.
    fields: Vec<field::Match>,
    /// The optional target prefix matched by this directive.
    pub(crate) target: Option<String>,
    /// The maximum level enabled by this directive.
    pub(crate) level: LevelFilter,
}

/// A set of dynamic filtering directives.
pub(super) type Dynamics = DirectiveSet<Directive>;

/// A set of static filtering directives.
pub(super) type Statics = DirectiveSet<StaticDirective>;

/// Dynamic matchers associated with a registered callsite.
pub(super) type CallsiteMatcher = MatchSet<field::CallsiteMatch>;
/// Dynamic matchers associated with a span instance.
pub(super) type SpanMatcher = MatchSet<field::SpanMatch>;

/// Result of matching dynamic directives against a callsite.
pub(super) enum CallsiteMatchResult {
    /// One or more directives match the callsite.
    Matched(Box<CallsiteMatcher>),
    /// A directive matched the target/span but required fields the callsite does not define.
    Rejected,
    /// No directive matched the callsite target/span.
    Unmatched,
}

/// A set of field matchers and the fallback level for unmatched fields.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct MatchSet<T> {
    /// Field-value matchers for matching dynamic directives.
    field_matches: FilterVec<T>,
    /// The level enabled when no field-value matcher applies.
    base_level: LevelFilter,
}

impl Directive {
    /// Returns whether this directive matches a span by name.
    pub(super) const fn has_name(&self) -> bool {
        self.in_span.is_some()
    }

    /// Returns whether this directive matches any fields.
    pub(super) const fn has_fields(&self) -> bool {
        !self.fields.is_empty()
    }

    /// Converts this directive to a static directive when possible.
    #[allow(
        clippy::single_call_fn,
        reason = "directive table construction keeps dynamic-to-static promotion explicit"
    )]
    pub(super) fn to_static(&self) -> Option<StaticDirective> {
        if !self.is_static() {
            return None;
        }

        // TODO(eliza): these strings are all immutable; we should consider
        // `Arc`ing them to make this more efficient...
        let field_names = self.fields.iter().map(field::Match::name).collect();

        Some(StaticDirective::new(
            self.target.clone(),
            field_names,
            self.level,
        ))
    }

    /// Returns whether this directive can be evaluated statically.
    fn is_static(&self) -> bool {
        !self.has_name() && !self.fields.iter().any(field::Match::has_value)
    }

    /// Returns whether this directive's target and span-name filters match.
    fn matches_target_and_span(&self, meta: &Metadata<'_>) -> bool {
        if let Some(target) = self.target.as_deref()
            && !meta.target().starts_with(target)
        {
            return false;
        }

        if let Some(name) = self.in_span.as_deref()
            && name != meta.name()
        {
            return false;
        }

        true
    }

    /// Returns whether this directive's field filters match the callsite fields.
    fn fields_match(&self, meta: &Metadata<'_>) -> bool {
        let actual_fields = meta.fields();
        for expected_field in &self.fields {
            if actual_fields.field(&expected_field.name).is_none() {
                return false;
            }
        }

        true
    }

    /// Returns whether this directive requires dynamic span context.
    #[allow(
        clippy::single_call_fn,
        reason = "directive table construction names the dynamic partition predicate"
    )]
    pub(super) const fn is_dynamic(&self) -> bool {
        self.has_name() || self.has_fields()
    }

    /// Creates a callsite field matcher for this directive and metadata.
    fn field_matcher(&self, meta: &Metadata<'_>) -> Option<field::CallsiteMatch> {
        let fieldset = meta.fields();
        let fields = self
            .fields
            .iter()
            .filter_map(|field_match| {
                if let Some(actual_field) = fieldset.field(&field_match.name) {
                    let expected_value = field_match.value.clone()?;
                    Some(Ok((actual_field, expected_value)))
                } else {
                    Some(Err(()))
                }
            })
            .collect::<Result<FieldMap<_>, ()>>()
            .ok()?;
        Some(field::CallsiteMatch {
            fields,
            level: self.level,
        })
    }

    /// Splits dynamic and static directives into their lookup tables.
    #[allow(
        clippy::single_call_fn,
        reason = "environment filter construction keeps directive table assembly separate from parsing"
    )]
    pub(super) fn make_tables(
        directives: impl IntoIterator<Item = Self>,
    ) -> (Dynamics, Statics) {
        // TODO(eliza): this could be made more efficient...
        let (dyns, stats): (Vec<Self>, Vec<Self>) =
            directives.into_iter().partition(Self::is_dynamic);
        let statics = stats
            .into_iter()
            .filter_map(|directive| directive.to_static())
            .chain(dyns.iter().filter_map(Self::to_static))
            .collect();
        (Dynamics::from_iter(dyns), statics)
    }

    /// Converts regex value matchers to exact debug-output matchers.
    pub(super) fn deregexify(&mut self) {
        for field_match in &mut self.fields {
            field_match.value = match field_match.value.take() {
                Some(field::ValueMatch::Pat(pat)) => {
                    Some(field::ValueMatch::Debug(Box::new(pat.into_debug_match())))
                }
                existing => existing,
            }
        }
    }

    /// Parses a single directive from a string.
    pub(super) fn parse(from: &str, regex: bool) -> Result<Self, ParseError> {
        let mut directive = Self {
            level: LevelFilter::TRACE,
            target: None,
            in_span: None,
            fields: Vec::new(),
        };

        let mut parse_state = DirectiveParseState::Start;
        for (position, token) in from.trim().char_indices() {
            parse_state =
                transition_parse_state(&mut directive, parse_state, from, position, token, regex)?;
        }

        finish_parse_state(&mut directive, parse_state, from)?;

        Ok(directive)
    }

    /// Writes the target component and returns whether anything was written.
    fn fmt_target(&self, formatter: &mut fmt::Formatter<'_>) -> Result<bool, fmt::Error> {
        if let Some(ref target) = self.target {
            fmt::Display::fmt(target, formatter)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

/// Returns directive-list components split on commas outside field lists.
pub(super) fn split_directives(source: &str) -> DirectiveParts<'_> {
    DirectiveParts {
        source,
        chars: Some(source.char_indices()),
        start: 0,
        state: DirectivePartState::Base,
    }
}

/// Location state while splitting a directive list.
#[derive(Debug, Copy, Clone)]
enum DirectivePartState {
    /// The iterator is outside square-bracketed span and field filters.
    Base,
    /// The iterator is inside a square-bracketed span and field filter.
    Bracketed,
    /// The iterator is inside a braced field list.
    FieldList,
}

/// Iterator over comma-separated directive-list components.
#[derive(Debug)]
pub(super) struct DirectiveParts<'a> {
    /// The complete directive-list source.
    source: &'a str,
    /// Character iterator over the source with byte positions.
    chars: Option<CharIndices<'a>>,
    /// Byte position where the next directive component starts.
    start: usize,
    /// The directive-list location state.
    state: DirectivePartState,
}

impl<'a> Iterator for DirectiveParts<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        let chars = self.chars.as_mut()?;

        for (position, token) in chars.by_ref() {
            match (self.state, token) {
                (DirectivePartState::Base, '[') | (DirectivePartState::FieldList, '}') => {
                    self.state = DirectivePartState::Bracketed;
                }
                (DirectivePartState::Bracketed, ']') => {
                    self.state = DirectivePartState::Base;
                }
                (DirectivePartState::Bracketed, '{') => {
                    self.state = DirectivePartState::FieldList;
                }
                (DirectivePartState::Base | DirectivePartState::Bracketed, ',') => {
                    let directive = self.source.get(self.start..position);
                    self.start = position.checked_add(token.len_utf8()).unwrap_or(self.source.len());
                    return directive;
                }
                _ => {}
            }
        }

        self.chars = None;
        self.source.get(self.start..)
    }
}

/// State for parsing a single directive.
#[derive(Debug, Copy, Clone)]
enum DirectiveParseState {
    /// No directive component has been observed.
    Start,
    /// A leading component that may become either a level or target.
    LevelOrTarget {
        /// Byte position where the component starts.
        start: usize,
    },
    /// A span-name component inside square brackets.
    Span {
        /// Byte position where the span name starts.
        span_start: usize,
    },
    /// A field matcher inside braces.
    Field {
        /// Byte position where the field matcher starts.
        field_start: usize,
    },
    /// The parser has completed a field-list component.
    Fields,
    /// The parser has completed the target/span component and may see a level.
    Target,
    /// A level component after `=`.
    Level {
        /// Byte position where the level starts.
        level_start: usize,
    },
    /// The parser has accepted a complete directive.
    Complete,
}

/// Returns the byte position immediately after a parsed character.
fn after_char(position: usize, token: char) -> Result<usize, ParseError> {
    position
        .checked_add(token.len_utf8())
        .ok_or_else(ParseError::new)
}

/// Returns a directive substring known by the parser state.
fn directive_part(
    source: &str,
    start: usize,
    end: usize,
) -> Result<&str, ParseError> {
    source.get(start..end).ok_or_else(ParseError::new)
}

/// Returns a directive substring from a parser position through the end.
fn directive_part_from(source: &str, start: usize) -> Result<&str, ParseError> {
    source.get(start..).ok_or_else(ParseError::new)
}

/// Parses a component that may be either a bare level or a target.
fn parse_level_or_target(
    level_or_target: &str,
) -> Result<(LevelFilter, Option<String>), ParseError> {
    if level_or_target.is_empty() {
        return Ok((LevelFilter::TRACE, None));
    }

    LevelFilter::from_str(level_or_target).map_or_else(
        |_| Ok((LevelFilter::TRACE, Some(level_or_target.to_owned()))),
        |level| Ok((level, None)),
    )
}

/// Parses a level component, defaulting an empty component to `TRACE`.
fn parse_level(level: &str) -> Result<LevelFilter, ParseError> {
    if level.is_empty() {
        Ok(LevelFilter::TRACE)
    } else {
        LevelFilter::from_str(level).map_err(Into::into)
    }
}

/// Pushes a field matcher parsed from a directive substring.
fn push_field_match(
    directive: &mut Directive,
    source: &str,
    start: usize,
    end: usize,
    regex: bool,
) -> Result<(), ParseError> {
    let field_match = directive_part(source, start, end)?;
    if field_match.is_empty() {
        return Err(ParseError::new());
    }
    directive.fields.push(field::Match::parse(field_match, regex)?);
    Ok(())
}

/// Advances parsing from the initial directive state.
#[allow(
    clippy::single_call_fn,
    reason = "directive parser state machine names the start-state transition separately from token dispatch"
)]
fn transition_start_state(position: usize, token: char) -> Result<DirectiveParseState, ParseError> {
    if token == '[' {
        return Ok(DirectiveParseState::Span {
            span_start: after_char(position, token)?,
        });
    }
    if !['-', ':', '_'].contains(&token) && !token.is_alphanumeric() {
        return Err(ParseError::new());
    }
    Ok(DirectiveParseState::LevelOrTarget { start: position })
}

/// Advances parsing while a level or target prefix is being read.
#[allow(
    clippy::single_call_fn,
    reason = "directive parser state machine names the ambiguous level-or-target transition"
)]
fn transition_level_or_target_state(
    directive: &mut Directive,
    start: usize,
    source: &str,
    position: usize,
    token: char,
) -> Result<DirectiveParseState, ParseError> {
    match token {
        '=' => {
            directive.target = Some(directive_part(source, start, position)?.to_owned());
            Ok(DirectiveParseState::Level {
                level_start: after_char(position, token)?,
            })
        }
        '[' => {
            directive.target = Some(directive_part(source, start, position)?.to_owned());
            Ok(DirectiveParseState::Span {
                span_start: after_char(position, token)?,
            })
        }
        ',' => {
            let (level, target) = parse_level_or_target(directive_part_from(source, start)?)?;
            directive.level = level;
            directive.target = target;
            Ok(DirectiveParseState::Complete)
        }
        _ => Ok(DirectiveParseState::LevelOrTarget { start }),
    }
}

/// Advances parsing after an explicit target has been read.
#[allow(
    clippy::single_call_fn,
    reason = "directive parser state machine names the explicit target-to-level transition"
)]
fn transition_target_state(position: usize, token: char) -> Result<DirectiveParseState, ParseError> {
    if token == '=' {
        return Ok(DirectiveParseState::Level {
            level_start: after_char(position, token)?,
        });
    }
    Err(ParseError::new())
}

/// Advances parsing while a span name is being read.
#[allow(
    clippy::single_call_fn,
    reason = "directive parser state machine names span-name parsing and field-block entry"
)]
fn transition_span_state(
    directive: &mut Directive,
    span_start: usize,
    source: &str,
    position: usize,
    token: char,
) -> Result<DirectiveParseState, ParseError> {
    match token {
        ']' => {
            directive.in_span = Some(directive_part(source, span_start, position)?.to_owned());
            Ok(DirectiveParseState::Target)
        }
        '{' => {
            let span = directive_part(source, span_start, position)?;
            directive.in_span = (!span.is_empty()).then(|| span.to_owned());
            Ok(DirectiveParseState::Field {
                field_start: after_char(position, token)?,
            })
        }
        _ => Ok(DirectiveParseState::Span { span_start }),
    }
}

/// Advances parsing while a field matcher is being read.
#[allow(
    clippy::single_call_fn,
    reason = "directive parser state machine names field matcher accumulation and separator handling"
)]
fn transition_field_state(
    directive: &mut Directive,
    field_start: usize,
    source: &str,
    position: usize,
    token: char,
    regex: bool,
) -> Result<DirectiveParseState, ParseError> {
    match token {
        '}' => {
            push_field_match(directive, source, field_start, position, regex)?;
            Ok(DirectiveParseState::Fields)
        }
        ',' => {
            push_field_match(directive, source, field_start, position, regex)?;
            Ok(DirectiveParseState::Field {
                field_start: after_char(position, token)?,
            })
        }
        _ => Ok(DirectiveParseState::Field { field_start }),
    }
}

/// Advances parsing after at least one field matcher has been read.
#[allow(
    clippy::single_call_fn,
    reason = "directive parser state machine names post-field-block closure validation"
)]
const fn transition_fields_state(token: char) -> Result<DirectiveParseState, ParseError> {
    if token == ']' {
        return Ok(DirectiveParseState::Target);
    }
    Err(ParseError::new())
}

/// Advances parsing while an explicit level is being read.
#[allow(
    clippy::single_call_fn,
    reason = "directive parser state machine names explicit level parsing completion"
)]
fn transition_level_state(
    directive: &mut Directive,
    level_start: usize,
    source: &str,
    position: usize,
    token: char,
) -> Result<DirectiveParseState, ParseError> {
    if token == ',' {
        directive.level = parse_level(directive_part(source, level_start, position)?)?;
        return Ok(DirectiveParseState::Complete);
    }
    Ok(DirectiveParseState::Level { level_start })
}

/// Advances the directive parser by one character.
#[allow(
    clippy::single_call_fn,
    reason = "the directive parser keeps per-character state transitions isolated for review"
)]
fn transition_parse_state(
    directive: &mut Directive,
    parse_state: DirectiveParseState,
    source: &str,
    position: usize,
    token: char,
    regex: bool,
) -> Result<DirectiveParseState, ParseError> {
  use DirectiveParseState::{Complete, Field, Fields, Level, LevelOrTarget, Span, Start, Target};

  match parse_state {
        Start => transition_start_state(position, token),
        LevelOrTarget { start } => transition_level_or_target_state(directive, start, source, position, token),
        Target => transition_target_state(position, token),
        Span { span_start } => transition_span_state(directive, span_start, source, position, token),
        Field { field_start } => transition_field_state(directive, field_start, source, position, token, regex),
        Fields => transition_fields_state(token),
        Level { level_start } => transition_level_state(directive, level_start, source, position, token),
        Complete => Err(ParseError::new()),
    }
}

/// Completes directive parsing after the final character has been consumed.
#[allow(
    clippy::single_call_fn,
    reason = "the directive parser keeps end-of-input state handling isolated for review"
)]
fn finish_parse_state(
    directive: &mut Directive,
    parse_state: DirectiveParseState,
    source: &str,
) -> Result<(), ParseError> {
    match parse_state {
        DirectiveParseState::LevelOrTarget { start } => {
            let (level, target) = parse_level_or_target(directive_part_from(source, start)?)?;
            directive.level = level;
            directive.target = target;
            Ok(())
        }
        DirectiveParseState::Level { level_start } => {
            directive.level = parse_level(directive_part_from(source, level_start)?)?;
            Ok(())
        }
        DirectiveParseState::Target | DirectiveParseState::Complete => Ok(()),
        DirectiveParseState::Start
        | DirectiveParseState::Span { .. }
        | DirectiveParseState::Field { .. }
        | DirectiveParseState::Fields => Err(ParseError::new()),
    }
}

impl Match for Directive {
    fn cares_about(&self, meta: &Metadata<'_>) -> bool {
        self.matches_target_and_span(meta) && self.fields_match(meta)
    }

    fn level(&self) -> &LevelFilter {
        &self.level
    }
}

impl FromStr for Directive {
    type Err = ParseError;
    fn from_str(from: &str) -> Result<Self, Self::Err> {
        Self::parse(from, true)
    }
}

impl Default for Directive {
    fn default() -> Self {
        Self {
            level: LevelFilter::OFF,
            target: None,
            in_span: None,
            fields: Vec::new(),
        }
    }
}

impl PartialOrd for Directive {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Directive {
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
            // Next compare based on the presence of span names.
            .then_with(|| self.in_span.is_some().cmp(&other.in_span.is_some()))
            // Then we compare how many fields are defined by each
            // directive.
            .then_with(|| self.fields.len().cmp(&other.fields.len()))
            // Finally, we fall back to lexicographical ordering if the directives are
            // equally specific. Although this is no longer semantically important,
            // we need to define a total ordering to determine the directive's place
            // in the BTreeMap.
            .then_with(|| {
                self.target
                    .cmp(&other.target)
                    .then_with(|| self.in_span.cmp(&other.in_span))
                    .then_with(|| self.fields[..].cmp(&other.fields[..]))
            })
            .reverse()
    }
}

impl fmt::Display for Directive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let wrote_target = self.fmt_target(f)?;
        let wrote_span_or_fields = self.has_name() || self.has_fields();

        if wrote_span_or_fields {
            f.write_str("[")?;

            if let Some(ref span) = self.in_span {
                fmt::Display::fmt(span, f)?;
            }

            let mut field_matches = self.fields.iter();
            if let Some(first_field_match) = field_matches.next() {
                write!(f, "{{{first_field_match}")?;
                field_matches.try_for_each(|next_field_match| write!(f, ",{next_field_match}"))?;
                f.write_str("}")?;
            }

            f.write_str("]")?;
        }

        if wrote_target || wrote_span_or_fields {
            f.write_str("=")?;
        }

        fmt::Display::fmt(&self.level, f)
    }
}

impl From<LevelFilter> for Directive {
    fn from(level: LevelFilter) -> Self {
        Self {
            level,
            ..Self::default()
        }
    }
}

impl From<Level> for Directive {
    fn from(level: Level) -> Self {
        LevelFilter::from_level(level).into()
    }
}

// === impl Dynamics ===

impl Dynamics {
    /// Builds a dynamic matcher for the given metadata.
    pub(super) fn matcher(&self, metadata: &Metadata<'_>) -> CallsiteMatchResult {
        let mut fallback_level = None;
        let mut rejected_fields = false;
        let field_matches = self
            .directives()
            .filter_map(|directive| {
                if !directive.matches_target_and_span(metadata) {
                    return None;
                }
                if !directive.fields_match(metadata) {
                    rejected_fields = true;
                    return None;
                }
                if let Some(field_match) = directive.field_matcher(metadata) {
                    return Some(field_match);
                }
                match fallback_level {
                    Some(ref current_level) if directive.level > *current_level => {
                        fallback_level = Some(directive.level);
                    }
                    None => fallback_level = Some(directive.level),
                    _ => {}
                }
                None
            })
            .collect();

        if let Some(base_level) = fallback_level {
            CallsiteMatchResult::Matched(Box::new(CallsiteMatcher {
                field_matches,
                base_level,
            }))
        } else if !field_matches.is_empty() {
            CallsiteMatchResult::Matched(Box::new(CallsiteMatcher {
                field_matches,
                base_level: LevelFilter::OFF,
            }))
        } else if rejected_fields {
            CallsiteMatchResult::Rejected
        } else {
            CallsiteMatchResult::Unmatched
        }
    }

    /// Returns whether any dynamic directive matches field values.
    pub(super) fn has_value_filters(&self) -> bool {
        self.directives()
            .any(|directive| directive.fields.iter().any(|field_match| field_match.value.is_some()))
    }
}

// ===== impl DynamicMatch =====

impl CallsiteMatcher {
    /// Create a new `SpanMatch` for a given instance of the matched callsite.
    pub(crate) fn to_span_match(&self, attrs: &span::Attributes<'_>) -> SpanMatcher {
        let field_matches = self
            .field_matches
            .iter()
            .map(|field_match| {
                let span_match = field_match.to_span_match();
                attrs.record(&mut span_match.visitor());
                span_match
            })
            .collect();
        SpanMatcher {
            field_matches,
            base_level: self.base_level,
        }
    }
}

impl SpanMatcher {
    /// Returns the level currently enabled for this callsite.
    #[allow(
        clippy::single_call_fn,
        reason = "span matcher level resolution is the named query used by environment filters"
    )]
    pub(crate) fn level(&self) -> LevelFilter {
        self.field_matches
            .iter()
            .filter_map(field::SpanMatch::filter)
            .max()
            .unwrap_or(self.base_level)
    }

    /// Records updated span fields against this matcher.
    pub(crate) fn record_update(&self, record: &span::Record<'_>) {
        for field_match in &self.field_matches {
            record.record(&mut field_match.visitor());
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use alloc::{format, vec};
    use strict_test_support::{ensure, ensure_eq, ensure_ok, TestFailure};

    struct DirectiveExpectation {
        target: Option<&'static str>,
        level: LevelFilter,
        in_span: Option<&'static str>,
    }

    fn parse_directives(dirs: impl AsRef<str>) -> Vec<Directive> {
        split_directives(dirs.as_ref())
            .filter_map(|directive| directive.parse().ok())
            .collect()
    }

    fn expect_parse(dirs: impl AsRef<str>) -> Result<Vec<Directive>, TestFailure> {
        split_directives(dirs.as_ref())
            .map(|directive| ensure_ok(directive.parse(), "directive should parse"))
            .collect()
    }

    fn ensure_directives(
        dirs: &[Directive],
        expected: &[DirectiveExpectation],
    ) -> Result<(), TestFailure> {
        ensure_eq(&dirs.len(), &expected.len(), "parsed directive count")?;
        for (directive, expectation) in dirs.iter().zip(expected) {
            ensure(
                directive.target.as_deref() == expectation.target,
                "parsed directive target matches",
            )?;
            ensure(
                directive.level == expectation.level,
                "parsed directive level matches",
            )?;
            ensure(
                directive.in_span.as_deref() == expectation.in_span,
                "parsed directive span matches",
            )?;
        }
        Ok(())
    }

    /// Parses `input`, sorts the directives, and checks the resulting target ordering.
    fn ensure_directive_target_order(
        input: &str,
        expected: &[&str],
        message: &'static str,
    ) -> Result<(), TestFailure> {
        let mut dirs = expect_parse(input)?;
        dirs.sort_unstable();
        let sorted = dirs
            .iter()
            .map(|directive| directive.target.as_deref())
            .collect::<Vec<_>>();
        let expected_targets = expected.iter().copied().map(Some).collect::<Vec<_>>();
        ensure(sorted == expected_targets, message)
    }

    /// Parses `input` and checks it yields the six-entry crate/level directive expectation.
    fn ensure_parses_level_directives(input: &str) -> Result<(), TestFailure> {
        let dirs = parse_directives(input);
        ensure_directives(
            &dirs,
            &[
                DirectiveExpectation {
                    target: Some("crate1::mod1"),
                    level: LevelFilter::ERROR,
                    in_span: None,
                },
                DirectiveExpectation {
                    target: Some("crate1::mod2"),
                    level: LevelFilter::WARN,
                    in_span: None,
                },
                DirectiveExpectation {
                    target: Some("crate1::mod2::mod3"),
                    level: LevelFilter::INFO,
                    in_span: None,
                },
                DirectiveExpectation {
                    target: Some("crate2"),
                    level: LevelFilter::DEBUG,
                    in_span: None,
                },
                DirectiveExpectation {
                    target: Some("crate3"),
                    level: LevelFilter::TRACE,
                    in_span: None,
                },
                DirectiveExpectation {
                    target: Some("crate3::mod2::mod1"),
                    level: LevelFilter::OFF,
                    in_span: None,
                },
            ],
        )
    }

    /// Parses `input` and checks the case-insensitive ralith directive expectation.
    fn ensure_parses_ralith_case_insensitive(input: &str) -> Result<(), TestFailure> {
        let dirs = parse_directives(input);
        ensure_directives(
            &dirs,
            &[
                DirectiveExpectation {
                    target: Some("common"),
                    level: LevelFilter::INFO,
                    in_span: None,
                },
                DirectiveExpectation {
                    target: Some("server"),
                    level: LevelFilter::DEBUG,
                    in_span: None,
                },
            ],
        )
    }

    #[test]
    fn directive_ordering_by_target_len() -> Result<(), TestFailure> {
        // TODO(eliza): it would be nice to have a property-based test for this
        // instead.
        ensure_directive_target_order(
            "foo::bar=debug,foo::bar::baz=trace,foo=info,a_really_long_name_with_no_colons=warn",
            &[
                "a_really_long_name_with_no_colons",
                "foo::bar::baz",
                "foo::bar",
                "foo",
            ],
            "directives sort by descending target length",
        )
    }
    #[test]
    fn directive_ordering_by_span() -> Result<(), TestFailure> {
        // TODO(eliza): it would be nice to have a property-based test for this
        // instead.
        ensure_directive_target_order(
            "bar[span]=trace,foo=debug,baz::quux=info,a[span]=warn",
            &["baz::quux", "bar", "foo", "a"],
            "directives sort by span specificity",
        )
    }

    #[test]
    fn directive_ordering_uses_lexicographic_when_equal() -> Result<(), TestFailure> {
        // TODO(eliza): it would be nice to have a property-based test for this
        // instead.
        let mut dirs = expect_parse("span[b]=debug,b=debug,a=trace,c=info,span[a]=info")?;
        dirs.sort_unstable();

        let expected = vec![
            (Some("span"), Some("b")),
            (Some("span"), Some("a")),
            (Some("c"), None),
            (Some("b"), None),
            (Some("a"), None),
        ];
        let sorted = dirs
            .iter()
            .map(|directive| {
                (
                    directive.target.as_deref(),
                    directive.in_span.as_deref(),
                )
            })
            .collect::<Vec<_>>();

        ensure(
            sorted == expected,
            "directives sort lexicographically after specificity",
        )
    }

    #[test]
    fn directive_ordering_by_field_num() -> Result<(), TestFailure> {
        // TODO(eliza): it would be nice to have a property-based test for this
        // instead.
        ensure_directive_target_order(
            "b[{foo,bar}]=info,c[{baz,quuux,quuux}]=debug,a[{foo}]=warn,bar[{field}]=trace,foo=debug,baz::quux=info",
            &["baz::quux", "bar", "foo", "c", "b", "a"],
            "directives sort by field count",
        )
    }

    #[test]
    fn parse_directives_ralith() -> Result<(), TestFailure> {
        let dirs = parse_directives("common=trace,server=trace");
        ensure_directives(
            &dirs,
            &[
                DirectiveExpectation {
                    target: Some("common"),
                    level: LevelFilter::TRACE,
                    in_span: None,
                },
                DirectiveExpectation {
                    target: Some("server"),
                    level: LevelFilter::TRACE,
                    in_span: None,
                },
            ],
        )
    }

    #[test]
    fn parse_directives_ralith_uc() -> Result<(), TestFailure> {
        ensure_parses_ralith_case_insensitive("common=INFO,server=DEBUG")
    }

    #[test]
    fn parse_directives_ralith_mixed() -> Result<(), TestFailure> {
        ensure_parses_ralith_case_insensitive("common=iNfo,server=dEbUg")
    }

    #[test]
    fn parse_directives_valid() -> Result<(), TestFailure> {
        let dirs = parse_directives("crate1::mod1=error,crate1::mod2,crate2=debug,crate3=off");
        ensure_directives(
            &dirs,
            &[
                DirectiveExpectation {
                    target: Some("crate1::mod1"),
                    level: LevelFilter::ERROR,
                    in_span: None,
                },
                DirectiveExpectation {
                    target: Some("crate1::mod2"),
                    level: LevelFilter::TRACE,
                    in_span: None,
                },
                DirectiveExpectation {
                    target: Some("crate2"),
                    level: LevelFilter::DEBUG,
                    in_span: None,
                },
                DirectiveExpectation {
                    target: Some("crate3"),
                    level: LevelFilter::OFF,
                    in_span: None,
                },
            ],
        )
    }

    #[test]
    fn parse_level_directives() -> Result<(), TestFailure> {
        ensure_parses_level_directives(
            "crate1::mod1=error,crate1::mod2=warn,crate1::mod2::mod3=info,\
             crate2=debug,crate3=trace,crate3::mod2::mod1=off",
        )
    }

    #[test]
    fn parse_uppercase_level_directives() -> Result<(), TestFailure> {
        ensure_parses_level_directives(
            "crate1::mod1=ERROR,crate1::mod2=WARN,crate1::mod2::mod3=INFO,\
             crate2=DEBUG,crate3=TRACE,crate3::mod2::mod1=OFF",
        )
    }

    #[test]
    fn parse_numeric_level_directives() -> Result<(), TestFailure> {
        ensure_parses_level_directives(
            "crate1::mod1=1,crate1::mod2=2,crate1::mod2::mod3=3,crate2=4,\
             crate3=5,crate3::mod2::mod1=0",
        )
    }

    #[test]
    fn parse_directives_invalid_crate() -> Result<(), TestFailure> {
        // test parse_directives with multiple = in specification
        let dirs = parse_directives("crate1::mod1=warn=info,crate2=debug");
        ensure_directives(
            &dirs,
            &[DirectiveExpectation {
                target: Some("crate2"),
                level: LevelFilter::DEBUG,
                in_span: None,
            }],
        )
    }

    #[test]
    fn parse_directives_invalid_level() -> Result<(), TestFailure> {
        // test parse_directives with 'noNumber' as log level
        let dirs = parse_directives("crate1::mod1=noNumber,crate2=debug");
        ensure_directives(
            &dirs,
            &[DirectiveExpectation {
                target: Some("crate2"),
                level: LevelFilter::DEBUG,
                in_span: None,
            }],
        )
    }

    #[test]
    fn parse_directives_string_level() -> Result<(), TestFailure> {
        // test parse_directives with 'warn' as log level
        let dirs = parse_directives("crate1::mod1=wrong,crate2=warn");
        ensure_directives(
            &dirs,
            &[DirectiveExpectation {
                target: Some("crate2"),
                level: LevelFilter::WARN,
                in_span: None,
            }],
        )
    }

    #[test]
    fn parse_directives_empty_level() -> Result<(), TestFailure> {
        // test parse_directives with '' as log level
        let dirs = parse_directives("crate1::mod1=wrong,crate2=");
        ensure_directives(
            &dirs,
            &[DirectiveExpectation {
                target: Some("crate2"),
                level: LevelFilter::TRACE,
                in_span: None,
            }],
        )
    }

    #[test]
    fn parse_directives_global() -> Result<(), TestFailure> {
        // test parse_directives with no crate
        let dirs = parse_directives("warn,crate2=debug");
        ensure_directives(
            &dirs,
            &[
                DirectiveExpectation {
                    target: None,
                    level: LevelFilter::WARN,
                    in_span: None,
                },
                DirectiveExpectation {
                    target: Some("crate2"),
                    level: LevelFilter::DEBUG,
                    in_span: None,
                },
            ],
        )
    }

    // helper function for tests below
    fn test_parse_bare_level(
        directive_to_test: &str,
        level_expected: LevelFilter,
    ) -> Result<(), TestFailure> {
        let dirs = parse_directives(directive_to_test);
        ensure_directives(
            &dirs,
            &[DirectiveExpectation {
                target: None,
                level: level_expected,
                in_span: None,
            }],
        )
    }

    #[test]
    fn parse_directives_global_bare_warn_lc() -> Result<(), TestFailure> {
        // test parse_directives with no crate, in isolation, all lowercase
        test_parse_bare_level("warn", LevelFilter::WARN)
    }

    #[test]
    fn parse_directives_global_bare_warn_uc() -> Result<(), TestFailure> {
        // test parse_directives with no crate, in isolation, all uppercase
        test_parse_bare_level("WARN", LevelFilter::WARN)
    }

    #[test]
    fn parse_directives_global_bare_warn_mixed() -> Result<(), TestFailure> {
        // test parse_directives with no crate, in isolation, mixed case
        test_parse_bare_level("wArN", LevelFilter::WARN)
    }

    #[test]
    fn parse_directives_valid_with_spans() -> Result<(), TestFailure> {
        let dirs = parse_directives("crate1::mod1[foo]=error,crate1::mod2[bar],crate2[baz]=debug");
        ensure_directives(
            &dirs,
            &[
                DirectiveExpectation {
                    target: Some("crate1::mod1"),
                    level: LevelFilter::ERROR,
                    in_span: Some("foo"),
                },
                DirectiveExpectation {
                    target: Some("crate1::mod2"),
                    level: LevelFilter::TRACE,
                    in_span: Some("bar"),
                },
                DirectiveExpectation {
                    target: Some("crate2"),
                    level: LevelFilter::DEBUG,
                    in_span: Some("baz"),
                },
            ],
        )
    }

    #[test]
    fn parse_directives_with_dash_in_target_name() -> Result<(), TestFailure> {
        let dirs = parse_directives("target-name=info");
        ensure_directives(
            &dirs,
            &[DirectiveExpectation {
                target: Some("target-name"),
                level: LevelFilter::INFO,
                in_span: None,
            }],
        )
    }

    #[test]
    fn parse_directives_with_dash_in_span_name() -> Result<(), TestFailure> {
        // Reproduces https://github.com/tokio-rs/tracing/issues/1367

        let dirs = parse_directives("target[span-name]=info");
        ensure_directives(
            &dirs,
            &[DirectiveExpectation {
                target: Some("target"),
                level: LevelFilter::INFO,
                in_span: Some("span-name"),
            }],
        )
    }

    #[test]
    fn parse_directives_with_special_characters_in_span_name() -> Result<(), TestFailure> {
        let span_name = "!\"#$%&'()*+-./:;<=>?@^_`|~[}";

        let dirs = parse_directives(format!("target[{span_name}]=info"));
        ensure_directives(
            &dirs,
            &[DirectiveExpectation {
                target: Some("target"),
                level: LevelFilter::INFO,
                in_span: Some(span_name),
            }],
        )
    }

    #[test]
    fn parse_directives_with_invalid_span_chars() -> Result<(), TestFailure> {
        let invalid_span_name = "]{";

        let dirs = parse_directives(format!("target[{invalid_span_name}]=info"));
        ensure_eq(
            &dirs.len(),
            &0_usize,
            "invalid span characters reject directive",
        )
    }
}
