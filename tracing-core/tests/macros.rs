//! Macro API compatibility coverage.

#[cfg(test)]
mod tests {
    use tracing_core::{
        callsite::Callsite,
        field::FieldSet,
        metadata,
        metadata::{Kind, Level, Metadata},
        subscriber::Interest,
    };

    #[test]
    fn metadata_macro_api() {
        // This test should catch any inadvertent breaking changes
        // caused by changes to the macro.
        struct TestCallsite;

        impl Callsite for TestCallsite {
            fn set_interest(&self, _: Interest) {}

            fn metadata(&self) -> &Metadata<'_> {
                &TEST_METADATA
            }
        }

        static CALLSITE: TestCallsite = TestCallsite;
        static TEST_METADATA: Metadata<'static> = Metadata::new(
            "test_metadata",
            "test_target",
            Level::DEBUG,
            None,
            None,
            None,
            &FieldSet::new(&[], tracing_core::identify_callsite!(&CALLSITE)),
            Kind::SPAN,
        );
        let _debug_metadata = metadata! {
            name: "test_metadata",
            target: "test_target",
            level: Level::DEBUG,
            fields: &["foo", "bar", "baz"],
            callsite: &CALLSITE,
            kind: Kind::SPAN,
        };
        let _trace_metadata = metadata! {
            name: "test_metadata",
            target: "test_target",
            level: Level::TRACE,
            fields: &[],
            callsite: &CALLSITE,
            kind: Kind::EVENT,
        };
        let _info_metadata = metadata! {
            name: "test_metadata",
            target: "test_target",
            level: Level::INFO,
            fields: &[],
            callsite: &CALLSITE,
            kind: Kind::EVENT
        };
    }
}
