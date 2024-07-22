use std::io::{self, Cursor, Error, ErrorKind, Read};

use crate::types::{num::Integer, var_int::VarInt};

use super::packet::Packet;

pub struct PacketReader {
    stream: Cursor<Vec<u8>>,
}

impl PacketReader {
    pub fn new(packet: &Packet) -> Self {
        let stream = Cursor::new(packet.data.clone());
        PacketReader { stream }
    }

    pub async fn read_var_int(&mut self) -> io::Result<VarInt> {
        let var_int = VarInt::read(&mut self.stream).await?;

        Ok(var_int)
    }

    pub async fn read_string(&mut self) -> io::Result<String> {
        let len = self.read_var_int().await?.0 as usize;
        let mut stream = vec![0; len];
        self.stream.read_exact(&mut stream)?;

        String::from_utf8(stream).map_err(|e| Error::new(ErrorKind::InvalidData, e))
    }

    pub async fn read_int<T: Integer>(&mut self) -> io::Result<T> {
        let mut buf = vec![0; T::byte_len()];
        self.stream.read_exact(&mut buf)?;

        Ok(T::from_bytes(&buf))
    }
}
