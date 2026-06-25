//! Example binary for tracing workspace checks.
#![cfg(test)]

use strict_test_support::{TestFailure, ensure_ok};
use tracing::{field::display, subscriber::with_default};
use tracing_attributes::instrument;
use tracing_mock::{expect, span::NewSpan, subscriber};

#[instrument(fields(foo = "bar", dsa = true, num = 1))]
#[allow(
    clippy::single_call_fn,
    reason = "field fixture remains a named instrumented function so literal custom fields can be asserted"
)]
fn fn_no_param() {}

#[instrument(fields(foo = "bar"))]
#[allow(
    clippy::single_call_fn,
    reason = "field fixture remains a named instrumented function so parameters and custom fields can be asserted"
)]
fn fn_param(param: u32) {}

#[instrument(fields(foo = "bar", empty))]
#[allow(
    clippy::single_call_fn,
    reason = "field fixture remains a named instrumented function so empty custom fields can be asserted"
)]
fn fn_empty_field() {}

#[instrument(fields(s = text, len = text.len()), skip(text))]
#[allow(
    clippy::single_call_fn,
    reason = "field fixture remains a named instrumented function so expression fields can be asserted"
)]
fn fn_expr_field(text: &str) {}

#[instrument(fields(s = text, s.len = text.len(), s.is_empty = text.is_empty()), skip(text))]
#[allow(
    clippy::single_call_fn,
    reason = "field fixture remains a named instrumented function so dotted expression fields can be asserted"
)]
fn fn_two_expr_fields(text: &str) {}

#[instrument(fields(s = %text, s.len = text.len()), skip(text))]
#[allow(
    clippy::single_call_fn,
    reason = "field fixture remains a named instrumented function so display expression fields can be asserted"
)]
fn fn_clashy_expr_field(text: &str) {}

#[instrument(fields(s = "s"), skip(text))]
#[allow(
    clippy::single_call_fn,
    reason = "field fixture remains a named instrumented function so literal field replacement can be asserted"
)]
fn fn_clashy_expr_field2(text: &str) {
    let observed_text = String::from(text);
    drop(observed_text);
}

#[instrument(fields(s = &text), skip(text))]
#[allow(
    clippy::single_call_fn,
    reason = "field fixture remains a named instrumented function so borrowed string fields can be asserted"
)]
fn fn_string(text: String) {
    drop(text);
}

#[instrument(fields(keywords.impl.type.fn = arg), skip(arg))]
#[allow(
    clippy::single_call_fn,
    reason = "field fixture remains a named instrumented function so keyword path field names can be asserted"
)]
fn fn_keyword_ident_in_field(arg: &str) {}

const CONST_FIELD_NAME: &str = "foo.bar";

#[instrument(fields({CONST_FIELD_NAME} = "baz"))]
#[allow(
    clippy::single_call_fn,
    reason = "field fixture remains a named instrumented function so const expression field names can be asserted"
)]
fn fn_const_field_name() {}

#[allow(
    clippy::single_call_fn,
    reason = "const field-name fixture must remain callable from the instrument field expression under test"
)]
const fn get_const_fn_field_name() -> &'static str {
    "foo.bar"
}

#[instrument(fields({get_const_fn_field_name()} = "baz"))]
#[allow(
    clippy::single_call_fn,
    reason = "field fixture remains a named instrumented function so const fn field names can be asserted"
)]
fn fn_const_fn_field_name() {}

struct FieldNames;
impl FieldNames {
    const FOO_BAR: &'static str = "foo.bar";
}

#[instrument(fields({FieldNames::FOO_BAR} = "baz"))]
#[allow(
    clippy::single_call_fn,
    reason = "field fixture remains a named instrumented function so associated const field names can be asserted"
)]
fn fn_struct_const_field_name() {}

#[instrument(fields({"foo"} = "bar"))]
#[allow(
    clippy::single_call_fn,
    reason = "field fixture remains a named instrumented function so string literal field names can be asserted"
)]
fn fn_string_field_name() {}

const CLASHY_FIELD_NAME: &str = "s";

#[instrument(fields(s = text, {CLASHY_FIELD_NAME} = "foo"), skip(text))]
#[allow(
    clippy::single_call_fn,
    reason = "field fixture remains a named instrumented function so duplicate const field names can be asserted"
)]
fn fn_clashy_const_field_name(text: &str) {
    let observed_text = String::from(text);
    drop(observed_text);
}

#[derive(Debug)]
struct HasField {
    my_field: &'static str,
}

impl HasField {
    #[instrument(fields(my_field = self.my_field), skip(self))]
    fn self_expr_field(&self) {}
}

#[test]
fn fields() -> Result<(), TestFailure> {
    let span = expect::span().with_fields(
        expect::field("foo")
            .with_value(&"bar")
            .and(expect::field("dsa").with_value(&true))
            .and(expect::field("num").with_value(&1))
            .only(),
    );
    run_test(span, || {
        fn_no_param();
    })?;
    Ok(())
}

#[test]
fn expr_field() -> Result<(), TestFailure> {
    let span = expect::span().with_fields(
        expect::field("s")
            .with_value(&"hello world")
            .and(expect::field("len").with_value(&"hello world".len()))
            .only(),
    );
    run_test(span, || {
        fn_expr_field("hello world");
    })?;
    Ok(())
}

#[test]
fn two_expr_fields() -> Result<(), TestFailure> {
    let span = expect::span().with_fields(
        expect::field("s")
            .with_value(&"hello world")
            .and(expect::field("s.len").with_value(&"hello world".len()))
            .and(expect::field("s.is_empty").with_value(&false))
            .only(),
    );
    run_test(span, || {
        fn_two_expr_fields("hello world");
    })?;
    Ok(())
}

#[test]
fn clashy_expr_field() -> Result<(), TestFailure> {
    let span = expect::span().with_fields(
        // Overriding the `s` field should record `s` as a `Display` value,
        // rather than as a `Debug` value.
        expect::field("s")
            .with_value(&display("hello world"))
            .and(expect::field("s.len").with_value(&"hello world".len()))
            .only(),
    );
    run_test(span, || {
        fn_clashy_expr_field("hello world");
    })?;

    let clashy_literal_span =
        expect::span().with_fields(expect::field("s").with_value(&"s").only());
    run_test(clashy_literal_span, || {
        fn_clashy_expr_field2("hello world");
    })?;
    Ok(())
}

#[test]
fn self_expr_field() -> Result<(), TestFailure> {
    let span =
        expect::span().with_fields(expect::field("my_field").with_value(&"hello world").only());
    run_test(span, || {
        let has_field = HasField {
            my_field: "hello world",
        };
        has_field.self_expr_field();
    })?;
    Ok(())
}

#[test]
fn parameters_with_fields() -> Result<(), TestFailure> {
    let span = expect::span().with_fields(
        expect::field("foo")
            .with_value(&"bar")
            .and(expect::field("param").with_value(&1_u32))
            .only(),
    );
    run_test(span, || {
        fn_param(1);
    })?;
    Ok(())
}

#[test]
fn empty_field() -> Result<(), TestFailure> {
    let span = expect::span().with_fields(expect::field("foo").with_value(&"bar").only());
    run_test(span, || {
        fn_empty_field();
    })?;
    Ok(())
}

#[test]
fn string_field() -> Result<(), TestFailure> {
    let span = expect::span().with_fields(expect::field("s").with_value(&"hello world").only());
    run_test(span, || {
        fn_string(String::from("hello world"));
    })?;
    Ok(())
}

#[test]
fn keyword_ident_in_field_name() -> Result<(), TestFailure> {
    let span = expect::span().with_fields(
        expect::field("keywords.impl.type.fn")
            .with_value(&"test")
            .only(),
    );
    run_test(span, || fn_keyword_ident_in_field("test"))?;
    Ok(())
}

#[test]
fn expr_const_field_name() -> Result<(), TestFailure> {
    let span = expect::span().with_fields(expect::field("foo.bar").with_value(&"baz").only());
    run_test(span, || {
        fn_const_field_name();
    })?;
    Ok(())
}

#[test]
fn expr_const_fn_field_name() -> Result<(), TestFailure> {
    let span = expect::span().with_fields(expect::field("foo.bar").with_value(&"baz").only());
    run_test(span, || {
        fn_const_fn_field_name();
    })?;
    Ok(())
}

#[test]
fn struct_const_field_name() -> Result<(), TestFailure> {
    let span = expect::span().with_fields(expect::field("foo.bar").with_value(&"baz").only());
    run_test(span, || {
        fn_struct_const_field_name();
    })?;
    Ok(())
}

#[test]
fn string_field_name() -> Result<(), TestFailure> {
    let span = expect::span().with_fields(expect::field("foo").with_value(&"bar").only());
    run_test(span, || {
        fn_string_field_name();
    })?;
    Ok(())
}

#[test]
fn clashy_const_field_name() -> Result<(), TestFailure> {
    let span = expect::span().with_fields(
        // #3158: To be consistent with event! and span! macros, the duplicated value should be
        // dropped, but checking for duplicated fields would incur a significant runtime cost, as
        // non-trivial constants (const, const fn, ...) cannot be evaluated at compile time.
        expect::field("s")
            .with_value(&"foo")
            .and(expect::field("s").with_value(&"hello world")),
    );
    run_test(span, || {
        fn_clashy_const_field_name("hello world");
    })?;
    Ok(())
}

fn run_test<F: FnOnce() -> T, T>(span: NewSpan, fun: F) -> Result<(), TestFailure> {
    let (subscriber, handle) = subscriber::mock()
        .new_span(span)
        .enter(expect::span())
        .exit(expect::span())
        .only()
        .run_with_handle();

    let _result = with_default(subscriber, fun);
    ensure_ok(handle.finished(), "mock expectations should finish")?;
    Ok(())
}
