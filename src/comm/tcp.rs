use crate::Error;

use super::{Client, Communicator, Protocol};
use core::fmt;
use parking_lot::{Mutex, MutexGuard};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

pub fn connect<A: ToSocketAddrs + fmt::Debug>(addr: A, timeout: Duration) -> Result<Client, Error> {
    Ok(Client(Tcp::create(addr, timeout)?))
}

#[allow(clippy::module_name_repetitions)]
pub struct Tcp {
    addr: SocketAddr,
    stream: Mutex<Option<TcpStream>>,
    timeout: Duration,
    busy: Mutex<()>,
}

#[allow(clippy::module_name_repetitions)]
pub type TcpClient = Arc<Tcp>;

macro_rules! handle_tcp_stream_error {
    ($stream: expr, $err: expr, $any: expr) => {{
        if $any || $err.kind() == std::io::ErrorKind::TimedOut {
            $stream.take();
        }
        $err
    }};
}

impl Communicator for Tcp {
    fn lock(&self) -> MutexGuard<()> {
        self.busy.lock()
    }
    fn reconnect(&self) {
        self.stream.lock().take();
    }
    fn write(&self, buf: &[u8]) -> Result<(), std::io::Error> {
        let mut stream = self.get_stream()?;
        stream
            .as_mut()
            .unwrap()
            .write_all(buf)
            .map_err(|e| handle_tcp_stream_error!(stream, e, true))
    }
    fn read_exact(&self, buf: &mut [u8]) -> Result<(), std::io::Error> {
        let mut stream = self.get_stream()?;
        stream
            .as_mut()
            .unwrap()
            .read_exact(buf)
            .map_err(|e| handle_tcp_stream_error!(stream, e, false))
    }
    fn protocol(&self) -> Protocol {
        Protocol::Tcp
    }
}

impl Tcp {
    fn create<A: ToSocketAddrs + fmt::Debug>(
        addr: A,
        timeout: Duration,
    ) -> Result<TcpClient, Error> {
        Ok(Self {
            addr: addr
                .to_socket_addrs()?
                .next()
                .ok_or_else(|| Error::invalid_data(format!("Invalid address: {:?}", addr)))?,
            stream: <_>::default(),
            busy: <_>::default(),
            timeout,
        }
        .into())
    }
    fn get_stream(&self) -> Result<MutexGuard<Option<TcpStream>>, std::io::Error> {
        let mut lock = self.stream.lock();
        if lock.as_mut().is_none() {
            let stream = TcpStream::connect_timeout(&self.addr, self.timeout)?;
            stream.set_read_timeout(Some(self.timeout))?;
            stream.set_write_timeout(Some(self.timeout))?;
            stream.set_nodelay(true)?;
            lock.replace(stream);
        }
        Ok(lock)
    }
}
