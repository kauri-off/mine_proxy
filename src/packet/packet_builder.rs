use crate::types::{num::Integer, var_int::VarInt};

use super::packet::Packet;

pub struct PacketBuilder {
    pub packet_id: VarInt,
    pub data: Vec<u8>,
}

impl PacketBuilder {
    pub fn new(packet_id: VarInt) -> PacketBuilder {
        PacketBuilder {
            packet_id,
            data: vec![],
        }
    }

    pub fn build(self) -> Packet {
        Packet {
            packet_id: self.packet_id,
            data: self.data,
        }
    }

    pub fn var_int(mut self, var_int: VarInt) -> Self {
        let _ = var_int.write_sync(&mut self.data);
        self
    }

    pub fn string(mut self, string: String) -> Self {
        self = self.var_int(VarInt(string.len() as i32));

        self.data.extend(string.as_bytes());
        self
    }

    pub fn int<I: Integer>(mut self, int: I) -> Self {
        self.data.extend(int.to_bytes());
        self
    }
}
