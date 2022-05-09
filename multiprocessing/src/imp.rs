pub use ctor::ctor;

use crate::{FnOnce, Receiver};
use lazy_static::lazy_static;
use nix::fcntl;
use std::os::unix::io::{FromRawFd, RawFd};
use std::sync::RwLock;

lazy_static! {
    pub static ref MAIN_ENTRY: RwLock<Option<fn() -> i32>> = RwLock::new(None);
}

pub trait Report {
    fn report(self) -> i32;
}

impl Report for () {
    fn report(self) -> i32 {
        0
    }
}

impl<T, E: std::fmt::Debug> Report for Result<T, E> {
    fn report(self) -> i32 {
        match self {
            Ok(_) => 0,
            Err(e) => {
                eprintln!("Error: {e:?}");
                1
            }
        }
    }
}

pub fn main() {
    let mut args = std::env::args();
    if let Some(s) = args.next() {
        if s == "_multiprocessing_" {
            let entry_rx_fd: RawFd = args
                .next()
                .expect("Expected three CLI arguments for multiprocessing")
                .parse()
                .expect("Expected the first CLI argument for multiprocessing to be an integer");

            let input_rx_fd: RawFd = args
                .next()
                .expect("Expected three CLI arguments for multiprocessing")
                .parse()
                .expect("Expected the second CLI argument for multiprocessing to be an integer");

            let output_tx_fd: RawFd = args
                .next()
                .expect("Expected three CLI arguments for multiprocessing")
                .parse()
                .expect("Expected the third CLI argument for multiprocessing to be an integer");

            enable_cloexec(entry_rx_fd).expect("Failed to set O_CLOEXEC for entry_rx_fd");
            enable_cloexec(input_rx_fd).expect("Failed to set O_CLOEXEC for input_rx_fd");
            enable_cloexec(output_tx_fd).expect("Failed to set O_CLOEXEC for output_tx_fd");

            let mut entry_rx = unsafe {
                Receiver::<Box<dyn FnOnce<(RawFd, RawFd), Output = i32>>>::from_raw_fd(entry_rx_fd)
            };

            let entry = entry_rx
                .recv()
                .expect("Failed to read entry for multiprocessing")
                .expect("No entry passed");

            std::process::exit(entry(input_rx_fd, output_tx_fd));
        }
    }
    std::process::exit(MAIN_ENTRY
        .read()
        .expect("Failed to acquire read access to MAIN_ENTRY")
        .expect(
            "MAIN_ENTRY was not registered: is #[multiprocessing::main] missing?",
        )());
}

pub fn disable_cloexec(fd: RawFd) -> std::io::Result<()> {
    fcntl::fcntl(fd, fcntl::FcntlArg::F_SETFD(fcntl::FdFlag::empty()))?;
    Ok(())
}

pub fn enable_cloexec(fd: RawFd) -> std::io::Result<()> {
    fcntl::fcntl(fd, fcntl::FcntlArg::F_SETFD(fcntl::FdFlag::FD_CLOEXEC))?;
    Ok(())
}

pub fn disable_nonblock(fd: RawFd) -> std::io::Result<()> {
    fcntl::fcntl(fd, fcntl::FcntlArg::F_SETFL(fcntl::OFlag::empty()))?;
    Ok(())
}

pub fn enable_nonblock(fd: RawFd) -> std::io::Result<()> {
    fcntl::fcntl(fd, fcntl::FcntlArg::F_SETFL(fcntl::OFlag::O_NONBLOCK))?;
    Ok(())
}
