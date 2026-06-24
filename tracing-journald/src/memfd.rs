//! memfd helpers.

use rustix::{
    fd::AsFd,
    fs::{MemfdFlags, SealFlags, fcntl_add_seals, memfd_create},
};
use std::fs::File;
use std::io::Result;

pub(crate) fn create_sealable() -> Result<File> {
    let fd = memfd_create(
        c"tracing-journald",
        MemfdFlags::ALLOW_SEALING | MemfdFlags::CLOEXEC,
    )?;

    Ok(File::from(fd))
}

pub(crate) fn seal_fully(fd: impl AsFd) -> Result<()> {
    fcntl_add_seals(
        fd,
        SealFlags::SHRINK | SealFlags::GROW | SealFlags::WRITE | SealFlags::SEAL,
    )?;

    Ok(())
}
