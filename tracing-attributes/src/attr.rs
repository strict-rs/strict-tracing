/// Arguments to `#[instrument(err(...))]` and `#[instrument(ret(...))]` which describe how the
/// return value event should be emitted.
#[derive(Clone, Default, Debug)]
struct EventArgs {
    /// Optional event level override for `err` or `ret` events.
    level: Option<Level>,
    /// Formatting mode used for the emitted value or error field.
    mode: FormatMode,
}

/// Parsed arguments supplied to the `#[instrument(...)]` attribute.
#[derive(Clone, Default, Debug)]
struct InstrumentArgs {
    /// Optional span level override.
    level: Option<Level>,
    /// Optional span name override.
    name: Option<LitStrOrIdent>,
    /// Optional span target override.
    target: Option<LitStrOrIdent>,
    /// Optional explicit parent span expression.
    parent: Option<Expr>,
    /// Optional `follows_from` causal span expression.
    follows_from: Option<Expr>,
    /// Function parameters that should not be recorded as fields.
    skips: Vec<Ident>,
    /// Whether all function parameters should be skipped.
    skip_all: bool,
    /// Custom fields supplied through `fields(...)`.
    fields: Option<Fields>,
    /// Optional configuration for an emitted error event.
    err_args: Option<EventArgs>,
    /// Optional configuration for an emitted return-value event.
    ret_args: Option<EventArgs>,
    /// Errors describing any unrecognized parse inputs that we skipped.
    parse_warnings: Vec<syn::Error>,
}

impl InstrumentArgs {
    /// Return the configured span level, defaulting to `INFO`.
    fn level(&self) -> Level {
        self.level.clone().unwrap_or(Level::Info)
    }

    /// Return the configured span target tokens, defaulting to `module_path!()`.
    fn target(&self) -> TokenStream {
        self.target
            .as_ref()
            .map_or_else(|| quote!(module_path!()), |target| quote!(#target))
    }

    /// Generate "deprecation" warnings for any unrecognized attribute inputs
    /// that we skipped.
    ///
    /// For backwards compatibility, we need to emit compiler warnings rather
    /// than errors for unrecognized inputs. Generating a fake deprecation is
    /// the only way to do this on stable Rust right now.
    fn warnings(&self) -> impl ToTokens + use<> {
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

impl Parse for InstrumentArgs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut args = Self::default();
        while !input.is_empty() {
            let lookahead = input.lookahead1();
            if lookahead.peek(kw::name) {
                if args.name.is_some() {
                    return Err(input.error("expected only a single `name` argument"));
                }
                let name = input.parse::<StrArg<kw::name>>()?.value;
                args.name = Some(name);
            } else if lookahead.peek(LitStr) {
                // XXX: apparently we support names as either named args with an
                // sign, _or_ as unnamed string literals. That's weird, but
                // changing it is apparently breaking.
                // This also means that when using idents for name, it must be via
                // a named arg, i.e. `#[instrument(name = SOME_IDENT)]`.
                if args.name.is_some() {
                    return Err(input.error("expected only a single `name` argument"));
                }
                args.name = Some(input.parse()?);
            } else if lookahead.peek(kw::target) {
                if args.target.is_some() {
                    return Err(input.error("expected only a single `target` argument"));
                }
                let target = input.parse::<StrArg<kw::target>>()?.value;
                args.target = Some(target);
            } else if lookahead.peek(kw::parent) {
                if args.target.is_some() {
                    return Err(input.error("expected only a single `parent` argument"));
                }
                let parent = input.parse::<ExprArg<kw::parent>>()?;
                args.parent = Some(parent.value);
            } else if lookahead.peek(kw::follows_from) {
                if args.target.is_some() {
                    return Err(input.error("expected only a single `follows_from` argument"));
                }
                let follows_from = input.parse::<ExprArg<kw::follows_from>>()?;
                args.follows_from = Some(follows_from.value);
            } else if lookahead.peek(kw::level) {
                if args.level.is_some() {
                    return Err(input.error("expected only a single `level` argument"));
                }
                args.level = Some(input.parse()?);
            } else if lookahead.peek(kw::skip) {
                if !args.skips.is_empty() {
                    return Err(input.error("expected only a single `skip` argument"));
                }
                if args.skip_all {
                    return Err(input.error("expected either `skip` or `skip_all` argument"));
                }
                let Skips(skips) = input.parse()?;
                args.skips = skips;
            } else if lookahead.peek(kw::skip_all) {
                if args.skip_all {
                    return Err(input.error("expected only a single `skip_all` argument"));
                }
                if !args.skips.is_empty() {
                    return Err(input.error("expected either `skip` or `skip_all` argument"));
                }
                let _skip_all: kw::skip_all = input.parse()?;
                args.skip_all = true;
            } else if lookahead.peek(kw::fields) {
                if args.fields.is_some() {
                    return Err(input.error("expected only a single `fields` argument"));
                }
                args.fields = Some(input.parse()?);
            } else if lookahead.peek(kw::err) {
                let _err: kw::err = input.parse()?;
                let err_args = EventArgs::parse(input)?;
                args.err_args = Some(err_args);
            } else if lookahead.peek(kw::ret) {
                let _ret: kw::ret = input.parse()?;
                let ret_args = EventArgs::parse(input)?;
                args.ret_args = Some(ret_args);
            } else if lookahead.peek(Token![,]) {
                let _comma: Token![,] = input.parse()?;
            } else {
                // We found a token that we didn't expect!
                // We want to emit warnings for these, rather than errors, so
                // we'll add it to the list of unrecognized inputs we've seen so
                // far and keep going.
                args.parse_warnings.push(lookahead.error());
                // Parse the unrecognized token tree to advance the parse
                // stream, and throw it away so we can keep parsing.
                let _unknown: proc_macro2::TokenTree = input.parse()?;
            }
        }
        Ok(args)
    }
}

impl EventArgs {
    /// Return the configured event level, falling back to the caller-provided default.
    fn level(&self, default: Level) -> Level {
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
                if result.level.is_some() {
                    return Err(content.error("expected only a single `level` argument"));
                }
                result.level = Some(content.parse()?);
                return Ok(());
            }

            if result.mode != FormatMode::default() {
                return Err(content.error("expected only a single format argument"));
            }

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
enum LitStrOrIdent {
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
    _p: PhantomData<T>,
}

impl<T: Parse> Parse for StrArg<T> {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let _keyword: T = input.parse()?;
        let _eq: Token![=] = input.parse()?;
        let value = input.parse()?;
        Ok(Self {
            value,
            _p: PhantomData,
        })
    }
}

/// Parser for `keyword = expr` arguments.
struct ExprArg<T> {
    /// Parsed expression value.
    value: Expr,
    /// Marker tying this parser to the expected custom keyword.
    _p: PhantomData<T>,
}

impl<T: Parse> Parse for ExprArg<T> {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let _keyword: T = input.parse()?;
        let _eq: Token![=] = input.parse()?;
        let value = input.parse()?;
        Ok(Self {
            value,
            _p: PhantomData,
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
enum FormatMode {
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
struct Fields(
    /// Comma-separated custom field definitions.
    Punctuated<Field, Token![,]>,
);

/// One parsed field entry from `fields(...)`.
#[derive(Clone, Debug)]
struct Field {
    /// Field name, either dotted identifiers or `{expr}`.
    name: FieldName,
    /// Optional explicit field value after `=`.
    value: Option<Expr>,
    /// Formatting sigil or value mode for this field.
    kind: FieldKind,
}

/// Formatting mode for a custom field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FieldKind {
    /// Format the field with `?`.
    Debug,
    /// Format the field with `%`.
    Display,
    /// Record the field as a `tracing::Value`.
    Value,
}

/// Parsed custom field name.
#[derive(Clone, Debug)]
enum FieldName {
    /// Field name from the `{expr}` dynamic-name form.
    Expr(Expr),
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
            FieldName::Expr(expr)
        } else {
            FieldName::Punctuated(Punctuated::parse_separated_nonempty_with(
                input,
                Ident::parse_any,
            )?)
        };
        let value = if input.peek(Token![=]) {
            let _eq: Token![=] = input.parse()?;
            if let Some(value_kind) = parse_field_kind_prefix(input)? {
                kind = value_kind;
            }
            Some(input.parse()?)
        } else {
            None
        };
        Ok(Self { name, value, kind })
    }
}

impl ToTokens for Field {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        if let Some(ref value) = self.value {
            let name = &self.name;
            let kind = &self.kind;
            tokens.extend(quote! {
                #name = #kind #value
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
enum Level {
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
                Err(input.error(
                    "unknown verbosity level, expected one of \"trace\", \
                     \"debug\", \"info\", \"warn\", or \"error\", or a number 1-5",
                ))
            }
        } else if lookahead.peek(LitInt) {
            fn is_level(lit: &LitInt, expected: u64) -> bool {
                lit.base10_parse::<u64>()
                    .is_ok_and(|value| value == expected)
            }
            let level: LitInt = input.parse()?;
            match &level {
                literal if is_level(literal, 1) => Ok(Self::Trace),
                literal if is_level(literal, 2) => Ok(Self::Debug),
                literal if is_level(literal, 3) => Ok(Self::Info),
                literal if is_level(literal, 4) => Ok(Self::Warn),
                literal if is_level(literal, 5) => Ok(Self::Error),
                _ => Err(input.error(
                    "unknown verbosity level, expected one of \"trace\", \
                     \"debug\", \"info\", \"warn\", or \"error\", or a number 1-5",
                )),
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
