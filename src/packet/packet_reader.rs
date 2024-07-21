use std::io::{self, Error, ErrorKind};

use crate::types::{num::Integer, var_int::VarInt};

use super::packet::Packet;

pub struct PacketReader {
    stream: Vec<u8>,
    cursor: usize,
}

impl PacketReader {
    pub fn new(packet: &Packet) -> Self {
        let stream = packet.data.clone();
        PacketReader { stream, cursor: 0 }
    }

    pub async fn read_var_int(&mut self) -> io::Result<VarInt> {
        let mut stream = &self.stream[self.cursor..];
        let (var_int, offset) = VarInt::read(&mut stream).await?;
        self.cursor += offset;

        Ok(var_int)
    }

    pub async fn read_string(&mut self) -> io::Result<String> {
        let len = self.read_var_int().await?.0 as usize;
        let stream = self.stream[self.cursor..self.cursor + len].to_vec();
        self.cursor += len;

        String::from_utf8(stream).map_err(|e| Error::new(ErrorKind::InvalidData, e))
    }

    pub async fn read_int<T: Integer>(&mut self) -> io::Result<T> {
        let buf = &self.stream[self.cursor..self.cursor + T::byte_len()];
        self.cursor += T::byte_len();

        Ok(T::from_bytes(buf))
    }
}
