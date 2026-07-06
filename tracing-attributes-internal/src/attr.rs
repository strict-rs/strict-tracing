//! Parser for `#[instrument]` attribute arguments.
//!
//! [`InstrumentArgs`] drives the whole `#[instrument(...)]` argument list; the supporting types
//! parse levels, `err(...)` / `ret(...)` event arguments, `skip(...)` lists, and the
//! `fields(...)` DSL.

use core::marker::PhantomData;

use proc_macro2::TokenStream;
use quote::ToTokens;
use quote::quote;
use quote::quote_spanned;
use syn::Expr;
use syn::Ident;
use syn::LitInt;
use syn::LitStr;
use syn::Path;
use syn::Token;
use syn::ext::IdentExt as _;
use syn::parse::Parse;
use syn::parse::ParseStream;
use syn::punctuated::Punctuated;
use syn::token::Brace;
use syn::token::Paren;

/// Arguments to `#[instrument(err(...))]` and `#[instrument(ret(...))]` which describe how the
/// return value event should be emitted.
#[derive(Clone, Default, Debug)]
pub struct EventArgs {
  /// Optional event level override for `err` or `ret` events.
  pub level: Option<Level>,
  /// Formatting mode used for the emitted value or error field.
  pub mode:  FormatMode,
}

/// Parsed arguments supplied to the `#[instrument(...)]` attribute.
#[derive(Clone, Default, Debug)]
pub struct InstrumentArgs {
  /// Optional span level override.
  pub level:          Option<Level>,
  /// Optional span name override.
  pub name:           Option<LitStrOrIdent>,
  /// Optional span target override.
  pub target:         Option<LitStrOrIdent>,
  /// Optional explicit parent span expression.
  pub parent:         Option<Expr>,
  /// Optional `follows_from` causal span expression.
  pub follows_from:   Option<Expr>,
  /// Function parameters that should not be recorded as fields.
  pub skips:          Vec<Ident>,
  /// Whether all function parameters should be skipped.
  pub skip_all:       bool,
  /// Custom fields supplied through `fields(...)`.
  pub fields:         Option<Fields>,
  /// Optional configuration for an emitted error event.
  pub err_args:       Option<EventArgs>,
  /// Optional configuration for an emitted return-value event.
  pub ret_args:       Option<EventArgs>,
  /// Errors describing any unrecognized parse inputs that we skipped.
  pub parse_warnings: Vec<syn::Error>,
}

impl InstrumentArgs {
  /// Return the configured span level, defaulting to `INFO`.
  #[must_use]
  pub fn level(&self) -> Level {
    self.level.clone().unwrap_or(Level::Info)
  }

  /// Return the configured span target tokens, defaulting to `module_path!()`.
  #[must_use]
  pub fn target(&self) -> TokenStream {
    self
      .target
      .as_ref()
      .map_or_else(|| quote!(module_path!()), |target| quote!(#target))
  }

  /// Generate "deprecation" warnings for any unrecognized attribute inputs
  /// that we skipped.
  ///
  /// For backwards compatibility, we need to emit compiler warnings rather
  /// than errors for unrecognized inputs. Generating a fake deprecation is
  /// the only way to do this on stable Rust right now.
  #[must_use]
  pub fn warnings(&self) -> impl ToTokens + use<> {
    let warnings = self.parse_warnings.iter().map(|err| {
      let message_text = format!("found unrecognized input, {err}");
      let message = LitStr::new(&message_text, err.span());
      // TODO(eliza): This is a bit of a hack, but it's just about the
      // only way to emit warnings from a proc macro on stable Rust.
      // Eventually, when the `proc_macro::Diagnostic` API stabilizes, we
      // should definitely use that instead.
      quote_spanned! {err.span()=>
          {
              #[deprecated(since = "not actually deprecated", note = #message)]
              const TRACING_INSTRUMENT_WARNING: () = ();
              let _ = TRACING_INSTRUMENT_WARNING;
          }
      }
    });
    quote! {
        { #(#warnings)* }
    }
  }
}

/// Reject a duplicated `#[instrument(...)]` argument before it is parsed.
///
/// `already_set` is the caller's check of the field that owns the argument about to be parsed;
/// the returned error is spanned at the parse stream's current cursor, i.e. at the duplicated
/// argument rather than at the occurrence that was accepted first.
///
/// # Errors
///
/// Returns a parse error carrying `message` when `already_set` is `true`.
fn ensure_unset(already_set: bool, input: ParseStream<'_>, message: &str) -> syn::Result<()> {
  if already_set {
    return Err(input.error(message));
  }
  Ok(())
}

/// A parser for one recognized `#[instrument(...)]` argument form.
type InstrumentArgParser = fn(&mut InstrumentArgs, ParseStream<'_>) -> syn::Result<bool>;

/// Recognized `#[instrument(...)]` argument parsers, in source compatibility order.
const INSTRUMENT_ARG_PARSERS: &[InstrumentArgParser] = &[
  parse_name_argument, parse_bare_name_argument, parse_target_argument, parse_parent_argument, parse_follows_from_argument,
  parse_level_argument, parse_skip_argument, parse_skip_all_argument, parse_fields_argument, parse_err_argument, parse_ret_argument,
  parse_comma_argument,
];

/// Parses a `name = ...` argument.
#[allow(
  clippy::single_call_fn,
  reason = "named parser step documents the #[instrument(name = ...)] argument contract in the ordered scan"
)]
fn parse_name_argument(args: &mut InstrumentArgs, input: ParseStream<'_>) -> syn::Result<bool> {
  if !input.peek(kw::name) {
    return Ok(false);
  }
  ensure_unset(args.name.is_some(), input, "expected only a single `name` argument")?;
  let name = input.parse::<StrArg<kw::name>>()?.value;
  args.name = Some(name);
  Ok(true)
}

/// Parses the legacy bare string span-name argument.
#[allow(
  clippy::single_call_fn,
  reason = "named parser step preserves the legacy bare string span-name argument contract"
)]
fn parse_bare_name_argument(args: &mut InstrumentArgs, input: ParseStream<'_>) -> syn::Result<bool> {
  if !input.peek(LitStr) {
    return Ok(false);
  }
  // XXX: apparently we support names as either named args with an
  // sign, _or_ as unnamed string literals. That's weird, but
  // changing it is apparently breaking.
  // This also means that when using idents for name, it must be via
  // a named arg, i.e. `#[instrument(name = SOME_IDENT)]`.
  ensure_unset(args.name.is_some(), input, "expected only a single `name` argument")?;
  args.name = Some(input.parse()?);
  Ok(true)
}

/// Parses a `target = ...` argument.
#[allow(
  clippy::single_call_fn,
  reason = "named parser step documents the #[instrument(target = ...)] argument contract in the ordered scan"
)]
fn parse_target_argument(args: &mut InstrumentArgs, input: ParseStream<'_>) -> syn::Result<bool> {
  if !input.peek(kw::target) {
    return Ok(false);
  }
  ensure_unset(args.target.is_some(), input, "expected only a single `target` argument")?;
  let target = input.parse::<StrArg<kw::target>>()?.value;
  args.target = Some(target);
  Ok(true)
}

/// Parses a `parent = ...` argument.
#[allow(
  clippy::single_call_fn,
  reason = "named parser step documents the #[instrument(parent = ...)] argument contract in the ordered scan"
)]
fn parse_parent_argument(args: &mut InstrumentArgs, input: ParseStream<'_>) -> syn::Result<bool> {
  if !input.peek(kw::parent) {
    return Ok(false);
  }
  ensure_unset(args.parent.is_some(), input, "expected only a single `parent` argument")?;
  let parent = input.parse::<ExprArg<kw::parent>>()?;
  args.parent = Some(parent.value);
  Ok(true)
}

/// Parses a `follows_from = ...` argument.
#[allow(
  clippy::single_call_fn,
  reason = "named parser step documents the #[instrument(follows_from = ...)] argument contract in the ordered scan"
)]
fn parse_follows_from_argument(args: &mut InstrumentArgs, input: ParseStream<'_>) -> syn::Result<bool> {
  if !input.peek(kw::follows_from) {
    return Ok(false);
  }
  ensure_unset(args.follows_from.is_some(), input, "expected only a single `follows_from` argument")?;
  let follows_from = input.parse::<ExprArg<kw::follows_from>>()?;
  args.follows_from = Some(follows_from.value);
  Ok(true)
}

/// Parses a `level = ...` argument.
#[allow(
  clippy::single_call_fn,
  reason = "named parser step documents the #[instrument(level = ...)] argument contract in the ordered scan"
)]
fn parse_level_argument(args: &mut InstrumentArgs, input: ParseStream<'_>) -> syn::Result<bool> {
  if !input.peek(kw::level) {
    return Ok(false);
  }
  ensure_unset(args.level.is_some(), input, "expected only a single `level` argument")?;
  args.level = Some(input.parse()?);
  Ok(true)
}

/// Parses a `skip(...)` argument.
#[allow(
  clippy::single_call_fn,
  reason = "named parser step documents the #[instrument(skip(...))] argument contract and skip_all exclusivity"
)]
fn parse_skip_argument(args: &mut InstrumentArgs, input: ParseStream<'_>) -> syn::Result<bool> {
  if !input.peek(kw::skip) {
    return Ok(false);
  }
  ensure_unset(!args.skips.is_empty(), input, "expected only a single `skip` argument")?;
  ensure_unset(args.skip_all, input, "expected either `skip` or `skip_all` argument")?;
  let Skips(skips) = input.parse()?;
  args.skips = skips;
  Ok(true)
}

/// Parses a `skip_all` argument.
#[allow(
  clippy::single_call_fn,
  reason = "named parser step documents the #[instrument(skip_all)] argument contract and skip exclusivity"
)]
fn parse_skip_all_argument(args: &mut InstrumentArgs, input: ParseStream<'_>) -> syn::Result<bool> {
  if !input.peek(kw::skip_all) {
    return Ok(false);
  }
  ensure_unset(args.skip_all, input, "expected only a single `skip_all` argument")?;
  ensure_unset(!args.skips.is_empty(), input, "expected either `skip` or `skip_all` argument")?;
  let _skip_all: kw::skip_all = input.parse()?;
  args.skip_all = true;
  Ok(true)
}

/// Parses a `fields(...)` argument.
#[allow(
  clippy::single_call_fn,
  reason = "named parser step documents the #[instrument(fields(...))] argument contract in the ordered scan"
)]
fn parse_fields_argument(args: &mut InstrumentArgs, input: ParseStream<'_>) -> syn::Result<bool> {
  if !input.peek(kw::fields) {
    return Ok(false);
  }
  ensure_unset(args.fields.is_some(), input, "expected only a single `fields` argument")?;
  args.fields = Some(input.parse()?);
  Ok(true)
}

/// Parses an `err` event argument.
#[allow(
  clippy::single_call_fn,
  reason = "named parser step documents the #[instrument(err)] event argument contract in the ordered scan"
)]
fn parse_err_argument(args: &mut InstrumentArgs, input: ParseStream<'_>) -> syn::Result<bool> {
  if !input.peek(kw::err) {
    return Ok(false);
  }
  let _err: kw::err = input.parse()?;
  let err_args = EventArgs::parse(input)?;
  args.err_args = Some(err_args);
  Ok(true)
}

/// Parses a `ret` event argument.
#[allow(
  clippy::single_call_fn,
  reason = "named parser step documents the #[instrument(ret)] event argument contract in the ordered scan"
)]
fn parse_ret_argument(args: &mut InstrumentArgs, input: ParseStream<'_>) -> syn::Result<bool> {
  if !input.peek(kw::ret) {
    return Ok(false);
  }
  let _ret: kw::ret = input.parse()?;
  let ret_args = EventArgs::parse(input)?;
  args.ret_args = Some(ret_args);
  Ok(true)
}

/// Parses an argument separator.
#[allow(
  clippy::single_call_fn,
  reason = "named parser step keeps separator handling explicit in the ordered #[instrument] argument scan"
)]
fn parse_comma_argument(_args: &mut InstrumentArgs, input: ParseStream<'_>) -> syn::Result<bool> {
  if !input.peek(Token![,]) {
    return Ok(false);
  }
  let _comma: Token![,] = input.parse()?;
  Ok(true)
}

/// Attempts to parse one recognized `#[instrument(...)]` argument.
#[allow(
  clippy::single_call_fn,
  reason = "ordered parser dispatch keeps InstrumentArgs parsing flat while preserving argument precedence"
)]
fn parse_next_instrument_arg(args: &mut InstrumentArgs, input: ParseStream<'_>) -> syn::Result<bool> {
  for parser in INSTRUMENT_ARG_PARSERS {
    if parser(args, input)? {
      return Ok(true);
    }
  }
  Ok(false)
}

impl Parse for InstrumentArgs {
  fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
    let mut args = Self::default();
    while !input.is_empty() {
      if parse_next_instrument_arg(&mut args, input)? {
        continue;
      }

      // We found a token that we didn't expect!
      // We want to emit warnings for these, rather than errors, so
      // we'll add it to the list of unrecognized inputs we've seen so
      // far and keep going.
      let lookahead = input.lookahead1();
      args.parse_warnings.push(lookahead.error());
      // Parse the unrecognized token tree to advance the parse
      // stream, and throw it away so we can keep parsing.
      let _unknown: proc_macro2::TokenTree = input.parse()?;
    }
    Ok(args)
  }
}

impl EventArgs {
  /// Return the configured event level, falling back to the caller-provided default.
  #[must_use]
  pub fn level(&self, default: Level) -> Level {
    self.level.clone().unwrap_or(default)
  }
}

impl Parse for EventArgs {
  fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
    if !input.peek(Paren) {
      return Ok(Self::default());
    }
    let content;
    let _paren = syn::parenthesized!(content in input);
    let mut result = Self::default();
    let mut parse_one_arg = || {
      let lookahead = content.lookahead1();
      if lookahead.peek(kw::level) {
        ensure_unset(result.level.is_some(), &content, "expected only a single `level` argument")?;
        result.level = Some(content.parse()?);
        return Ok(());
      }

      ensure_unset(
        result.mode != FormatMode::default(),
        &content,
        "expected only a single format argument",
      )?;

      let Some(ident) = content.parse::<Option<Ident>>()? else {
        return Ok(());
      };

      match ident.to_string().as_str() {
        "Debug" => result.mode = FormatMode::Debug,
        "Display" => result.mode = FormatMode::Display,
        _ => {
          return Err(syn::Error::new(
            ident.span(),
            "unknown event formatting mode, expected either `Debug` or `Display`",
          ));
        }
      }
      Ok(())
    };
    parse_one_arg()?;
    if !content.is_empty() {
      if content.lookahead1().peek(Token![,]) {
        let _comma: Token![,] = content.parse()?;
        parse_one_arg()?;
      } else {
        return Err(content.error("expected `,` or `)`"));
      }
    }
    Ok(result)
  }
}

/// Either a string literal or an identifier used by `name = ...` and `target = ...`.
#[derive(Debug, Clone)]
pub enum LitStrOrIdent {
  /// A literal value such as `"my_span"`.
  LitStr(LitStr),
  /// An identifier whose value is resolved in generated code.
  Ident(Ident),
}

impl ToTokens for LitStrOrIdent {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    match *self {
      Self::LitStr(ref target) => target.to_tokens(tokens),
      Self::Ident(ref ident) => ident.to_tokens(tokens),
    }
  }
}

impl Parse for LitStrOrIdent {
  fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
    input
      .parse::<LitStr>()
      .map(LitStrOrIdent::LitStr)
      .or_else(|_| input.parse::<Ident>().map(LitStrOrIdent::Ident))
  }
}

/// Parser for `keyword = string_or_ident` arguments.
struct StrArg<T> {
  /// Parsed argument value.
  value: LitStrOrIdent,
  /// Marker tying this parser to the expected custom keyword.
  _p:    PhantomData<T>,
}

impl<T: Parse> Parse for StrArg<T> {
  fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
    let _keyword: T = input.parse()?;
    let _eq: Token![=] = input.parse()?;
    let parsed = input.parse()?;
    Ok(Self {
      value: parsed,
      _p:    PhantomData,
    })
  }
}

/// Parser for `keyword = expr` arguments.
struct ExprArg<T> {
  /// Parsed expression value.
  value: Expr,
  /// Marker tying this parser to the expected custom keyword.
  _p:    PhantomData<T>,
}

impl<T: Parse> Parse for ExprArg<T> {
  fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
    let _keyword: T = input.parse()?;
    let _eq: Token![=] = input.parse()?;
    let parsed = input.parse()?;
    Ok(Self {
      value: parsed,
      _p:    PhantomData,
    })
  }
}

/// Parsed identifiers from `skip(...)`.
struct Skips(Vec<Ident>);

impl Parse for Skips {
  fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
    let _skip: kw::skip = input.parse()?;
    let content;
    let _paren = syn::parenthesized!(content in input);
    let names = content.parse_terminated(Ident::parse_any, Token![,])?;
    let mut skips = Vec::new();
    for name in names {
      let span = name.span();
      if skips.iter().any(|existing| existing == &name) {
        return Err(syn::Error::new(span, "tried to skip the same field twice"));
      }
      skips.push(name);
    }
    Ok(Self(skips))
  }
}

/// Formatting mode for emitted `err` and `ret` events.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, Default)]
pub enum FormatMode {
  /// Use the macro's default formatter for the event kind.
  #[default]
  Default,
  /// Use `%`, requiring `Display`.
  Display,
  /// Use `?`, requiring `Debug`.
  Debug,
}

/// Parsed custom fields from `fields(...)`.
#[derive(Clone, Debug)]
pub struct Fields(
  /// Comma-separated custom field definitions.
  pub Punctuated<Field, Token![,]>,
);

/// One parsed field entry from `fields(...)`.
#[derive(Clone, Debug)]
pub struct Field {
  /// Field name, either dotted identifiers or `{expr}`.
  pub name:  FieldName,
  /// Optional explicit field value after `=`.
  pub value: Option<Expr>,
  /// Formatting sigil or value mode for this field.
  pub kind:  FieldKind,
}

/// Formatting mode for a custom field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FieldKind {
  /// Format the field with `?`.
  Debug,
  /// Format the field with `%`.
  Display,
  /// Record the field as a `tracing::Value`.
  Value,
}

/// Parsed custom field name.
#[derive(Clone, Debug)]
pub enum FieldName {
  /// Field name from the `{expr}` dynamic-name form, boxed because `syn::Expr` dwarfs the
  /// dotted-identifier variant.
  Expr(Box<Expr>),
  /// Field name from one or more dotted identifiers.
  Punctuated(Punctuated<Ident, Token![.]>),
}

impl ToTokens for FieldName {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    match *self {
      Self::Expr(ref expr) => {
        Brace::default().surround(tokens, |inner_tokens| expr.to_tokens(inner_tokens));
      }
      Self::Punctuated(ref punctuated) => punctuated.to_tokens(tokens),
    }
  }
}

impl Parse for Fields {
  fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
    let _fields: kw::fields = input.parse()?;
    let content;
    let _paren = syn::parenthesized!(content in input);
    let fields = content.parse_terminated(Field::parse, Token![,])?;
    Ok(Self(fields))
  }
}

impl ToTokens for Fields {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    self.0.to_tokens(tokens);
  }
}

impl Parse for Field {
  fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
    let mut kind = parse_field_kind_prefix(input)?.unwrap_or(FieldKind::Value);
    // Parse name as either an expr between braces or a dotted identifier.
    let name = if input.peek(Brace) {
      let content;
      let _brace = syn::braced!(content in input);
      let expr = content.call(Expr::parse)?;
      FieldName::Expr(Box::new(expr))
    } else {
      FieldName::Punctuated(Punctuated::parse_separated_nonempty_with(input, Ident::parse_any)?)
    };
    let parsed = if input.peek(Token![=]) {
      let _eq: Token![=] = input.parse()?;
      if let Some(value_kind) = parse_field_kind_prefix(input)? {
        kind = value_kind;
      }
      Some(input.parse()?)
    } else {
      None
    };
    Ok(Self {
      name,
      value: parsed,
      kind,
    })
  }
}

impl ToTokens for Field {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    if let Some(ref value_expr) = self.value {
      let name = &self.name;
      let kind = &self.kind;
      tokens.extend(quote! {
          #name = #kind #value_expr
      });
    } else if self.kind == FieldKind::Value {
      // XXX(eliza): I don't like that fields without values produce
      // empty fields rather than local variable shorthand...but,
      // we've released a version where field names without values in
      // `instrument` produce empty field values, so changing it now
      // is a breaking change. agh.
      let name = &self.name;
      tokens.extend(quote!(#name = ::tracing::field::Empty));
    } else {
      self.kind.to_tokens(tokens);
      self.name.to_tokens(tokens);
    }
  }
}

impl ToTokens for FieldKind {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    match *self {
      Self::Debug => tokens.extend(quote! { ? }),
      Self::Display => tokens.extend(quote! { % }),
      Self::Value => {}
    }
  }
}

/// Parse an optional `%` or `?` field formatting prefix.
fn parse_field_kind_prefix(input: ParseStream<'_>) -> syn::Result<Option<FieldKind>> {
  if input.peek(Token![%]) {
    let _percent: Token![%] = input.parse()?;
    return Ok(Some(FieldKind::Display));
  }

  if input.peek(Token![?]) {
    let _question: Token![?] = input.parse()?;
    return Ok(Some(FieldKind::Debug));
  }

  Ok(None)
}

/// Parsed tracing level for spans and events.
#[derive(Clone, Debug)]
pub enum Level {
  /// `TRACE`.
  Trace,
  /// `DEBUG`.
  Debug,
  /// `INFO`.
  Info,
  /// `WARN`.
  Warn,
  /// `ERROR`.
  Error,
  /// Path expression resolved in generated code.
  Path(Path),
}

impl Parse for Level {
  fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
    let _level_keyword: kw::level = input.parse()?;
    let _eq: Token![=] = input.parse()?;
    let lookahead = input.lookahead1();
    if lookahead.peek(LitStr) {
      let level: LitStr = input.parse()?;
      let level_name = level.value();
      if level_name.eq_ignore_ascii_case("trace") {
        Ok(Self::Trace)
      } else if level_name.eq_ignore_ascii_case("debug") {
        Ok(Self::Debug)
      } else if level_name.eq_ignore_ascii_case("info") {
        Ok(Self::Info)
      } else if level_name.eq_ignore_ascii_case("warn") {
        Ok(Self::Warn)
      } else if level_name.eq_ignore_ascii_case("error") {
        Ok(Self::Error)
      } else {
        Err(input.error("unknown verbosity level, expected one of \"trace\", \"debug\", \"info\", \"warn\", or \"error\", or a number 1-5"))
      }
    } else if lookahead.peek(LitInt) {
      fn is_level(lit: &LitInt, expected: u64) -> bool {
        lit.base10_parse::<u64>().is_ok_and(|parsed_level| parsed_level == expected)
      }
      let level: LitInt = input.parse()?;
      match &level {
        literal if is_level(literal, 1) => Ok(Self::Trace),
        literal if is_level(literal, 2) => Ok(Self::Debug),
        literal if is_level(literal, 3) => Ok(Self::Info),
        literal if is_level(literal, 4) => Ok(Self::Warn),
        literal if is_level(literal, 5) => Ok(Self::Error),
        _ => Err(
          input.error("unknown verbosity level, expected one of \"trace\", \"debug\", \"info\", \"warn\", or \"error\", or a number 1-5"),
        ),
      }
    } else if lookahead.peek(Ident) {
      Ok(Self::Path(input.parse()?))
    } else {
      Err(lookahead.error())
    }
  }
}

impl ToTokens for Level {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    match *self {
      Self::Trace => tokens.extend(quote!(::tracing::Level::TRACE)),
      Self::Debug => tokens.extend(quote!(::tracing::Level::DEBUG)),
      Self::Info => tokens.extend(quote!(::tracing::Level::INFO)),
      Self::Warn => tokens.extend(quote!(::tracing::Level::WARN)),
      Self::Error => tokens.extend(quote!(::tracing::Level::ERROR)),
      Self::Path(ref path) => tokens.extend(quote!(#path)),
    }
  }
}

/// Custom keywords used by the `#[instrument(...)]` parser.
mod kw {
  syn::custom_keyword!(fields);
  syn::custom_keyword!(skip);
  syn::custom_keyword!(skip_all);
  syn::custom_keyword!(level);
  syn::custom_keyword!(target);
  syn::custom_keyword!(parent);
  syn::custom_keyword!(follows_from);
  syn::custom_keyword!(name);
  syn::custom_keyword!(err);
  syn::custom_keyword!(ret);
}

#[cfg(test)]
mod tests {
  use proc_macro2::TokenStream;
  use quote::ToTokens as _;
  use quote::quote;
  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_eq;
  use strict_test_support::ensure_ok;
  use strict_test_support::ensure_some;

  use super::Fields;
  use super::FormatMode;
  use super::InstrumentArgs;
  use super::Level;

  /// Render a parsed level using the token representation generated for macro output.
  fn level_tokens(level: &Level) -> String {
    level.to_token_stream().to_string()
  }

  /// Parse `tokens` as `#[instrument(...)]` arguments, requiring rejection with exactly
  /// `expected_message`.
  fn ensure_rejects(tokens: TokenStream, expected_message: &'static str) -> Result<(), TestFailure> {
    let parse_error = ensure_some(
      syn::parse2::<InstrumentArgs>(tokens).err(),
      "duplicate or conflicting arguments must be rejected",
    )?;
    ensure_eq(
      &parse_error.to_string().as_str(),
      &expected_message,
      "rejection message must match the pinned text",
    )
  }

  #[test]
  fn parses_names_targets_levels_and_skip_lists() -> Result<(), TestFailure> {
    let args = ensure_ok(
      syn::parse2::<InstrumentArgs>(quote!(
        "literal_name",
        target = target_ident,
        level = 5,
        skip(self, r#type),
        fields(extra = %value)
      )),
      "literal name, ident target, numeric level, raw skips, and fields parse together",
    )?;

    ensure(args.name.is_some(), "literal name is stored")?;
    ensure_eq(
      &args.target().to_string(),
      &quote!(target_ident).to_string(),
      "identifier target is emitted as target tokens",
    )?;
    ensure_eq(
      &level_tokens(&args.level()),
      &quote!(::tracing::Level::ERROR).to_string(),
      "numeric level 5 maps to ERROR",
    )?;
    ensure_eq(&args.skips.len(), &2_usize, "two skip identifiers are recorded")?;
    ensure(!args.skip_all, "skip list does not enable skip_all")?;
    ensure(args.fields.is_some(), "custom fields are retained")
  }

  #[test]
  fn parses_every_string_level_and_custom_path_level() -> Result<(), TestFailure> {
    let cases = [
      (quote!(level = "trace"), quote!(::tracing::Level::TRACE).to_string()),
      (quote!(level = "debug"), quote!(::tracing::Level::DEBUG).to_string()),
      (quote!(level = "info"), quote!(::tracing::Level::INFO).to_string()),
      (quote!(level = "warn"), quote!(::tracing::Level::WARN).to_string()),
      (quote!(level = "error"), quote!(::tracing::Level::ERROR).to_string()),
      (quote!(level = custom::LEVEL), quote!(custom::LEVEL).to_string()),
    ];

    for (input, expected_tokens) in cases {
      let args = ensure_ok(syn::parse2::<InstrumentArgs>(input), "supported level literal or path parses")?;
      ensure_eq(&level_tokens(&args.level()), &expected_tokens, "parsed level emits expected tokens")?;
    }

    ensure_rejects(
      quote!(level = "verbose"),
      "unexpected end of input, unknown verbosity level, expected one of \"trace\", \"debug\", \"info\", \"warn\", or \"error\", or a \
       number 1-5",
    )?;
    ensure_rejects(
      quote!(level = 6),
      "unexpected end of input, unknown verbosity level, expected one of \"trace\", \"debug\", \"info\", \"warn\", or \"error\", or a \
       number 1-5",
    )
  }

  #[test]
  fn parses_event_args_modes_levels_and_rejects_duplicates() -> Result<(), TestFailure> {
    let args = ensure_ok(
      syn::parse2::<InstrumentArgs>(quote!(
        level = "debug",
        err(Display, level = "warn"),
        ret(Debug, level = custom::RET_LEVEL)
      )),
      "err and ret argument lists parse",
    )?;

    let err_args = ensure_some(args.err_args.as_ref(), "err args are recorded")?;
    ensure(err_args.mode == FormatMode::Display, "err Display mode is recorded")?;
    ensure_eq(
      &level_tokens(&err_args.level(Level::Error)),
      &quote!(::tracing::Level::WARN).to_string(),
      "err level override is used",
    )?;

    let ret_args = ensure_some(args.ret_args.as_ref(), "ret args are recorded")?;
    ensure(ret_args.mode == FormatMode::Debug, "ret Debug mode is recorded")?;
    ensure_eq(
      &level_tokens(&ret_args.level(Level::Info)),
      &quote!(custom::RET_LEVEL).to_string(),
      "ret custom level override is used",
    )?;

    let defaults = ensure_ok(
      syn::parse2::<InstrumentArgs>(quote!(err, ret)),
      "bare err and ret arguments use defaults",
    )?;
    ensure(
      ensure_some(defaults.err_args.as_ref(), "bare err args are recorded")?.mode == FormatMode::Default,
      "bare err uses default format mode",
    )?;
    ensure(
      ensure_some(defaults.ret_args.as_ref(), "bare ret args are recorded")?.mode == FormatMode::Default,
      "bare ret uses default format mode",
    )?;

    ensure_rejects(quote!(err(Display, Debug)), "expected only a single format argument")?;
    ensure_rejects(
      quote!(ret(level = "info", level = "debug")),
      "expected only a single `level` argument",
    )
  }

  #[test]
  fn parses_field_names_values_and_formatting_modes() -> Result<(), TestFailure> {
    let fields = ensure_ok(
      syn::parse2::<Fields>(quote!(
        fields(
          bare,
          ?debug_only,
          %display_only,
          dotted.name = answer,
          explicit_debug = ?answer,
          explicit_display = %answer,
          {dynamic_name()} = value
        )
      )),
      "supported custom field forms parse",
    )?;
    let rendered = fields.to_token_stream().to_string();

    ensure(
      rendered.contains("bare = :: tracing :: field :: Empty"),
      "bare field emits an explicit Empty value",
    )?;
    ensure(rendered.contains("? debug_only"), "debug field shorthand is emitted")?;
    ensure(rendered.contains("% display_only"), "display field shorthand is emitted")?;
    ensure(rendered.contains("dotted . name = answer"), "dotted field value is emitted")?;
    ensure(rendered.contains("explicit_debug = ? answer"), "explicit debug value is emitted")?;
    ensure(
      rendered.contains("explicit_display = % answer"),
      "explicit display value is emitted",
    )?;
    ensure(
      rendered.contains("{ dynamic_name () } = value"),
      "dynamic field name is emitted in braces",
    )
  }

  #[test]
  fn parse_warnings_preserve_unknown_inputs_without_dropping_valid_args() -> Result<(), TestFailure> {
    let args = ensure_ok(
      syn::parse2::<InstrumentArgs>(quote!(unknown_token, level = "info", another_unknown, skip_all)),
      "unknown inputs are retained as warnings while valid inputs parse",
    )?;

    ensure_eq(&args.parse_warnings.len(), &2_usize, "two unknown tokens become warnings")?;
    ensure_eq(
      &level_tokens(&args.level()),
      &quote!(::tracing::Level::INFO).to_string(),
      "valid level is still recorded after unknown input",
    )?;
    ensure(args.skip_all, "valid skip_all is still recorded after unknown input")?;
    let warnings = args.warnings().to_token_stream().to_string();
    ensure(
      warnings.contains("TRACING_INSTRUMENT_WARNING"),
      "warning tokens define the fake deprecation marker",
    )?;
    ensure(
      warnings.contains("found unrecognized input"),
      "warning tokens include the diagnostic text",
    )
  }

  #[test]
  fn skip_list_rejects_duplicate_raw_or_plain_identifiers() -> Result<(), TestFailure> {
    ensure_rejects(quote!(skip(first_arg, first_arg)), "tried to skip the same field twice")?;
    ensure_rejects(quote!(skip(r#type, r#type)), "tried to skip the same field twice")
  }

  #[test]
  fn accepts_target_with_parent() -> Result<(), TestFailure> {
    let args = ensure_ok(
      syn::parse2::<InstrumentArgs>(quote!(target = "custom_target", parent = source::parent_span)),
      "`target` combined with `parent` must parse",
    )?;
    ensure(args.target.is_some(), "the `target` argument must be recorded")?;
    ensure(args.parent.is_some(), "the `parent` argument must be recorded")
  }

  #[test]
  fn accepts_target_with_follows_from() -> Result<(), TestFailure> {
    let args = ensure_ok(
      syn::parse2::<InstrumentArgs>(quote!(target = "custom_target", follows_from = causes)),
      "`target` combined with `follows_from` must parse",
    )?;
    ensure(args.target.is_some(), "the `target` argument must be recorded")?;
    ensure(args.follows_from.is_some(), "the `follows_from` argument must be recorded")
  }

  #[test]
  fn rejects_duplicate_name() -> Result<(), TestFailure> {
    ensure_rejects(
      quote!(name = "first_name", name = "second_name"),
      "expected only a single `name` argument",
    )
  }

  #[test]
  fn rejects_duplicate_target() -> Result<(), TestFailure> {
    ensure_rejects(
      quote!(target = "first_target", target = "second_target"),
      "expected only a single `target` argument",
    )
  }

  #[test]
  fn rejects_duplicate_parent() -> Result<(), TestFailure> {
    ensure_rejects(
      quote!(parent = first_parent, parent = second_parent),
      "expected only a single `parent` argument",
    )
  }

  #[test]
  fn rejects_duplicate_follows_from() -> Result<(), TestFailure> {
    ensure_rejects(
      quote!(follows_from = first_causes, follows_from = second_causes),
      "expected only a single `follows_from` argument",
    )
  }

  #[test]
  fn rejects_duplicate_level() -> Result<(), TestFailure> {
    ensure_rejects(quote!(level = "info", level = "debug"), "expected only a single `level` argument")
  }

  #[test]
  fn rejects_duplicate_skip() -> Result<(), TestFailure> {
    ensure_rejects(quote!(skip(first_arg), skip(second_arg)), "expected only a single `skip` argument")
  }

  #[test]
  fn rejects_duplicate_skip_all() -> Result<(), TestFailure> {
    ensure_rejects(quote!(skip_all, skip_all), "expected only a single `skip_all` argument")
  }

  #[test]
  fn rejects_duplicate_fields() -> Result<(), TestFailure> {
    ensure_rejects(
      quote!(fields(first_field), fields(second_field)),
      "expected only a single `fields` argument",
    )
  }

  #[test]
  fn rejects_skip_then_skip_all() -> Result<(), TestFailure> {
    ensure_rejects(quote!(skip(first_arg), skip_all), "expected either `skip` or `skip_all` argument")
  }

  #[test]
  fn rejects_skip_all_then_skip() -> Result<(), TestFailure> {
    ensure_rejects(quote!(skip_all, skip(first_arg)), "expected either `skip` or `skip_all` argument")
  }
}
