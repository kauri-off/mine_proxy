use std::io;

use crate::{
    packet::{packet::Packet, packet_builder::PacketBuilder, packet_reader::PacketReader},
    types::var_int::VarInt,
};

pub trait PacketT: Sized {
    fn serialize(self) -> Packet;
    async fn deserialize(packet: &Packet) -> io::Result<Self>;
}

#[derive(Debug)]
pub struct Handshake {
    pub packet_id: VarInt,
    pub protocol_version: VarInt,
    pub server_address: String,
    pub server_port: u16,
    pub next_state: VarInt,
}

impl PacketT for Handshake {
    fn serialize(self) -> Packet {
        PacketBuilder::new(self.packet_id)
            .var_int(self.protocol_version)
            .string(self.server_address)
            .int(self.server_port)
            .var_int(self.next_state)
            .build()
    }

    async fn deserialize(packet: &Packet) -> io::Result<Self> {
        let mut pr = PacketReader::new(packet);
        let protocol_version = pr.read_var_int().await?;
        let server_address = pr.read_string().await?;
        let server_port: u16 = pr.read_int().await?;
        let next_state = pr.read_var_int().await?;

        Ok(Handshake {
            packet_id: packet.packet_id.clone(),
            protocol_version,
            server_address,
            server_port,
            next_state,
        })
    }
}

pub struct SetCompression {
    pub packet_id: VarInt,
    pub threshold: VarInt,
}

impl PacketT for SetCompression {
    fn serialize(self) -> Packet {
        PacketBuilder::new(self.packet_id)
            .var_int(self.threshold)
            .build()
    }

    async fn deserialize(packet: &Packet) -> io::Result<Self> {
        let mut packet_reader = PacketReader::new(packet);

        Ok(SetCompression {
            packet_id: packet.packet_id.clone(),
            threshold: packet_reader.read_var_int().await?,
        })
    }
}
