//! Tests optional layer composition.
#![cfg(feature = "registry")]

#[cfg(test)]
mod tests {
    use strict_test_support::{TestFailure, ensure, ensure_ok};
    use tracing::subscriber::with_default;
    use tracing_core::{
        LevelFilter, Metadata, Subscriber,
        subscriber::{Interest, SubscriberResult},
    };
    use tracing_mock::layer::named;
    use tracing_subscriber::{layer, prelude::*, reload::Layer};

    #[derive(Debug)]
    struct BasicLayer(Option<LevelFilter>);

    impl<S: Subscriber> tracing_subscriber::Layer<S> for BasicLayer {
        fn register_callsite(
            &self,
            _metadata: &'static Metadata<'static>,
        ) -> SubscriberResult<Interest> {
            Ok(Interest::sometimes())
        }

        fn enabled(
            &self,
            _metadata: &Metadata<'_>,
            _: layer::Context<'_, S>,
        ) -> SubscriberResult<bool> {
            Ok(true)
        }

        fn max_level_hint(&self) -> SubscriberResult<Option<LevelFilter>> {
            Ok(self.0)
        }
    }

    fn ensure_hint<S>(
        subscriber: &S,
        expected: Option<LevelFilter>,
        context: &'static str,
    ) -> Result<(), TestFailure>
    where
        S: Subscriber,
    {
        ensure(subscriber.max_level_hint() == expected, context)
    }

    #[test]
    fn just_layer() -> Result<(), TestFailure> {
        let subscriber = tracing_subscriber::registry().with(LevelFilter::INFO);
        ensure_hint(
            &subscriber,
            Some(LevelFilter::INFO),
            "plain layer reports info",
        )
    }

    #[test]
    fn subscriber_and_option_some_layer() -> Result<(), TestFailure> {
        let subscriber = tracing_subscriber::registry()
            .with(LevelFilter::INFO)
            .with(Some(LevelFilter::DEBUG));
        ensure_hint(
            &subscriber,
            Some(LevelFilter::DEBUG),
            "Some layer overrides previous hint",
        )
    }

    #[test]
    fn subscriber_and_option_none_layer() -> Result<(), TestFailure> {
        let subscriber = tracing_subscriber::registry()
            .with(LevelFilter::ERROR)
            .with(None::<LevelFilter>);
        ensure_hint(
            &subscriber,
            Some(LevelFilter::ERROR),
            "None layer preserves previous hint",
        )
    }

    #[test]
    fn just_option_some_layer() -> Result<(), TestFailure> {
        let subscriber = tracing_subscriber::registry().with(None::<LevelFilter>);
        ensure_hint(
            &subscriber,
            Some(LevelFilter::OFF),
            "standalone None layer disables all levels",
        )
    }

    #[test]
    fn just_option_none_layer() -> Result<(), TestFailure> {
        let subscriber = tracing_subscriber::registry().with(Some(LevelFilter::ERROR));
        ensure_hint(
            &subscriber,
            Some(LevelFilter::ERROR),
            "standalone Some layer reports its level",
        )
    }

    #[test]
    fn none_outside_doesnt_override_max_level() -> Result<(), TestFailure> {
        let none_outside = tracing_subscriber::registry()
            .with(BasicLayer(None))
            .with(None::<LevelFilter>);
        ensure_hint(&none_outside, None, "outer None preserves inner None hint")?;

        let error_outside = tracing_subscriber::registry()
            .with(BasicLayer(None))
            .with(Some(LevelFilter::ERROR));
        ensure_hint(
            &error_outside,
            Some(LevelFilter::ERROR),
            "outer Some layer wins over inner None hint",
        )?;

        let debug_inside = tracing_subscriber::registry()
            .with(BasicLayer(Some(LevelFilter::DEBUG)))
            .with(None::<LevelFilter>);
        ensure_hint(
            &debug_inside,
            Some(LevelFilter::DEBUG),
            "outer None preserves inner debug hint",
        )?;

        let filtered_none = tracing_subscriber::registry()
            .with(BasicLayer(None))
            .with(None::<LevelFilter>.with_filter(LevelFilter::DEBUG));
        ensure_hint(
            &filtered_none,
            None,
            "filtered outer None preserves inner None hint",
        )?;

        let filtered_none_with_info = tracing_subscriber::registry()
            .with(BasicLayer(Some(LevelFilter::INFO)))
            .with(None::<LevelFilter>.with_filter(LevelFilter::DEBUG));
        ensure_hint(
            &filtered_none_with_info,
            Some(LevelFilter::DEBUG),
            "outer filter level wins over inner info hint",
        )?;

        let filtered_basic = tracing_subscriber::registry()
            .with(BasicLayer(Some(LevelFilter::INFO)).with_filter(LevelFilter::DEBUG))
            .with(None::<LevelFilter>);
        ensure_hint(
            &filtered_basic,
            Some(LevelFilter::DEBUG),
            "inner filtered layer level survives outer None",
        )?;

        let filtered_basic_none = tracing_subscriber::registry()
            .with(BasicLayer(None).with_filter(LevelFilter::DEBUG))
            .with(None::<LevelFilter>);
        ensure_hint(
            &filtered_basic_none,
            Some(LevelFilter::DEBUG),
            "inner filter level survives both None hints",
        )?;

        let info_inside = tracing_subscriber::registry()
            .with(BasicLayer(Some(LevelFilter::INFO)))
            .with(None::<LevelFilter>);
        ensure_hint(
            &info_inside,
            Some(LevelFilter::INFO),
            "outer None does not override inner info hint",
        )
    }

    #[test]
    fn none_inside_doesnt_override_max_level() -> Result<(), TestFailure> {
        let none_inside = tracing_subscriber::registry()
            .with(None::<LevelFilter>)
            .with(BasicLayer(None));
        ensure_hint(&none_inside, None, "inner None preserves outer None hint")?;

        let error_inside = tracing_subscriber::registry()
            .with(Some(LevelFilter::ERROR))
            .with(BasicLayer(None));
        ensure_hint(
            &error_inside,
            Some(LevelFilter::ERROR),
            "inner None preserves outer error hint",
        )?;

        let debug_basic = tracing_subscriber::registry()
            .with(None::<LevelFilter>)
            .with(BasicLayer(Some(LevelFilter::DEBUG)));
        ensure_hint(
            &debug_basic,
            Some(LevelFilter::DEBUG),
            "inner debug hint wins over outer None",
        )?;

        let filtered_none = tracing_subscriber::registry()
            .with(None::<LevelFilter>.with_filter(LevelFilter::DEBUG))
            .with(BasicLayer(None));
        ensure_hint(
            &filtered_none,
            None,
            "inner None wins over filtered outer None",
        )?;

        let filtered_none_with_info = tracing_subscriber::registry()
            .with(None::<LevelFilter>.with_filter(LevelFilter::DEBUG))
            .with(BasicLayer(Some(LevelFilter::INFO)));
        ensure_hint(
            &filtered_none_with_info,
            Some(LevelFilter::DEBUG),
            "outer filter level wins over inner info hint",
        )?;

        let filtered_basic = tracing_subscriber::registry()
            .with(None::<LevelFilter>)
            .with(BasicLayer(Some(LevelFilter::INFO)).with_filter(LevelFilter::DEBUG));
        ensure_hint(
            &filtered_basic,
            Some(LevelFilter::DEBUG),
            "inner filtered layer level wins over outer None",
        )?;

        let filtered_basic_none = tracing_subscriber::registry()
            .with(None::<LevelFilter>)
            .with(BasicLayer(None).with_filter(LevelFilter::DEBUG));
        ensure_hint(
            &filtered_basic_none,
            Some(LevelFilter::DEBUG),
            "inner filter level wins over both None hints",
        )?;

        let info_basic = tracing_subscriber::registry()
            .with(None::<LevelFilter>)
            .with(BasicLayer(Some(LevelFilter::INFO)));
        ensure_hint(
            &info_basic,
            Some(LevelFilter::INFO),
            "inner info hint wins over outer None",
        )
    }

    #[test]
    fn reload_works_with_none() -> Result<(), TestFailure> {
        let (layer1, handle1) = Layer::new(None::<BasicLayer>);
        let (layer2, _handle2) = Layer::new(None::<BasicLayer>);

        let subscriber = tracing_subscriber::registry().with(layer1).with(layer2);
        ensure_hint(
            &subscriber,
            Some(LevelFilter::OFF),
            "two empty reload layers start disabled",
        )?;

        ensure_ok(
            handle1.reload(Some(BasicLayer(None))),
            "reload layer accepts a None hint layer",
        )?;
        ensure_hint(
            &subscriber,
            None,
            "reloaded None hint passes through correctly",
        )?;

        ensure_ok(
            handle1.reload(Some(BasicLayer(Some(LevelFilter::DEBUG)))),
            "reload layer accepts a debug hint layer",
        )?;
        ensure_hint(
            &subscriber,
            Some(LevelFilter::DEBUG),
            "reloaded debug hint passes through correctly",
        )
    }

    #[test]
    fn on_register_dispatch_is_called() -> Result<(), TestFailure> {
        let (inner_layer, inner_handle) = named("inner").on_register_dispatch().run_with_handle();

        let subscriber = tracing_subscriber::registry().with(Some(inner_layer));
        with_default(subscriber, || {});

        ensure_ok(inner_handle.finished(), "mock expectations should finish")?;
        Ok(())
    }
}
