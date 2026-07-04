use std::io;
use std::io::Write;
use std::thread;

use crossbeam_channel::Receiver;
use crossbeam_channel::RecvError;
use crossbeam_channel::TryRecvError;

use crate::Msg;

/// Background writer that drains queued log lines into an inner writer.
pub(super) struct Worker<T: Write + Send + 'static> {
  /// Destination writer receiving log lines from the queue.
  writer:   T,
  /// Receiver for queued log lines and shutdown messages.
  receiver: Receiver<Msg>,
  /// Zero-capacity channel used to acknowledge shutdown completion.
  shutdown: Receiver<()>,
}

/// Result of one worker receive/drain step.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum WorkerState {
  /// No more messages are currently buffered.
  Empty,
  /// All senders have disconnected.
  Disconnected,
  /// A log line was written and more messages may be available.
  Continue,
  /// A shutdown message was received.
  Shutdown,
}

impl<T: Write + Send + 'static> Worker<T> {
  /// Creates a worker around the queue receiver, destination writer, and shutdown channel.
  #[allow(
    clippy::single_call_fn,
    reason = "constructor keeps worker channel ownership private while non_blocking wires thread startup"
  )]
  pub(super) const fn new(receiver: Receiver<Msg>, writer: T, shutdown: Receiver<()>) -> Self {
    Self {
      writer,
      receiver,
      shutdown,
    }
  }

  /// Handles the blocking receive that starts a batch.
  fn handle_recv(&mut self, result: Result<Msg, RecvError>) -> io::Result<WorkerState> {
    match result {
      Ok(Msg::Line(msg)) => {
        self.writer.write_all(&msg)?;
        Ok(WorkerState::Continue)
      }
      Ok(Msg::Shutdown) => Ok(WorkerState::Shutdown),
      Err(_error) => Ok(WorkerState::Disconnected),
    }
  }

  /// Handles non-blocking receives that drain the rest of a batch.
  fn handle_try_recv(&mut self, result: Result<Msg, TryRecvError>) -> io::Result<WorkerState> {
    match result {
      Ok(Msg::Line(msg)) => {
        self.writer.write_all(&msg)?;
        Ok(WorkerState::Continue)
      }
      Ok(Msg::Shutdown) => Ok(WorkerState::Shutdown),
      Err(TryRecvError::Empty) => Ok(WorkerState::Empty),
      Err(TryRecvError::Disconnected) => Ok(WorkerState::Disconnected),
    }
  }

  /// Blocks on the first recv of each batch of logs, unless the
  /// channel is disconnected. Afterwards, grabs as many logs as
  /// it can off the channel, buffers them and attempts a flush.
  fn work(&mut self) -> io::Result<WorkerState> {
    // Worker thread yields here if receive buffer is empty
    let mut worker_state = self.handle_recv(self.receiver.recv())?;

    while worker_state == WorkerState::Continue {
      worker_state = self.handle_try_recv(self.receiver.try_recv())?;
    }
    self.writer.flush()?;
    Ok(worker_state)
  }

  /// Creates a worker thread that processes a channel until it's disconnected
  ///
  /// # Errors
  ///
  /// Returns the thread-spawn error if the operating system refuses to create
  /// the background worker thread.
  pub(super) fn worker_thread(mut self, name: String) -> io::Result<thread::JoinHandle<()>> {
    thread::Builder::new().name(name).spawn(move || {
      loop {
        match self.work() {
          Ok(WorkerState::Continue | WorkerState::Empty) => {}
          Ok(WorkerState::Shutdown | WorkerState::Disconnected) => {
            let _shutdown_ack: Result<(), RecvError> = self.shutdown.recv();
            break;
          }
          Err(_error) => {}
        }
      }
      let _flush_result = self.writer.flush();
    })
  }
}
