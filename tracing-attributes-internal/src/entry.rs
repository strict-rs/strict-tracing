//! Parse-and-expand entry pipeline for the `#[instrument]` attribute.
//!
//! [`instrument`] is the single top-level entry the `tracing-attributes` shim delegates to. It runs
//! a two-pass strategy over the annotated item: `instrument_precise` fully parses the item as a
//! [`syn::ItemFn`] (rejecting `const fn`s and detecting async-trait expansions), and on any parse
//! error falls back to `instrument_speculative`, which parses a [`MaybeItemFn`] whose body is kept
//! as a raw [`proc_macro2::TokenStream`]. Both passes funnel into [`crate::expand::gen_function`].

use proc_macro2::TokenStream;
use quote::ToTokens;
use quote::TokenStreamExt as _;
use quote::quote;
use syn::Attribute;
use syn::ItemFn;
use syn::Signature;
use syn::Visibility;
use syn::parse::Parse;
use syn::parse::ParseBuffer;
use syn::parse::ParseStream;
use syn::token::Brace;

use crate::attr::InstrumentArgs;
use crate::expand::AsyncInfo;
use crate::expand::gen_function;

/// Expand the `#[instrument]` attribute over its arguments and the annotated item tokens.
///
/// This is the implementation entry point the `tracing-attributes` `#[proc_macro_attribute]` shim
/// forwards to. It parses the attribute arguments, then runs the precise/speculative two-pass
/// strategy, returning either the instrumented item or a `compile_error!` expansion.
#[must_use]
#[allow(
  clippy::single_call_fn,
  reason = "top-level pipeline entry the tracing-attributes shim delegates to; kept as one named boundary"
)]
pub fn instrument(args: TokenStream, item_tokens: TokenStream) -> TokenStream {
  let parsed_args = match syn::parse2::<InstrumentArgs>(args) {
    Ok(parsed_args) => parsed_args,
    Err(parse_error) => return parse_error.into_compile_error(),
  };
  // Cloning a `TokenStream` is cheap since it's reference counted internally.
  instrument_precise(parsed_args.clone(), item_tokens.clone()).unwrap_or_else(|_err| instrument_speculative(parsed_args, item_tokens))
}

/// Instrument the function, without parsing the function body (instead using the raw tokens).
#[allow(
  clippy::single_call_fn,
  reason = "speculative parser path is isolated from precise parsing fallback to preserve macro control flow"
)]
fn instrument_speculative(args: InstrumentArgs, item_tokens: TokenStream) -> TokenStream {
  let parsed_input = match syn::parse2::<MaybeItemFn>(item_tokens) {
    Ok(parsed_input) => parsed_input,
    Err(parse_error) => return parse_error.into_compile_error(),
  };
  let instrumented_function_name = parsed_input.sig.ident.to_string();
  let parsed_input_ref = parsed_input.as_ref();
  gen_function(&parsed_input_ref, args, instrumented_function_name.as_str(), None)
}

/// Instrument the function, by fully parsing the function body,
/// which allows us to rewrite some statements related to async-like patterns.
#[allow(
  clippy::single_call_fn,
  reason = "precise parser path isolates async-trait detection and const-fn validation before fallback"
)]
fn instrument_precise(args: InstrumentArgs, item_tokens: TokenStream) -> Result<TokenStream, syn::Error> {
  let parsed_input = syn::parse2::<ItemFn>(item_tokens)?;
  let instrumented_function_name = parsed_input.sig.ident.to_string();

  if parsed_input.sig.constness.is_some() {
    return Ok(quote! {
        compile_error!("the `#[instrument]` attribute may not be used with `const fn`s")
    });
  }

  // check for async_trait-like patterns in the block, and instrument
  // the future instead of the wrapper
  if let Some(async_like) = AsyncInfo::from_fn(&parsed_input) {
    return Ok(async_like.gen_async(&args, instrumented_function_name.as_str()));
  }

  let maybe_input = MaybeItemFn::from(parsed_input);
  let maybe_input_ref = maybe_input.as_ref();

  Ok(gen_function(&maybe_input_ref, args, instrumented_function_name.as_str(), None))
}

/// This is a more flexible/imprecise `ItemFn` type,
/// which's block is just a `TokenStream` (it may contain invalid code).
#[derive(Debug, Clone)]
pub struct MaybeItemFn {
  /// Outer attributes attached before the function item.
  outer_attrs: Vec<Attribute>,
  /// Inner attributes parsed from the function body opening.
  inner_attrs: Vec<Attribute>,
  /// Function visibility.
  vis:         Visibility,
  /// Function signature.
  sig:         Signature,
  /// Brace token delimiting the raw function body.
  brace_token: Brace,
  /// Raw function body tokens.
  block:       TokenStream,
}

impl MaybeItemFn {
  /// Borrows this raw-body function representation.
  #[must_use]
  pub const fn as_ref(&self) -> MaybeItemFnRef<'_, TokenStream> {
    MaybeItemFnRef {
      outer_attrs: &self.outer_attrs,
      inner_attrs: &self.inner_attrs,
      vis:         &self.vis,
      sig:         &self.sig,
      brace_token: &self.brace_token,
      block:       &self.block,
    }
  }
}

/// This parses a `TokenStream` into a `MaybeItemFn`
/// (just like `ItemFn`, but skips parsing the body).
impl Parse for MaybeItemFn {
  fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
    let outer_attrs = input.call(Attribute::parse_outer)?;
    let vis: Visibility = input.parse()?;
    let sig: Signature = input.parse()?;
    let inner_attrs = input.call(Attribute::parse_inner)?;
    let body;
    let brace_token = syn::braced!(body in input);
    let block: TokenStream = body.call(ParseBuffer::parse)?;
    Ok(Self {
      outer_attrs,
      inner_attrs,
      vis,
      sig,
      brace_token,
      block,
    })
  }
}

impl From<ItemFn> for MaybeItemFn {
  fn from(
    ItemFn {
      attrs,
      vis,
      sig,
      block,
    }: ItemFn,
  ) -> Self {
    let (outer_attrs, inner_attrs) = attrs.into_iter().partition(|attr| attr.style == syn::AttrStyle::Outer);
    let mut block_tokens = TokenStream::new();
    block_tokens.append_all(block.stmts);
    Self {
      outer_attrs,
      inner_attrs,
      vis,
      sig,
      brace_token: block.brace_token,
      block: block_tokens,
    }
  }
}

/// A generic reference type for `MaybeItemFn`,
/// that takes a generic block type `B` that implements `ToTokens` (for example,
/// `TokenStream` or `Block`).
#[derive(Debug, Clone)]
pub struct MaybeItemFnRef<'a, B: ToTokens> {
  /// Borrowed outer attributes.
  pub outer_attrs: &'a Vec<Attribute>,
  /// Borrowed inner attributes.
  pub inner_attrs: &'a Vec<Attribute>,
  /// Borrowed function visibility.
  pub vis:         &'a Visibility,
  /// Borrowed function signature.
  pub sig:         &'a Signature,
  /// Borrowed brace token delimiting the function body.
  pub brace_token: &'a Brace,
  /// Borrowed function body representation.
  pub block:       &'a B,
}

#[cfg(test)]
mod tests {
  use quote::quote;
  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_eq;
  use strict_test_support::ensure_ok;
  use syn::ItemFn;

  use super::MaybeItemFn;
  use super::instrument;

  #[test]
  fn invalid_attribute_arguments_expand_to_compile_error() -> Result<(), TestFailure> {
    let tokens = instrument(quote!(level = "verbose"), quote! {
      fn demo() {}
    })
    .to_string();

    ensure(tokens.contains("compile_error !"), "invalid args emit compile_error")?;
    ensure(
      tokens.contains("unknown verbosity level"),
      "compile_error includes parser diagnostic",
    )
  }

  #[test]
  fn const_functions_are_rejected_by_the_precise_parser() -> Result<(), TestFailure> {
    let tokens = instrument(quote!(), quote! {
      const fn demo() {}
    })
    .to_string();

    ensure(tokens.contains("compile_error !"), "const fn emits compile_error")?;
    ensure(
      tokens.contains("may not be used with `const fn`s"),
      "const fn rejection names the unsupported item kind",
    )
  }

  #[test]
  fn precise_parser_expands_regular_functions_with_span_and_return_event() -> Result<(), TestFailure> {
    let tokens = instrument(
      quote!(level = "debug", skip(skipped), fields(extra = answer), ret(Display)),
      quote! {
        pub fn demo(answer: u64, skipped: u64) -> u64 {
          answer
        }
      },
    )
    .to_string();

    ensure(tokens.contains("pub fn demo"), "expanded output preserves function signature")?;
    ensure(tokens.contains(":: tracing :: span !"), "expanded output creates a tracing span")?;
    ensure(tokens.contains("extra = answer"), "expanded output records custom fields")?;
    ensure(
      tokens.contains("return = % __tracing_attr_return"),
      "ret(Display) emits display-formatted return event",
    )?;
    ensure(!tokens.contains("skipped ="), "skipped parameter is not auto-recorded")
  }

  #[test]
  fn speculative_parser_preserves_raw_body_tokens_when_precise_parsing_fails() -> Result<(), TestFailure> {
    let tokens = instrument(quote!(name = "speculative"), quote! {
      fn demo() {
        let = ;
      }
    })
    .to_string();

    ensure(tokens.contains("fn demo"), "speculative output preserves function signature")?;
    ensure(tokens.contains("let = ;"), "speculative output preserves raw invalid body")?;
    ensure(
      tokens.contains("\"speculative\""),
      "speculative output still applies parsed attributes",
    )
  }

  #[test]
  fn maybe_item_fn_parse_and_from_item_fn_preserve_function_parts() -> Result<(), TestFailure> {
    let maybe = ensure_ok(
      syn::parse2::<MaybeItemFn>(quote! {
        #[inline]
        pub(crate) fn demo<T>(value: T) -> T
        where
          T: Clone
        {
          value
        }
      }),
      "raw-body function parser accepts regular function item",
    )?;
    let maybe_ref = maybe.as_ref();
    ensure_eq(&maybe_ref.outer_attrs.len(), &1_usize, "raw parser preserves outer attributes")?;
    ensure_eq(
      &maybe_ref.sig.ident.to_string(),
      &String::from("demo"),
      "raw parser preserves function ident",
    )?;
    ensure(maybe_ref.block.to_string().contains("value"), "raw parser preserves body tokens")?;

    let item = ensure_ok(
      syn::parse2::<ItemFn>(quote! {
        #[inline]
        fn converted() {
          let value = 1;
        }
      }),
      "precise function parser accepts conversion fixture",
    )?;
    let converted = MaybeItemFn::from(item);
    let converted_ref = converted.as_ref();
    ensure_eq(&converted_ref.outer_attrs.len(), &1_usize, "conversion preserves outer attributes")?;
    ensure(
      converted_ref.block.to_string().contains("let value = 1"),
      "conversion preserves body statements",
    )
  }
}
