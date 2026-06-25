//! Verifies tracing macros consume moved field arguments exactly once.

#[cfg(test)]
mod tests {
    use tracing::{Level, event, span};

    /// Test that spans and events only use their argument once. See #196 and #1739.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[test]
    fn test_move_arg() {
        let parent_foo = Foo;
        let parent_span = span!(Level::INFO, "Span 1", bar = ?Bar(parent_foo));
        let child_foo = Foo;
        span!(parent: &parent_span, Level::INFO, "Span 2", bar = ?Bar(child_foo));

        let event_foo = Foo;
        event!(Level::INFO, bar = ?Bar(event_foo), "Event 1");
        let child_event_foo = Foo;
        event!(parent: &parent_span, Level::INFO, bar = ?Bar(child_event_foo), "Event 2");
    }

    #[derive(Debug)]
    struct Foo;

    #[derive(Debug)]
    struct Bar(Foo);
}
