use crate::{imp, Deserialize, Deserializer, Object, Serialize, Serializer};
use nix::libc::{AF_UNIX, SOCK_CLOEXEC, SOCK_SEQPACKET};
use std::io::{Error, ErrorKind, IoSlice, IoSliceMut, Result};
use std::marker::PhantomData;
use std::os::unix::{
    io::{AsRawFd, FromRawFd, OwnedFd, RawFd},
    net::{AncillaryData, SocketAncillary, UnixStream},
};

pub(crate) const MAX_PACKET_SIZE: usize = 16 * 1024;

#[derive(Object)]
pub struct Sender<T: Serialize> {
    fd: UnixStream,
    marker: PhantomData<fn(T) -> T>,
}

#[derive(Object)]
pub struct Receiver<T: Deserialize> {
    fd: UnixStream,
    marker: PhantomData<fn(T) -> T>,
}

#[derive(Object)]
pub struct Duplex<S: Serialize, R: Deserialize> {
    sender: Sender<S>,
    receiver: Receiver<R>,
}

pub fn channel<T: Serialize + Deserialize>() -> Result<(Sender<T>, Receiver<T>)> {
    // UnixStream creates a SOCK_STREAM by default, while we need SOCK_SEQPACKET
    unsafe {
        let mut fds = [0, 0];
        if nix::libc::socketpair(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC, 0, fds.as_mut_ptr()) == -1
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok((Sender::from_raw_fd(fds[0]), Receiver::from_raw_fd(fds[1])))
    }
}

pub fn duplex<A: Serialize + Deserialize, B: Serialize + Deserialize>(
) -> Result<(Duplex<A, B>, Duplex<B, A>)> {
    let (a_tx, a_rx) = channel::<A>()?;
    let (b_tx, b_rx) = channel::<B>()?;
    Ok((
        Duplex {
            sender: a_tx,
            receiver: b_rx,
        },
        Duplex {
            sender: b_tx,
            receiver: a_rx,
        },
    ))
}

impl<T: Serialize> Sender<T> {
    pub fn from_unix_stream(fd: UnixStream) -> Self {
        Sender {
            fd,
            marker: PhantomData,
        }
    }

    pub fn send(&mut self, value: &T) -> Result<()> {
        let mut s = Serializer::new();
        s.serialize(value);

        let fds = s.drain_fds();
        let serialized = s.into_vec();

        let mut ancillary_buffer = [0; 253];

        // Send the data and pass file descriptors
        let mut buffer_pos: usize = 0;
        let mut fds_pos: usize = 0;

        loop {
            let buffer_end = serialized.len().min(buffer_pos + MAX_PACKET_SIZE - 1);
            let fds_end = fds.len().min(fds_pos + 253);

            let is_last = buffer_end == serialized.len() && fds_end == fds.len();

            let mut ancillary = SocketAncillary::new(&mut ancillary_buffer);
            if !ancillary.add_fds(&fds[fds_pos..fds_end]) {
                return Err(Error::new(ErrorKind::Other, "Too many fds to pass"));
            }

            let n_written = self.fd.send_vectored_with_ancillary(
                &[
                    IoSlice::new(&[is_last as u8]),
                    IoSlice::new(&serialized[buffer_pos..buffer_end]),
                ],
                &mut ancillary,
            )?;
            buffer_pos += n_written - 1;
            fds_pos = fds_end;

            if is_last {
                break;
            }
        }

        Ok(())
    }
}

impl<T: Serialize> AsRawFd for Sender<T> {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

impl<T: Serialize> FromRawFd for Sender<T> {
    unsafe fn from_raw_fd(fd: RawFd) -> Self {
        imp::disable_nonblock(fd).expect("Failed to reset O_NONBLOCK");
        Self::from_unix_stream(UnixStream::from_raw_fd(fd))
    }
}

impl<T: Deserialize> Receiver<T> {
    pub fn from_unix_stream(fd: UnixStream) -> Self {
        Receiver {
            fd,
            marker: PhantomData,
        }
    }

    pub fn recv(&mut self) -> Result<Option<T>> {
        // Read the data and the passed file descriptors
        let mut serialized: Vec<u8> = Vec::new();
        let mut buffer_pos: usize = 0;

        let mut ancillary_buffer = [0; 253];
        let mut received_fds: Vec<OwnedFd> = Vec::new();

        loop {
            serialized.resize(buffer_pos + MAX_PACKET_SIZE - 1, 0);

            let mut marker = [0];
            let mut ancillary = SocketAncillary::new(&mut ancillary_buffer[..]);

            let n_read = self.fd.recv_vectored_with_ancillary(
                &mut [
                    IoSliceMut::new(&mut marker),
                    IoSliceMut::new(&mut serialized[buffer_pos..]),
                ],
                &mut ancillary,
            )?;

            for cmsg in ancillary.messages() {
                if let Ok(AncillaryData::ScmRights(rights)) = cmsg {
                    for fd in rights {
                        received_fds.push(unsafe { OwnedFd::from_raw_fd(fd) });
                    }
                } else {
                    return Err(Error::new(
                        ErrorKind::Other,
                        format!("Unexpected kind of cmsg on stream"),
                    ));
                }
            }

            if ancillary.is_empty() && n_read == 0 {
                if buffer_pos == 0 && received_fds.is_empty() {
                    return Ok(None);
                } else {
                    return Err(Error::new(
                        ErrorKind::Other,
                        format!("Unterminated data on stream"),
                    ));
                }
            }

            if n_read == 0 {
                return Err(Error::new(
                    ErrorKind::Other,
                    format!("Unexpected empty message on stream"),
                ));
            }

            buffer_pos += n_read - 1;
            if marker[0] == 1 {
                break;
            }
        }

        serialized.truncate(buffer_pos);

        let mut d = Deserializer::from(serialized, received_fds);
        Ok(Some(d.deserialize()))
    }
}

impl<T: Deserialize> AsRawFd for Receiver<T> {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

impl<T: Deserialize> FromRawFd for Receiver<T> {
    unsafe fn from_raw_fd(fd: RawFd) -> Self {
        imp::disable_nonblock(fd).expect("Failed to reset O_NONBLOCK");
        Self::from_unix_stream(UnixStream::from_raw_fd(fd))
    }
}

impl<S: Serialize, R: Deserialize> Duplex<S, R> {
    pub fn send(&mut self, value: &S) -> Result<()> {
        self.sender.send(value)
    }
    pub fn recv(&mut self) -> Result<Option<R>> {
        self.receiver.recv()
    }
}
