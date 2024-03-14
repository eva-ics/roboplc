use binrw::{BinRead, BinWrite};
use std::{
    io::Cursor,
    marker::PhantomData,
    net::{SocketAddr, ToSocketAddrs, UdpSocket},
};

use crate::{Error, Result};

pub struct UdpInput<T>
where
    T: for<'a> BinRead<Args<'a> = ()>,
{
    server: UdpSocket,
    buffer: Vec<u8>,
    _phantom: PhantomData<T>,
}

impl<T> UdpInput<T>
where
    T: for<'a> BinRead<Args<'a> = ()>,
{
    pub fn bind<A: ToSocketAddrs>(addr: A, buf_size: usize) -> Result<Self> {
        let server = UdpSocket::bind(addr)?;
        Ok(Self {
            server,
            buffer: vec![0; buf_size],
            _phantom: PhantomData,
        })
    }
}

impl<T> Iterator for UdpInput<T>
where
    T: for<'a> BinRead<Args<'a> = ()>,
{
    type Item = Result<T>;

    fn next(&mut self) -> Option<Self::Item> {
        match self.server.recv(&mut self.buffer) {
            Ok(size) => {
                let mut cursor = Cursor::new(&self.buffer[..size]);
                Some(T::read_le(&mut cursor).map_err(Into::into))
            }
            Err(e) => Some(Err(e.into())),
        }
    }
}

pub struct UdpOutput {
    socket: UdpSocket,
    target: SocketAddr,
    data_buf: Vec<u8>,
}

impl UdpOutput {
    pub fn connect<A: ToSocketAddrs>(addr: A) -> Result<Self> {
        let socket = UdpSocket::bind(("0.0.0.0", 0))?;
        let target = addr
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| Error::InvalidData("no target address provided".to_string()))?;
        Ok(Self {
            socket,
            target,
            data_buf: <_>::default(),
        })
    }

    pub fn send<T>(&mut self, value: T) -> Result<()>
    where
        T: for<'a> BinWrite<Args<'a> = ()>,
    {
        let mut buf = Cursor::new(&mut self.data_buf);
        value.write_le(&mut buf)?;
        self.socket.send_to(&self.data_buf, self.target)?;
        Ok(())
    }
}
