use tracing_mock::{expect, subscriber};

fn main() {
    let (_subscriber, _handle) = subscriber::mock()
        .event(expect::event())
        .run_with_handle();

    let _removed = subscriber::MockHandle::assert_finished;
}
