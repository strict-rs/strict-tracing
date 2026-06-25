fn main() {
    let subscriber = tracing_subscriber::registry();
    <tracing_subscriber::Registry as tracing_subscriber::util::SubscriberInitExt>::init(subscriber);
}
