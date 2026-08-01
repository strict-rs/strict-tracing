//! Code generation for instrumented functions and async-trait rewrites.
//!
//! [`gen_function`] and `gen_block` emit the span-creating wrapper for a function body;
//! [`AsyncInfo`] detects async-trait-style expansions so the inner future is instrumented
//! rather than the allocating wrapper.

use std::iter;

use proc_macro2::TokenStream;
use quote::ToTokens;
use quote::TokenStreamExt as _;
use quote::quote;
use quote::quote_spanned;
use syn::Expr;
use syn::ExprAsync;
use syn::ExprCall;
use syn::FieldPat;
use syn::FnArg;
use syn::Ident;
use syn::Item;
use syn::ItemFn;
use syn::Pat;
use syn::PatIdent;
use syn::PatReference;
use syn::PatStruct;
use syn::PatTuple;
use syn::PatTupleStruct;
use syn::PatType;
use syn::Path;
use syn::ReturnType;
use syn::Stmt;
use syn::Token;
use syn::Type;
use syn::TypeInfer;
use syn::TypePath;
use syn::TypeReference;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned as _;
use syn::visit_mut::VisitMut;
use syn::visit_mut::visit_expr_mut;
use syn::visit_mut::visit_type_mut;

use crate::attr::FieldName;
use crate::attr::FormatMode;
use crate::attr::InstrumentArgs;
use crate::attr::Level;
use crate::entry::MaybeItemFn;
use crate::entry::MaybeItemFnRef;

/// Given an existing function, generate an instrumented version of that function
#[must_use]
pub fn gen_function<B: ToTokens>(
  input: &MaybeItemFnRef<'_, B>,
  args: InstrumentArgs,
  instrumented_function_name: &str,
  self_type: Option<&TypePath>,
) -> TokenStream {
  // these are needed ahead of time, as ItemFn contains the function body _and_
  // isn't representable inside a quote!/quote_spanned! macro
  // (Syn's ToTokens isn't implemented for ItemFn)
  let &MaybeItemFnRef {
    outer_attrs,
    inner_attrs,
    vis,
    modifiers,
    sig,
    brace_token,
    block,
  } = input;

  let output = &sig.output;
  let params = &sig.inputs;
  let safety = &sig.safety;
  let asyncness = &sig.asyncness;
  let constness = &sig.constness;
  let abi = &sig.abi;
  let ident = &sig.ident;
  let gen_params = &sig.generics.params;
  let where_clause = &sig.generics.where_clause;
  let lt_token = &sig.generics.lt_token;
  let gt_token = &sig.generics.gt_token;
  let fn_token = &sig.fn_token;
  let defaultness = &modifiers.defaultness;
  let paren_token = &sig.paren_token;
  let variadic = &sig.variadic;

  let warnings = args.warnings();

  let (return_type, return_span) = match *output {
    ReturnType::Type(_, ref explicit_return_type) => {
      let mut return_type = explicit_return_type.as_ref().clone();
      ImplTraitEraser.visit_type_mut(&mut return_type);
      (return_type, explicit_return_type.span())
    }
    ReturnType::Default => {
      // Point at function name if we don't have an explicit return type
      (syn::parse_quote! { () }, ident.span())
    }
  };
  // Install a fake return edge as the first thing in the function body, so
  // that we eagerly infer that the return type is what we declared in the
  // async fn signature.
  //
  // The return edge is never taken, but it affects inference, so it needs to
  // feed a value with the declared return type into a `return` expression.
  // Match on `None` rather than constructing the value with a divergent
  // expression so downstream crates can deny `unreachable_code`.
  let fake_return_edge = quote_spanned! {return_span=>
      match ::core::option::Option::<#return_type>::None {
          ::core::option::Option::Some(__tracing_attr_fake_return) => {
              return __tracing_attr_fake_return;
          }
          ::core::option::Option::None => {}
      }
  };
  let wrapped_block = quote! {
      {
          #fake_return_edge
          { #block }
      }
  };

  let body = gen_block(
    &wrapped_block,
    params,
    asyncness.is_some(),
    args,
    instrumented_function_name,
    self_type,
  );

  let mut result = quote!(
      #(#outer_attrs) *
      #vis #defaultness #constness #asyncness #safety #abi #fn_token #ident
      #lt_token #gen_params #gt_token
  );

  paren_token.surround(&mut result, |tokens| {
    params.to_tokens(tokens);
    variadic.to_tokens(tokens);
  });

  output.to_tokens(&mut result);
  where_clause.to_tokens(&mut result);

  brace_token.surround(&mut result, |tokens| {
    tokens.append_all(inner_attrs);
    warnings.to_tokens(tokens);
    body.to_tokens(tokens);
  });

  result
}

/// Instrument a block
fn gen_block<B: ToTokens>(
  block: &B,
  params: &Punctuated<FnArg, Token![,]>,
  async_context: bool,
  mut args: InstrumentArgs,
  instrumented_function_name: &str,
  self_type: Option<&TypePath>,
) -> TokenStream {
  let span_name = args
    .name
    .as_ref()
    .map_or_else(|| quote!(#instrumented_function_name), |name| quote!(#name));

  let args_level = args.level();
  let level = args_level.clone();

  let follows_from_sources = args.follows_from.iter();
  let follows_from_tokens = quote! {
      #(for cause in #follows_from_sources {
          __tracing_attr_span.follows_from(cause);
      })*
  };

  let span_tokens = gen_span(params, &mut args, &level, &span_name, self_type);
  let target = args.target();
  let (err_event, ret_event) = gen_events(&args, &target, &args_level);

  if async_context {
    return gen_async_block(block, &span_tokens, &follows_from_tokens, err_event, ret_event);
  }

  gen_sync_block(block, &span_tokens, &follows_from_tokens, &level, err_event, ret_event)
}

/// Generate the `tracing::span!` expression for an instrumented block.
#[allow(
  clippy::single_call_fn,
  reason = "span token generation is a named macro pipeline stage that owns parameter-field validation"
)]
fn gen_span(
  params: &Punctuated<FnArg, Token![,]>,
  args: &mut InstrumentArgs,
  level: &Level,
  span_name: &TokenStream,
  self_type: Option<&TypePath>,
) -> TokenStream {
  let param_names = collect_param_names(params, self_type);

  for skip in &args.skips {
    if !param_names.iter().any(|param| &param.user_ident == skip) {
      return quote_spanned! {skip.span()=>
          compile_error!("attempting to skip non-existent parameter")
      };
    }
  }

  let target = args.target();
  let quoted_fields: Vec<_> = param_names
    .iter()
    .filter_map(|param| {
      let user_name = &param.user_ident;
      if !should_record_param(user_name, args) {
        return None;
      }

      let real_name = &param.real_ident;
      let record_type = param.record_type;
      Some(match record_type {
        RecordType::Value => quote!(#user_name = #real_name),
        RecordType::Debug => quote!(#user_name = ::tracing::field::debug(&#real_name)),
      })
    })
    .collect();

  rename_custom_field_inputs(args, param_names, self_type);

  let parent = args.parent.iter();
  let custom_fields = &args.fields;

  quote!(::tracing::span!(
      target: #target,
      #(parent: #parent,)*
      #level,
      #span_name,
      #(#quoted_fields,)*
      #custom_fields

  ))
}

/// One function parameter collected for span-field generation, pairing the user-visible field
/// name with the generated binding the emitted code must actually read.
struct RenamedParam {
  /// Parameter name as the user wrote it; used as the generated span field name.
  user_ident:  Ident,
  /// Binding the generated code reads the value from (old async-trait rewrites `self` to `_self`).
  real_ident:  Ident,
  /// How the parameter's value is recorded on the generated span.
  record_type: RecordType,
}

/// Collect parameter names and their inferred recording mode.
#[allow(
  clippy::single_call_fn,
  reason = "parameter collection isolates destructuring and receiver normalization for span field generation"
)]
fn collect_param_names(params: &Punctuated<FnArg, Token![,]>, self_type: Option<&TypePath>) -> Vec<RenamedParam> {
  params
    .iter()
    .cloned()
    .flat_map(|param| match param {
      FnArg::Typed(PatType {
        pat,
        ty,
        ..
      }) => param_names(*pat, RecordType::parse_from_ty(&ty)),
      FnArg::Receiver(_) => Box::new(iter::once((Ident::new("self", param.span()), RecordType::Debug))),
    })
    .map(|(source_ident, record_type)| {
      if self_type.is_some() && source_ident == "_self" {
        RenamedParam {
          user_ident: Ident::new("self", source_ident.span()),
          real_ident: source_ident,
          record_type,
        }
      } else {
        RenamedParam {
          user_ident: source_ident.clone(),
          real_ident: source_ident,
          record_type,
        }
      }
    })
    .collect()
}

/// Return whether a parameter should be recorded as a generated span field.
#[allow(
  clippy::single_call_fn,
  reason = "recording predicate keeps skip and custom-field shadowing rules explicit"
)]
fn should_record_param(param: &Ident, args: &InstrumentArgs) -> bool {
  if args.skip_all || args.skips.contains(param) {
    return false;
  }

  args
    .fields
    .as_ref()
    .is_none_or(|fields| fields.0.iter().all(|field| field_name_allows_param(&field.name, param)))
}

/// Return whether a custom field name should leave a parameter auto-recorded.
#[allow(
  clippy::single_call_fn,
  reason = "custom-field shadowing rule is kept named to document parameter auto-recording"
)]
fn field_name_allows_param(field_name: &FieldName, param: &Ident) -> bool {
  match *field_name {
    FieldName::Expr(_) => true,
    FieldName::Punctuated(ref punctuated) => {
      let first_field = punctuated.first();
      first_field != punctuated.last() || first_field.is_none_or(|field_ident| field_ident != param)
    }
  }
}

/// Rewrite custom field expressions to refer to async-trait generated bindings.
#[allow(
  clippy::single_call_fn,
  reason = "async-trait field-expression rewrite requires a separate mutable pass after field collection"
)]
fn rename_custom_field_inputs(args: &mut InstrumentArgs, param_names: Vec<RenamedParam>, self_type: Option<&TypePath>) {
  let Some(custom_fields) = args.fields.as_mut() else {
    return;
  };

  let mut replacer = IdentAndTypesRenamer {
    idents: param_names
      .into_iter()
      .map(|param| (param.user_ident, param.real_ident))
      .collect(),
    types:  Vec::new(),
  };

  if let Some(receiver_self_type) = self_type {
    replacer.types.push(("Self", receiver_self_type.clone()));
  }

  for field_expr in custom_fields.0.iter_mut().filter_map(|field| field.value.as_mut()) {
    visit_expr_mut(&mut replacer, field_expr);
  }
}

/// Generate optional `err` and `ret` event expressions.
#[allow(
  clippy::single_call_fn,
  reason = "err and ret event token generation is a named macro pipeline stage"
)]
fn gen_events(args: &InstrumentArgs, target: &TokenStream, args_level: &Level) -> (Option<TokenStream>, Option<TokenStream>) {
  let err_event = args.err_args.as_ref().map(|event_args| {
    let level_tokens = event_args.level(Level::Error);
    match event_args.mode {
      FormatMode::Default | FormatMode::Display => quote!(::tracing::event!(target: #target, #level_tokens, error = %__tracing_attr_error)),
      FormatMode::Debug => quote!(::tracing::event!(target: #target, #level_tokens, error = ?__tracing_attr_error)),
    }
  });

  let ret_event = args.ret_args.as_ref().map(|event_args| {
    let level_tokens = event_args.level(args_level.clone());
    match event_args.mode {
      FormatMode::Display => quote!(::tracing::event!(target: #target, #level_tokens, return = %__tracing_attr_return)),
      FormatMode::Default | FormatMode::Debug => quote!(::tracing::event!(target: #target, #level_tokens, return = ?__tracing_attr_return)),
    }
  });

  (err_event, ret_event)
}

/// Generate the instrumented body for an async function or async block.
#[allow(
  clippy::single_call_fn,
  reason = "async body expansion is a separate macro pipeline stage from synchronous body expansion"
)]
fn gen_async_block<B: ToTokens>(
  block: &B,
  span_tokens: &TokenStream,
  follows_from_tokens: &TokenStream,
  err_event: Option<TokenStream>,
  ret_event: Option<TokenStream>,
) -> TokenStream {
  let instrumented_future = match (err_event, ret_event) {
    (Some(err_tokens), Some(ret_tokens)) => quote_spanned!(block.span()=>
        async move {
            let __match_scrutinee = async move #block.await;
            match  __match_scrutinee {
                Ok(__tracing_attr_return) => {
                    #ret_tokens;
                    Ok(__tracing_attr_return)
                },
                Err(__tracing_attr_error) => {
                    #err_tokens;
                    Err(__tracing_attr_error)
                }
            }
        }
    ),
    (Some(err_tokens), None) => quote_spanned!(block.span()=>
        async move {
            match async move #block.await {
                Ok(__tracing_attr_return) => Ok(__tracing_attr_return),
                Err(__tracing_attr_error) => {
                    #err_tokens;
                    Err(__tracing_attr_error)
                }
            }
        }
    ),
    (None, Some(ret_tokens)) => quote_spanned!(block.span()=>
        async move {
            let __tracing_attr_return = async move #block.await;
            #ret_tokens;
            __tracing_attr_return
        }
    ),
    (None, None) => quote_spanned!(block.span()=>
        async move #block
    ),
  };

  quote!(
      let __tracing_attr_span = #span_tokens;
      let __tracing_instrument_future = #instrumented_future;
      if !__tracing_attr_span.is_disabled() {
          #follows_from_tokens
          ::tracing::Instrument::instrument(
              __tracing_instrument_future,
              __tracing_attr_span
          )
          .await
      } else {
          __tracing_instrument_future.await
      }
  )
}

/// Generate the instrumented body for a synchronous function.
#[allow(
  clippy::single_call_fn,
  reason = "synchronous body expansion is a separate macro pipeline stage from async body expansion"
)]
fn gen_sync_block<B: ToTokens>(
  block: &B,
  span_tokens: &TokenStream,
  follows_from_tokens: &TokenStream,
  level: &Level,
  err_event: Option<TokenStream>,
  ret_event: Option<TokenStream>,
) -> TokenStream {
  let enter_span_tokens = quote!(
      // These variables are left uninitialized and initialized only
      // if the tracing level is statically enabled at this point.
      // While the tracing level is also checked at span creation
      // time, that will still create a dummy span, and a dummy guard
      // and drop the dummy guard later. By lazily initializing these
      // variables, Rust will generate a drop flag for them and thus
      // only drop the guard if it was created. This creates code that
      // is very straightforward for LLVM to optimize out if the tracing
      // level is statically disabled, while not causing any performance
      // regression in case the level is enabled.
      let __tracing_attr_span;
      let __tracing_attr_guard;
      if ::tracing::level_enabled!(#level) || ::tracing::if_log_enabled!(#level, {true} else {false}) {
          __tracing_attr_span = #span_tokens;
          #follows_from_tokens
          __tracing_attr_guard = __tracing_attr_span.enter();
      }
  );

  match (err_event, ret_event) {
    (Some(err_tokens), Some(ret_tokens)) => quote_spanned! {block.span()=>
        #enter_span_tokens
        match (move || #block)() {
            Ok(__tracing_attr_return) => {
                #ret_tokens;
                Ok(__tracing_attr_return)
            },
            Err(__tracing_attr_error) => {
                #err_tokens;
                Err(__tracing_attr_error)
            }
        }
    },
    (Some(err_tokens), None) => quote_spanned!(block.span()=>
        #enter_span_tokens
        match (move || #block)() {
            Ok(__tracing_attr_return) => Ok(__tracing_attr_return),
            Err(__tracing_attr_error) => {
                #err_tokens;
                Err(__tracing_attr_error)
            }
        }
    ),
    (None, Some(ret_tokens)) => quote_spanned!(block.span()=>
        #enter_span_tokens
        let __tracing_attr_return = (move || #block)();
        #ret_tokens;
        __tracing_attr_return
    ),
    (None, None) => quote_spanned!(block.span() =>
        // Because `quote` produces a stream of tokens _without_ whitespace, the
        // `if` and the block will appear directly next to each other. This
        // generates a clippy lint about suspicious `if/else` formatting.
        // Therefore, suppress the lint inside the generated code...
        {
            #enter_span_tokens
            #block
        }
    ),
  }
}

/// Indicates whether a field should be recorded as `Value` or `Debug`.
#[derive(Clone, Copy)]
enum RecordType {
  /// The field should be recorded using its `Value` implementation.
  Value,
  /// The field should be recorded using `tracing::field::debug()`.
  Debug,
}

impl RecordType {
  /// Array of primitive types which should be recorded as [`RecordType::Value`].
  const TYPES_FOR_VALUE: &'static [&'static str] = &[
    "bool", "str", "u8", "i8", "u16", "i16", "u32", "i32", "u64", "i64", "u128", "i128", "f32", "f64", "usize", "isize", "String",
    "NonZeroU8", "NonZeroI8", "NonZeroU16", "NonZeroI16", "NonZeroU32", "NonZeroI32", "NonZeroU64", "NonZeroI64", "NonZeroU128",
    "NonZeroI128", "NonZeroUsize", "NonZeroIsize", "Wrapping",
  ];

  /// Parse `RecordType` from [Type] by looking up
  /// the [`RecordType::TYPES_FOR_VALUE`] array.
  fn parse_from_ty(ty: &Type) -> Self {
    if let &Type::Path(TypePath {
      ref path, ..
    }) = ty
    {
      if path.segments.iter().next_back().is_some_and(|path_segment| {
        let ident = path_segment.ident.to_string();
        Self::TYPES_FOR_VALUE.contains(&ident.as_str())
      }) {
        Self::Value
      } else {
        Self::Debug
      }
    } else if let &Type::Reference(TypeReference {
      ref elem, ..
    }) = ty
    {
      Self::parse_from_ty(elem)
    } else {
      Self::Debug
    }
  }
}

/// Return the identifiers bound by an irrefutable function-argument pattern.
fn param_names(pattern: Pat, record_type: RecordType) -> Box<dyn Iterator<Item = (Ident, RecordType)>> {
  match pattern {
    Pat::Ident(PatIdent {
      ident, ..
    }) => Box::new(iter::once((ident, record_type))),
    Pat::Reference(PatReference {
      pat: referenced_pattern, ..
    }) => param_names(*referenced_pattern, record_type),
    // We can't get the concrete type of fields in the struct/tuple
    // patterns by using `syn`. e.g. `fn foo(Foo { x, y }: Foo) {}`.
    // Therefore, the struct/tuple patterns in the arguments will just
    // always be recorded as `RecordType::Debug`.
    Pat::Struct(PatStruct {
      fields, ..
    }) => Box::new(fields.into_iter().flat_map(
      |FieldPat {
         pat: field_pattern, ..
       }| param_names(*field_pattern, RecordType::Debug),
    )),
    Pat::Tuple(PatTuple {
      elems, ..
    }) => Box::new(
      elems
        .into_iter()
        .flat_map(|tuple_pattern| param_names(tuple_pattern, RecordType::Debug)),
    ),
    Pat::TupleStruct(PatTupleStruct {
      elems, ..
    }) => Box::new(
      elems
        .into_iter()
        .flat_map(|tuple_pattern| param_names(tuple_pattern, RecordType::Debug)),
    ),

    // The above *should* cover all cases of irrefutable patterns,
    // but we purposefully don't do any funny business here
    // (such as panicking) because that would obscure rustc's
    // much more informative error message.
    Pat::Const(_)
    | Pat::Lit(_)
    | Pat::Macro(_)
    | Pat::Or(_)
    | Pat::Paren(_)
    | Pat::Path(_)
    | Pat::Range(_)
    | Pat::Rest(_)
    | Pat::Slice(_)
    | Pat::Type(_)
    | Pat::Verbatim(_)
    | Pat::Wild(_)
    | _ => Box::new(iter::empty()),
  }
}

/// The specific async code pattern that was detected
#[derive(Debug)]
enum AsyncKind<'a> {
  /// Immediately-invoked async fn, as generated by `async-trait <= 0.1.43`:
  /// `async fn foo<...>(...) {...}; Box::pin(foo<...>(...))`
  Function(&'a ItemFn),
  /// A function returning an async (move) block, optionally `Box::pin`-ed,
  /// as generated by `async-trait >= 0.1.44`:
  /// `Box::pin(async move { ... })`
  Async {
    /// The detected async block containing the user body.
    async_expr: &'a ExprAsync,
    /// Whether the async block was wrapped in `Box::pin(...)`.
    pinned_box: bool,
  },
}

/// Information needed to instrument an async-trait-generated inner future.
#[derive(Debug)]
pub struct AsyncInfo<'block> {
  /// Statement in the outer function body that must be replaced.
  source_stmt: &'block Stmt,
  /// Detected async-trait expansion shape.
  kind:        AsyncKind<'block>,
  /// Concrete receiver type used to rewrite `Self` for old async-trait output.
  self_type:   Option<TypePath>,
  /// Original function whose body contains the async-trait expansion.
  input:       &'block ItemFn,
}

impl<'block> AsyncInfo<'block> {
  /// Get the AST of the inner function we need to hook, if it looks like a
  /// manual future implementation.
  ///
  /// When we are given a function that returns a (pinned) future containing the
  /// user logic, it is that (pinned) future that needs to be instrumented.
  /// Were we to instrument its parent, we would only collect information
  /// regarding the allocation of that future, and not its own span of execution.
  ///
  /// We inspect the block of the function to find if it matches any of the
  /// following patterns:
  ///
  /// - Immediately-invoked async fn, as generated by `async-trait <= 0.1.43`: `async fn
  ///   foo<...>(...) {...}; Box::pin(foo<...>(...))`
  ///
  /// - A function returning an async (move) block, optionally `Box::pin`-ed, as generated by
  ///   `async-trait >= 0.1.44`: `Box::pin(async move { ... })`
  ///
  /// We the return the statement that must be instrumented, along with some
  /// other information.
  /// '`gen_body`' will then be able to use that information to instrument the
  /// proper function/future.
  ///
  /// (this follows the approach suggested in
  /// <https://github.com/dtolnay/async-trait/issues/45#issuecomment-571245673>)
  #[must_use]
  #[allow(
    clippy::single_call_fn,
    reason = "async-trait shape detection is isolated from code generation and parser fallback"
  )]
  pub fn from_fn(input: &'block ItemFn) -> Option<Self> {
    // are we in an async context? If yes, this isn't a manual async-like pattern
    if input.sig.asyncness.is_some() {
      return None;
    }

    let block = &input.block;

    // list of async functions declared inside the block
    let inside_funs = block.stmts.iter().filter_map(|stmt| {
      // If a function declared inside the block is async, it is a candidate
      if let &Stmt::Item(Item::Fn(ref fun)) = stmt
        && fun.sig.asyncness.is_some()
      {
        Some((stmt, fun))
      } else {
        None
      }
    });

    // last expression of the block: it determines the return value of the
    // block, this is quite likely a `Box::pin` statement or an async block
    let (last_expr_stmt, last_expr) = block.stmts.iter().rev().find_map(|stmt| {
      if let Stmt::Expr(ref expr, _) = *stmt {
        Some((stmt, expr))
      } else {
        None
      }
    })?;

    // is the last expression an async block?
    if let Expr::Async(ref async_expr) = *last_expr {
      return Some(AsyncInfo {
        source_stmt: last_expr_stmt,
        kind: AsyncKind::Async {
          async_expr,
          pinned_box: false,
        },
        self_type: None,
        input,
      });
    }

    // is the last expression a function call?
    let Expr::Call(ExprCall {
      func: ref outside_func,
      args: ref outside_args,
      ..
    }) = *last_expr
    else {
      return None;
    };

    // is it a call to `Box::pin()`?
    let Expr::Path(ref outside_path) = *outside_func.as_ref() else {
      return None;
    };
    if !path_to_string(&outside_path.path).ends_with("Box::pin") {
      return None;
    }

    // Does the call take an argument? If it doesn't,
    // it's not gonna compile anyway, but that's no reason
    // to (try to) perform an out of bounds access
    if outside_args.is_empty() {
      return None;
    }

    // Is the argument to Box::pin an async block that
    // captures its arguments?
    if let Expr::Async(ref async_expr) = outside_args[0] {
      return Some(AsyncInfo {
        source_stmt: last_expr_stmt,
        kind: AsyncKind::Async {
          async_expr,
          pinned_box: true,
        },
        self_type: None,
        input,
      });
    }

    // Is the argument to Box::pin a function call itself?
    let Expr::Call(ExprCall {
      func: ref inner_call_func,
      ..
    }) = outside_args[0]
    else {
      return None;
    };

    // "stringify" the path of the function called
    let Expr::Path(ref func_path) = *inner_call_func.as_ref() else {
      return None;
    };
    let func_name = path_to_string(&func_path.path);

    // Was that function defined inside of the current block?
    // If so, retrieve the statement where it was declared and the function itself
    let (stmt_func_declaration, inner_func) = inside_funs.into_iter().find(|candidate| candidate.1.sig.ident == func_name)?;

    // If "_self" is present as an argument, we store its type to be able to rewrite "Self" (the
    // parameter type) with the type of "_self"
    let self_type = extract_self_type(inner_func);

    Some(AsyncInfo {
      source_stmt: stmt_func_declaration,
      kind: AsyncKind::Function(inner_func),
      self_type,
      input,
    })
  }

  /// Generate the outer function with its detected inner future instrumented.
  #[must_use]
  pub fn gen_async(self, args: &InstrumentArgs, instrumented_function_name: &str) -> TokenStream {
    let Self {
      source_stmt,
      kind,
      self_type,
      input,
    } = self;

    let replacement_index = input.block.stmts.iter().position(|stmt| stmt == source_stmt);

    let replacement = match kind {
      // `Box::pin(immediately_invoked_async_fn())`
      AsyncKind::Function(fun) => {
        let maybe_fun = MaybeItemFn::from((*fun).clone());
        let maybe_fun_ref = maybe_fun.as_ref();
        gen_function(&maybe_fun_ref, args.clone(), instrumented_function_name, self_type.as_ref())
      }
      // `async move { ... }`, optionally pinned
      AsyncKind::Async {
        async_expr,
        pinned_box,
      } => {
        let instrumented_block = gen_block(
          &async_expr.block,
          &input.sig.inputs,
          true,
          args.clone(),
          instrumented_function_name,
          None,
        );
        let async_attrs = &async_expr.attrs;
        if pinned_box {
          quote! {
              ::std::boxed::Box::pin(#(#async_attrs) * async move { #instrumented_block })
          }
        } else {
          quote! {
              #(#async_attrs) * async move { #instrumented_block }
          }
        }
      }
    };

    let out_stmts: Vec<TokenStream> = input
      .block
      .stmts
      .iter()
      .enumerate()
      .map(|(stmt_index, stmt)| {
        if Some(stmt_index) != replacement_index {
          return stmt.to_token_stream();
        }

        replacement.clone()
      })
      .collect();

    let vis = &input.vis;
    let sig = &input.sig;
    let attrs = &input.attrs;
    quote!(
        #(#attrs) *
        #vis #sig {
            #(#out_stmts) *
        }
    )
  }
}

/// Recover the concrete receiver type from an async-trait inner function's `_self` argument.
///
/// Old async-trait expansions rewrite the method receiver into a `_self` parameter, so that
/// parameter's declared type — behind any leading reference — is the concrete type that must
/// replace `Self` in user-supplied field expressions.
#[allow(
  clippy::single_call_fn,
  reason = "isolates the _self type-recovery walk so from_fn stays within the line budget"
)]
fn extract_self_type(inner_func: &ItemFn) -> Option<TypePath> {
  for arg in &inner_func.sig.inputs {
    if let FnArg::Typed(ref typed_arg) = *arg
      && let Pat::Ident(PatIdent {
        ref ident, ..
      }) = *typed_arg.pat
      && ident == "_self"
    {
      let mut arg_type = *typed_arg.ty.clone();
      // extract the inner type if the argument is "&self" or "&mut self"
      if let Type::Reference(TypeReference {
        elem, ..
      }) = arg_type
      {
        arg_type = *elem;
      }

      if let Type::Path(type_path) = arg_type {
        return Some(type_path);
      }
    }
  }
  None
}

/// Return a path's segments joined by `::`, ignoring path arguments.
fn path_to_string(path: &Path) -> String {
  let mut segments = path.segments.iter();
  let Some(first_segment) = segments.next() else {
    return String::new();
  };

  let mut rendered = first_segment.ident.to_string();
  for segment in segments {
    rendered.push_str("::");
    rendered.push_str(&segment.ident.to_string());
  }
  rendered
}

/// A visitor struct to replace idents and types in some piece
/// of code (e.g. the "self" and "Self" tokens in user-supplied
/// fields expressions when the function is generated by an old
/// version of async-trait).
struct IdentAndTypesRenamer<'a> {
  /// Type names that should be replaced with concrete type paths.
  types:  Vec<(&'a str, TypePath)>,
  /// Identifier names that should be rewritten to generated binding names.
  idents: Vec<(Ident, Ident)>,
}

impl VisitMut for IdentAndTypesRenamer<'_> {
  // `Ident` equality compares the symbol text, which is what we need when
  // replacing user-visible names with async-trait generated bindings.
  fn visit_ident_mut(&mut self, id: &mut Ident) {
    for replacement in &self.idents {
      let old_ident = &replacement.0;
      let new_ident = &replacement.1;
      if old_ident == id {
        *id = new_ident.clone();
      }
    }
  }

  fn visit_type_mut(&mut self, field_type: &mut Type) {
    for replacement in &self.types {
      let type_name = replacement.0;
      let new_type = &replacement.1;
      let replace_type = if let Type::Path(TypePath {
        ref path, ..
      }) = *field_type
      {
        path_to_string(path) == type_name
      } else {
        false
      };
      if replace_type {
        *field_type = Type::Path(new_type.clone());
      }
    }
  }
}

/// Replaces any `impl Trait` with `_` so it can be used as the type in
/// a `let` statement's LHS.
struct ImplTraitEraser;

impl VisitMut for ImplTraitEraser {
  fn visit_type_mut(&mut self, field_type: &mut Type) {
    if let Type::ImplTrait(..) = *field_type {
      *field_type = Type::Infer(TypeInfer {
        attrs:            Vec::new(),
        underscore_token: Token![_](field_type.span()),
      });
    } else {
      visit_type_mut(self, field_type);
    }
  }
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
  use syn::FnArg;
  use syn::ItemFn;

  use super::AsyncInfo;
  use super::MaybeItemFn;
  use super::gen_function;
  use super::param_names;
  use crate::attr::InstrumentArgs;
  use crate::attr::Level;

  /// Parse args and a function item, returning the generated function tokens.
  fn generated_function(arg_tokens: TokenStream, item_tokens: TokenStream) -> Result<String, TestFailure> {
    let parsed_args = ensure_ok(
      syn::parse2::<InstrumentArgs>(arg_tokens),
      "instrument arguments parse for generated function test",
    )?;
    let parsed_item = ensure_ok(
      syn::parse2::<ItemFn>(item_tokens),
      "function item parses for generated function test",
    )?;
    let maybe = MaybeItemFn::from(parsed_item);
    Ok(gen_function(&maybe.as_ref(), parsed_args, "demo", None).to_string())
  }

  /// Parse a function and return it as a `syn::ItemFn`.
  fn parsed_fn(item: TokenStream) -> Result<ItemFn, TestFailure> {
    ensure_ok(syn::parse2::<ItemFn>(item), "function fixture parses")
  }

  #[test]
  fn generated_span_records_value_debug_destructured_and_receiver_fields() -> Result<(), TestFailure> {
    let output = generated_function(quote!(fields(custom = answer)), quote! {
      fn demo(&self, answer: u64, custom: Custom, (left, right): (u8, u8)) {
        let _ = (&self, answer, custom, left, right);
      }
    })?;

    ensure(
      output.contains("self = :: tracing :: field :: debug (& self)"),
      "receiver is recorded with debug",
    )?;
    ensure(output.contains("answer = answer"), "primitive parameters are recorded as values")?;
    ensure(output.contains("custom = answer"), "custom field expression is emitted")?;
    ensure(
      !output.contains("custom = :: tracing :: field :: debug (& custom)"),
      "custom field shadows automatic parameter recording",
    )?;
    ensure(
      output.contains("left = :: tracing :: field :: debug (& left)"),
      "destructured tuple elements are recorded with debug",
    )?;
    ensure(
      output.contains("right = :: tracing :: field :: debug (& right)"),
      "all destructured tuple elements are recorded",
    )
  }

  #[test]
  fn generated_span_rejects_missing_skips_and_honors_skip_all() -> Result<(), TestFailure> {
    let missing_skip = generated_function(quote!(skip(missing)), quote! {
      fn demo(answer: u64) {
        let _ = answer;
      }
    })?;
    ensure(missing_skip.contains("compile_error !"), "missing skip emits compile error")?;
    ensure(
      missing_skip.contains("attempting to skip non-existent parameter"),
      "missing skip diagnostic is preserved",
    )?;

    let skip_all = generated_function(quote!(skip_all, fields(answer = 42)), quote! {
      fn demo(answer: u64, other: u64) {
        let _ = (answer, other);
      }
    })?;
    ensure(skip_all.contains("answer = 42"), "custom field remains when skip_all is set")?;
    ensure(!skip_all.contains("other = other"), "skip_all prevents automatic fields")
  }

  #[test]
  fn generated_sync_body_emits_err_and_ret_events_with_requested_modes() -> Result<(), TestFailure> {
    let output = generated_function(quote!(level = "warn", err(Debug, level = "error"), ret(Display)), quote! {
      fn demo(answer: Result<u64, Error>) -> Result<u64, Error> {
        answer
      }
    })?;

    ensure(
      output.contains("match (move ||"),
      "sync functions wrap the body for result instrumentation",
    )?;
    ensure(
      output.contains("error = ? __tracing_attr_error"),
      "err(Debug) records the error with debug formatting",
    )?;
    ensure(
      output.contains("return = % __tracing_attr_return"),
      "ret(Display) records the return value with display formatting",
    )?;
    ensure(output.contains(":: tracing :: Level :: ERROR"), "err level override is emitted")
  }

  #[test]
  fn generated_async_body_emits_future_instrumentation_and_events() -> Result<(), TestFailure> {
    let output = generated_function(quote!(err(Display), ret(Debug)), quote! {
      async fn demo(answer: Result<u64, Error>) -> Result<u64, Error> {
        answer
      }
    })?;

    ensure(
      output.contains("async move"),
      "async functions generate an async instrumentation wrapper",
    )?;
    ensure(
      output.contains(":: tracing :: Instrument :: instrument"),
      "enabled async spans instrument the generated future",
    )?;
    ensure(
      output.contains("error = % __tracing_attr_error"),
      "err(Display) records the error with display formatting",
    )?;
    ensure(
      output.contains("return = ? __tracing_attr_return"),
      "ret(Debug) records the return value with debug formatting",
    )
  }

  #[test]
  fn impl_trait_return_type_is_erased_for_the_fake_return_edge() -> Result<(), TestFailure> {
    let output = generated_function(quote!(), quote! {
      fn demo() -> impl Clone {
        1_u64
      }
    })?;

    ensure(
      output.contains("Option :: < _ > :: None"),
      "impl Trait return type is erased in the fake return edge",
    )
  }

  #[test]
  fn parameter_name_collection_handles_nested_patterns_without_inventing_fields() -> Result<(), TestFailure> {
    let item = parsed_fn(quote! {
      fn demo((first, Struct { second, nested: (third, _) }): Input, _: Ignored) {}
    })?;
    let params: Vec<String> = item
      .sig
      .inputs
      .iter()
      .cloned()
      .flat_map(|arg| match arg {
        FnArg::Typed(typed) => param_names(*typed.pat, super::RecordType::Debug),
        FnArg::Receiver(_) => param_names(syn::parse_quote!(self), super::RecordType::Debug),
      })
      .map(|(ident, _record_type)| ident.to_string())
      .collect();

    ensure(
      params == vec![String::from("first"), String::from("second"), String::from("third")],
      "nested irrefutable patterns expose only named bindings",
    )
  }

  #[test]
  fn async_info_detects_supported_async_trait_shapes_and_rejects_plain_functions() -> Result<(), TestFailure> {
    let plain_async = parsed_fn(quote! {
      async fn demo() {}
    })?;
    ensure(
      AsyncInfo::from_fn(&plain_async).is_none(),
      "native async fn does not look like async-trait output",
    )?;

    let async_block = parsed_fn(quote! {
      fn demo() -> impl Future<Output = u64> {
        async move { 1_u64 }
      }
    })?;
    ensure(AsyncInfo::from_fn(&async_block).is_some(), "async move return block is detected")?;

    let pinned_async_block = parsed_fn(quote! {
      fn demo() -> Pin<Box<dyn Future<Output = u64>>> {
        Box::pin(async move { 1_u64 })
      }
    })?;
    ensure(
      AsyncInfo::from_fn(&pinned_async_block).is_some(),
      "Box::pin(async move) return block is detected",
    )?;

    let old_async_trait = parsed_fn(quote! {
      fn demo(_self: &Demo) -> Pin<Box<dyn Future<Output = u64>>> {
        async fn inner(_self: &Demo) -> u64 {
          1_u64
        }
        Box::pin(inner(_self))
      }
    })?;
    let info = ensure_some(
      AsyncInfo::from_fn(&old_async_trait),
      "immediately invoked async function shape is detected",
    )?;
    let args = ensure_ok(
      syn::parse2::<InstrumentArgs>(quote!(fields(ty = core::mem::size_of::<Self>(), this = self))),
      "field rewrite args parse",
    )?;
    let output = info.gen_async(&args, "demo").to_string();
    ensure(output.contains("Box :: pin"), "old async-trait output remains pinned")?;
    ensure(
      output.contains("ty = core :: mem :: size_of :: < Demo > ()"),
      "Self type positions in custom fields are rewritten to concrete receiver type",
    )?;
    ensure(
      output.contains("this = _self"),
      "receiver binding in custom fields is rewritten to generated _self",
    )?;

    let nonmatching = parsed_fn(quote! {
      fn demo() -> u64 {
        1_u64
      }
    })?;
    ensure(
      AsyncInfo::from_fn(&nonmatching).is_none(),
      "plain synchronous function is not detected as async-trait output",
    )
  }

  #[test]
  fn event_args_default_levels_follow_span_or_error_defaults() -> Result<(), TestFailure> {
    let args = ensure_ok(
      syn::parse2::<InstrumentArgs>(quote!(level = "debug", err, ret)),
      "default event arguments parse",
    )?;
    let err_level = ensure_some(args.err_args.as_ref(), "err args exist")?.level(Level::Warn);
    let ret_level = ensure_some(args.ret_args.as_ref(), "ret args exist")?.level(args.level());

    ensure_eq(
      &err_level.to_token_stream().to_string(),
      &quote!(::tracing::Level::WARN).to_string(),
      "err event defaults to caller-provided error level",
    )?;
    ensure_eq(
      &ret_level.to_token_stream().to_string(),
      &quote!(::tracing::Level::DEBUG).to_string(),
      "ret event defaults to span level",
    )
  }
}
