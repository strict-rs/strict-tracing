//! socket helpers.

use std::io;
use std::io::IoSlice;
use std::mem::MaybeUninit;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::net::UnixDatagram;
use std::path::Path;

use rustix::cmsg_space;
use rustix::fd::BorrowedFd;
use rustix::io::Errno;
use rustix::net::MMsgHdr;
use rustix::net::SendAncillaryBuffer;
use rustix::net::SendAncillaryMessage;
use rustix::net::SendFlags;
use rustix::net::SocketAddrAny;
use rustix::net::SocketAddrUnix;
use rustix::net::sendmmsg;

/// Maximum byte length accepted by Linux `sockaddr_un.sun_path`.
const LINUX_SUN_PATH_BYTES: usize = 108;

/// Build a Rustix socket address after checking Linux path capacity.
#[allow(
  clippy::single_call_fn,
  reason = "socket path validation stays isolated before Rustix address construction"
)]
fn socket_addr(path: &Path) -> io::Result<SocketAddrAny> {
  if path.as_os_str().as_bytes().len() >= LINUX_SUN_PATH_BYTES {
    return Err(io::Error::from(Errno::NAMETOOLONG));
  }

  SocketAddrUnix::new(path).map(SocketAddrAny::from).map_err(io::Error::from)
}

/// Send one file descriptor to `path` with an `SCM_RIGHTS` control message.
#[allow(
  clippy::single_call_fn,
  reason = "SCM_RIGHTS descriptor transfer remains a named journald socket boundary"
)]
pub(super) fn send_one_fd_to<P: AsRef<Path>>(socket: &UnixDatagram, fd: BorrowedFd<'_>, path: P) -> io::Result<usize> {
  let addr = socket_addr(path.as_ref())?;
  let fds = [fd];
  let mut space = [MaybeUninit::uninit(); cmsg_space!(ScmRights(1))];
  let mut control = SendAncillaryBuffer::new(&mut space);

  if !control.push(SendAncillaryMessage::ScmRights(&fds)) {
    return Err(io::Error::from(Errno::NOBUFS));
  }

  let iov: [IoSlice<'_>; 0] = [];
  let mut messages = [MMsgHdr::new_with_addr(&addr, &iov, &mut control)];
  let sent = sendmmsg(socket, &mut messages, SendFlags::NOSIGNAL)?;

  if sent == 0 {
    return Ok(0);
  }

  Ok(messages[0].bytes_sent())
}

#[cfg(test)]
mod tests {
  use std::ffi::OsString;
  use std::os::unix::ffi::OsStringExt as _;

  use strict_test_support::TestFailure;
  use strict_test_support::ensure;
  use strict_test_support::ensure_ok;
  use strict_test_support::ensure_some;

  use super::*;

  #[test]
  fn socket_path_at_linux_sun_path_capacity_is_rejected() -> Result<(), TestFailure> {
    let path = OsString::from_vec(vec![b'a'; LINUX_SUN_PATH_BYTES]);
    let err = ensure_some(
      socket_addr(Path::new(&path)).err(),
      "path at Linux sun_path capacity should be rejected",
    )?;
    ensure(
      err.raw_os_error() == Some(Errno::NAMETOOLONG.raw_os_error()),
      "oversized socket path error",
    )
  }

  #[test]
  fn socket_path_below_linux_sun_path_capacity_is_accepted_by_address_builder() -> Result<(), TestFailure> {
    let path_len = LINUX_SUN_PATH_BYTES.saturating_sub(1);
    let path = OsString::from_vec(vec![b'a'; path_len]);
    let _addr = ensure_ok(socket_addr(Path::new(&path)), "path below Linux sun_path capacity should fit")?;
    Ok(())
  }
}
