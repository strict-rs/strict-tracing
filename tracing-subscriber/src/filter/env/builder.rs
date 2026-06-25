use super::{
    directive::{self, Directive},
    EnvFilter, FromEnvError,
};
use crate::{filter::ParseError, RwLock};
use alloc::{format, string::String, vec::Vec};
use core::fmt;
use std::{collections::HashMap, env, io, iter};
use thread_local::ThreadLocal;
use tracing::level_filters::STATIC_MAX_LEVEL;

/// A [builder] for constructing new [`EnvFilter`]s.
///
/// [builder]: https://rust-unofficial.github.io/patterns/patterns/creational/builder.html
#[derive(Debug, Clone)]
#[must_use]
pub struct Builder {
    /// Whether value matchers parse non-literal values as regular expressions.
    regex: bool,
    /// The environment variable read by environment parsing methods.
    env: Option<String>,
    /// The directive used when parsing yields no directives.
    default_directive: Option<Directive>,
}

impl Builder {
    /// Sets whether span field values can be matched with regular expressions.
    ///
    /// If this is `true`, field filter directives will be interpreted as
    /// regular expressions if they are not able to be interpreted as a `bool`,
    /// `i64`, `u64`, or `f64` literal. If this is `false,` those field values
    /// will be interpreted as literal [`std::fmt::Debug`] output instead.
    ///
    /// By default, regular expressions are enabled.
    ///
    /// **Note**: when [`EnvFilter`]s are constructed from untrusted inputs,
    /// disabling regular expressions is strongly encouraged.
    pub fn with_regex(self, regex: bool) -> Self {
        Self { regex, ..self }
    }

    /// Sets a default [filtering directive] that will be added to the filter if
    /// the parsed string or environment variable contains no filter directives.
    ///
    /// By default, there is no default directive.
    ///
    /// # Examples
    ///
    /// If [`parse`], [`parse_lossy`], [`parse_env`], or [`parse_env_lossy`] are
    /// called with an empty string or environment variable, the default
    /// directive is used instead:
    ///
    /// ```rust
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// use tracing_subscriber::filter::{EnvFilter, LevelFilter};
    ///
    /// let filter = EnvFilter::builder()
    ///     .with_default_directive(LevelFilter::INFO.into())
    ///     .parse("")?;
    ///
    /// if format!("{}", filter) != "info" {
    ///     return Err("default directive should be used for an empty filter".into());
    /// }
    /// # Ok(()) }
    /// ```
    ///
    /// Note that the `lossy` variants ([`parse_lossy`] and [`parse_env_lossy`])
    /// will ignore any invalid directives. If all directives in a filter
    /// string or environment variable are invalid, those methods will also use
    /// the default directive:
    ///
    /// ```rust
    /// use tracing_subscriber::filter::{EnvFilter, LevelFilter};
    ///
    /// let filter = EnvFilter::builder()
    ///     .with_default_directive(LevelFilter::INFO.into())
    ///     .parse_lossy("some_target=fake level,foo::bar=lolwut");
    ///
    /// if format!("{}", filter) != "info" {
    ///     return Err("lossy parsing should fall back to the default directive".into());
    /// }
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    ///
    /// If the string or environment variable contains valid filtering
    /// directives, the default directive is not used:
    ///
    /// ```rust
    /// use tracing_subscriber::filter::{EnvFilter, LevelFilter};
    ///
    /// let filter = EnvFilter::builder()
    ///     .with_default_directive(LevelFilter::INFO.into())
    ///     .parse_lossy("foo=trace");
    ///
    /// // The default directive is *not* used:
    /// if format!("{}", filter) != "foo=trace" {
    ///     return Err("valid directives should replace the default directive".into());
    /// }
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// Parsing a more complex default directive from a string:
    ///
    /// ```rust
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// use tracing_subscriber::filter::{EnvFilter, LevelFilter};
    ///
    /// let default = "myapp=debug".parse()?;
    ///
    /// let filter = EnvFilter::builder()
    ///     .with_default_directive(default)
    ///     .parse("")?;
    ///
    /// if format!("{}", filter) != "myapp=debug" {
    ///     return Err("parsed default directive should be preserved".into());
    /// }
    /// # Ok(()) }
    /// ```
    ///
    /// [`parse_lossy`]: Self::parse_lossy
    /// [`parse_env_lossy`]: Self::parse_env_lossy
    /// [`parse`]: Self::parse
    /// [`parse_env`]: Self::parse_env
    pub fn with_default_directive(self, default_directive: Directive) -> Self {
        Self {
            default_directive: Some(default_directive),
            ..self
        }
    }

    /// Sets the name of the environment variable used by the [`parse_env`],
    /// [`parse_env_lossy`], and [`try_parse_env`] methods.
    ///
    /// By default, this is the value of [`EnvFilter::DEFAULT_ENV`]
    /// (`RUST_LOG`).
    ///
    /// [`parse_env`]: Self::parse_env
    /// [`parse_env_lossy`]: Self::parse_env_lossy
    /// [`try_parse_env`]: Self::try_parse_env
    pub fn with_env_var(self, var: impl Into<String>) -> Self {
        Self {
            env: Some(var.into()),
            ..self
        }
    }

    /// Returns a new [`EnvFilter`] from the directives in the given string,
    /// *ignoring* any that are invalid.
    ///
    /// If `parse_lossy` is called with an empty string, then the
    /// [default directive] is used instead.
    ///
    /// [default directive]: Self::with_default_directive
    pub fn parse_lossy<Directives: AsRef<str>>(&self, directives: Directives) -> EnvFilter {
        let parsed_directives = directive::split_directives(directives.as_ref())
            .filter(|directive| !directive.is_empty())
            .filter_map(|directive| match Directive::parse(directive, self.regex) {
                Ok(parsed) => Some(parsed),
                Err(err) => {
                    write_stderr_line(format_args!("ignoring `{directive}`: {err}"));
                    None
                }
            });
        self.build_from_directives(parsed_directives)
    }

    /// Returns a new [`EnvFilter`] from the directives in the given string,
    /// or an error if any are invalid.
    ///
    /// If `parse` is called with an empty string, then the [default directive]
    /// is used instead.
    ///
    /// # Errors
    ///
    /// Returns an error if any non-empty directive cannot be parsed.
    ///
    /// [default directive]: Self::with_default_directive
    pub fn parse<Directives: AsRef<str>>(
        &self,
        directives: Directives,
    ) -> Result<EnvFilter, ParseError> {
        let directive_spec = directives.as_ref();
        if directive_spec.is_empty() {
            return Ok(self.build_from_directives(iter::empty()));
        }
        let parsed_directives = directive::split_directives(directive_spec)
            .filter(|directive| !directive.is_empty())
            .map(|directive| Directive::parse(directive, self.regex))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(self.build_from_directives(parsed_directives))
    }

    /// Returns a new [`EnvFilter`] from the directives in the configured
    /// environment variable, ignoring any directives that are invalid.
    ///
    /// If the environment variable is empty, then the [default directive]
    /// is used instead.
    ///
    /// [default directive]: Self::with_default_directive
    #[must_use]
    pub fn parse_env_lossy(&self) -> EnvFilter {
        let var = env::var(self.env_var_name()).unwrap_or_default();
        self.parse_lossy(var)
    }

    /// Returns a new [`EnvFilter`] from the directives in the configured
    /// environment variable. If the environment variable is unset, no directive is added.
    ///
    /// An error is returned if the environment contains invalid directives.
    ///
    /// If the environment variable is empty, then the [default directive]
    /// is used instead.
    ///
    /// # Errors
    ///
    /// Returns an error if the configured environment variable contains any
    /// invalid directives.
    ///
    /// [default directive]: Self::with_default_directive
    pub fn parse_env(&self) -> Result<EnvFilter, FromEnvError> {
        let var = env::var(self.env_var_name()).unwrap_or_default();
        self.parse(var).map_err(Into::into)
    }

    /// Returns a new [`EnvFilter`] from the directives in the configured
    /// environment variable, or an error if the environment variable is not set
    /// or contains invalid directives.
    ///
    /// If the environment variable is empty, then the [default directive]
    /// is used instead.
    ///
    /// # Errors
    ///
    /// Returns an error if the configured environment variable is unset or
    /// contains any invalid directives.
    ///
    /// [default directive]: Self::with_default_directive
    pub fn try_parse_env(&self) -> Result<EnvFilter, FromEnvError> {
        let var = env::var(self.env_var_name())?;
        self.parse(var).map_err(Into::into)
    }

    /// Builds an `EnvFilter` from parsed directives.
    pub(super) fn build_from_directives(
        &self,
        directives: impl IntoIterator<Item = Directive>,
    ) -> EnvFilter {
        let mut parsed_directives: Vec<_> = directives.into_iter().collect();
        let mut disabled = Vec::new();
        for directive in &mut parsed_directives {
            if directive.level > STATIC_MAX_LEVEL {
                disabled.push(directive.clone());
            }
            if !self.regex {
                directive.deregexify();
            }
        }

        if !disabled.is_empty() {
            emit_static_max_level_warnings(disabled);
        }

        let (dynamics, statics) = Directive::make_tables(parsed_directives);
        let has_dynamics = !dynamics.is_empty();

        let mut filter = EnvFilter {
            statics,
            dynamics,
            has_dynamics,
            by_id: RwLock::new(HashMap::default()),
            by_cs: RwLock::new(HashMap::default()),
            scope: ThreadLocal::new(),
            regex: self.regex,
        };

        if !has_dynamics
            && filter.statics.is_empty()
            && let Some(default) = self.default_directive.as_ref()
        {
            filter = filter.add_directive(default.clone());
        }

        filter
    }

    /// Returns the configured environment variable name.
    fn env_var_name(&self) -> &str {
        self.env.as_deref().unwrap_or(EnvFilter::DEFAULT_ENV)
    }
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            regex: true,
            env: None,
            default_directive: None,
        }
    }
}

/// Emits warnings for directives disabled by the statically configured max level.
#[allow(
    clippy::single_call_fn,
    reason = "static max-level warning assembly is kept separate from parsing control flow"
)]
fn emit_static_max_level_warnings(disabled: Vec<Directive>) {
    use tracing::Level;

    warn_static_max_level(
        "some trace filter directives would enable traces that are disabled statically",
    );
    for directive in disabled {
        let target = directive
            .target
            .as_ref()
            .map_or_else(|| "all targets".into(), |target| {
                format!("the `{target}` target")
            });
        let Some(level) = directive.level.into_level() else {
            continue;
        };
        write_static_max_context(&format!(
            "`{directive}` would enable the {level} level for {target}"
        ));
    }
    write_static_max_prefixed(
        "note:",
        &format!("the static max level is `{STATIC_MAX_LEVEL}`"),
    );

    let (feature, earlier_level) = match STATIC_MAX_LEVEL.into_level() {
        Some(Level::TRACE) => return,
        Some(Level::DEBUG) => ("max_level_debug", format!("{} ", Level::TRACE)),
        Some(Level::INFO) => ("max_level_info", format!("{} ", Level::DEBUG)),
        Some(Level::WARN) => ("max_level_warn", format!("{} ", Level::INFO)),
        Some(Level::ERROR) => ("max_level_error", format!("{} ", Level::WARN)),
        None => ("max_level_off", String::new()),
    };
    write_static_max_prefixed("help:", &format!(
        "to enable {earlier_level}logging, remove the `{feature}` feature from the `tracing` crate"
    ));
}

/// Emits a warning line for the static max-level diagnostic.
#[allow(
    clippy::single_call_fn,
    reason = "warning formatting owns the feature-specific ANSI styling for static max-level diagnostics"
)]
fn warn_static_max_level(message: &str) {
    #[cfg(not(feature = "nu-ansi-term"))]
    let formatted_message = format!("warning: {}", message);
    #[cfg(feature = "nu-ansi-term")]
    let formatted_message = {
        use nu_ansi_term::{Color, Style};

        let bold = Style::new().bold();
        let mut warning = Color::Yellow.paint("warning");
        warning.style_ref_mut().is_bold = true;
        format!("{}{} {}", warning, bold.paint(":"), bold.paint(message))
    };
    write_stderr_line(format_args!("{formatted_message}"));
}

/// Emits a note line for the static max-level diagnostic.
#[allow(
    clippy::single_call_fn,
    reason = "context formatting owns the feature-specific ANSI styling for static max-level diagnostics"
)]
fn write_static_max_context(message: &str) {
    #[cfg(not(feature = "nu-ansi-term"))]
    let formatted_message = format!("note: {}", message);
    #[cfg(feature = "nu-ansi-term")]
    let formatted_message = {
        use nu_ansi_term::Color;

        let mut pipe = Color::Fixed(21).paint("|");
        pipe.style_ref_mut().is_bold = true;
        format!(" {pipe} {message}")
    };
    write_stderr_line(format_args!("{formatted_message}"));
}

/// Emits a prefixed diagnostic line for the static max-level diagnostic.
fn write_static_max_prefixed(prefix: &str, message: &str) {
    #[cfg(not(feature = "nu-ansi-term"))]
    let formatted_message = format!("{} {}", prefix, message);
    #[cfg(feature = "nu-ansi-term")]
    let formatted_message = {
        use nu_ansi_term::{Color, Style};

        let mut equal = Color::Fixed(21).paint("=");
        equal.style_ref_mut().is_bold = true;
        format!(
            " {} {} {}",
            equal,
            Style::new().bold().paint(prefix),
            message
        )
    };
    write_stderr_line(format_args!("{formatted_message}"));
}

/// Writes one diagnostic line to standard error.
fn write_stderr_line(args: fmt::Arguments<'_>) {
    use io::Write as _;

    let mut stderr = io::stderr();
    if stderr.write_fmt(args).is_ok() {
        let _result = stderr.write_all(b"\n");
    }
}
