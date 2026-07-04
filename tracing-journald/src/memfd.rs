//! memfd helpers.

use std::fs::File;
use std::io::Result;

use rustix::fd::AsFd;
use rustix::fs::MemfdFlags;
use rustix::fs::SealFlags;
use rustix::fs::fcntl_add_seals;
use rustix::fs::memfd_create;

/// Create a memfd that can later be sealed before handing it to journald.
#[allow(
  clippy::single_call_fn,
  reason = "memfd creation remains a named Linux payload boundary for oversized journald messages"
)]
pub(super) fn create_sealable() -> Result<File> {
  let fd = memfd_create(c"tracing-journald", MemfdFlags::ALLOW_SEALING | MemfdFlags::CLOEXEC)?;

  Ok(File::from(fd))
}

/// Add the complete set of seals journald expects for an immutable payload fd.
#[allow(
  clippy::single_call_fn,
  reason = "journald memfd sealing policy remains isolated from oversized payload sending"
)]
pub(super) fn seal_fully(fd: impl AsFd) -> Result<()> {
  fcntl_add_seals(fd, SealFlags::SHRINK | SealFlags::GROW | SealFlags::WRITE | SealFlags::SEAL)?;

  Ok(())
}
