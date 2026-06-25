fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    tracing_subscriber::fmt().try_init()?;
    Ok(())
}
