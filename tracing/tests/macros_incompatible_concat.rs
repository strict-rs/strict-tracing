//! Macro concat compatibility coverage.

#[cfg(test)]
mod tests {
    use tracing::{Level, enabled, event, span};

    #[macro_export]
    /// Local macro that intentionally shadows `concat!`.
    macro_rules! concat {
        () => {};
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[test]
    fn span() {
        span!(Level::DEBUG, "foo");
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[test]
    fn event() {
        event!(Level::DEBUG, "foo");
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[test]
    fn enabled() {
        enabled!(Level::DEBUG);
    }
}
