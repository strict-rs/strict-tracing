//! Executor instrumentation implementations.

#[cfg(feature = "futures-01")]
/// Instrumentation for `futures` 0.1 executors.
mod futures_01;

#[cfg(feature = "futures-03")]
/// Instrumentation for `futures` 0.3 task spawners.
mod futures_03;
