//! socket helpers.

use rustix::{
    cmsg_space,
    fd::BorrowedFd,
    io::Errno,
    net::{
        MMsgHdr, SendAncillaryBuffer, SendAncillaryMessage, SendFlags, SocketAddrAny,
        SocketAddrUnix, sendmmsg,
    },
};
use std::{
    io::{self, IoSlice},
    mem::MaybeUninit,
    os::unix::{ffi::OsStrExt, net::UnixDatagram},
    path::Path,
};

const LINUX_SUN_PATH_BYTES: usize = 108;

fn socket_addr(path: &Path) -> io::Result<SocketAddrAny> {
    if path.as_os_str().as_bytes().len() >= LINUX_SUN_PATH_BYTES {
        return Err(io::Error::from(Errno::NAMETOOLONG));
    }

    SocketAddrUnix::new(path)
        .map(SocketAddrAny::from)
        .map_err(io::Error::from)
}

pub(crate) fn send_one_fd_to<P: AsRef<Path>>(
    socket: &UnixDatagram,
    fd: BorrowedFd<'_>,
    path: P,
) -> io::Result<usize> {
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
    use super::*;
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};

    #[test]
    fn socket_path_at_linux_sun_path_capacity_is_rejected() {
        let path = OsString::from_vec(vec![b'a'; LINUX_SUN_PATH_BYTES]);
        let err = socket_addr(Path::new(&path)).expect_err("path should be rejected");
        assert_eq!(err.raw_os_error(), Some(Errno::NAMETOOLONG.raw_os_error()));
    }

    #[test]
    fn socket_path_below_linux_sun_path_capacity_is_accepted_by_address_builder() {
        let path = OsString::from_vec(vec![b'a'; LINUX_SUN_PATH_BYTES - 1]);
        let _addr = socket_addr(Path::new(&path)).expect("path should fit in linux sun_path");
    }
}
