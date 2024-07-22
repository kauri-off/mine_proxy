use std::io;

use crate::{
    packet::{
        packet::UncompressedPacket, packet_builder::PacketBuilder, packet_reader::PacketReader,
    },
    types::var_int::VarInt,
};

pub trait PacketActions: Sized {
    fn serialize(self) -> UncompressedPacket;
    async fn deserialize(packet: &UncompressedPacket) -> io::Result<Self>;
}

/// PacketID 0x00
#[derive(Debug)]
pub struct Handshake {
    pub packet_id: VarInt,
    pub protocol_version: VarInt,
    pub server_address: String,
    pub server_port: u16,
    pub next_state: VarInt,
}

impl PacketActions for Handshake {
    fn serialize(self) -> UncompressedPacket {
        PacketBuilder::new(self.packet_id)
            .write_var_int(self.protocol_version)
            .write_string(self.server_address)
            .write_int(self.server_port)
            .write_var_int(self.next_state)
            .build()
    }

    async fn deserialize(packet: &UncompressedPacket) -> io::Result<Self> {
        let mut pr = PacketReader::new(packet);
        let protocol_version = pr.read_var_int().await?;
        let server_address = pr.read_string().await?;
        let server_port: u16 = pr.read_int()?;
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

/// PacketID 0x03
pub struct SetCompression {
    pub packet_id: VarInt,
    pub threshold: VarInt,
}

impl PacketActions for SetCompression {
    fn serialize(self) -> UncompressedPacket {
        PacketBuilder::new(self.packet_id)
            .write_var_int(self.threshold)
            .build()
    }

    async fn deserialize(packet: &UncompressedPacket) -> io::Result<Self> {
        let mut packet_reader = PacketReader::new(packet);

        Ok(SetCompression {
            packet_id: packet.packet_id.clone(),
            threshold: packet_reader.read_var_int().await?,
        })
    }
}

/// PacketID 0x04
pub struct ChatCommand {
    pub packet_id: VarInt,
    pub command: String,
}

impl PacketActions for ChatCommand {
    fn serialize(self) -> UncompressedPacket {
        PacketBuilder::new(self.packet_id)
            .write_string(self.command)
            .build()
    }

    async fn deserialize(packet: &UncompressedPacket) -> io::Result<Self> {
        let mut packet_reader = PacketReader::new(&packet);
        let packet_id = packet.packet_id.clone();

        let command = packet_reader.read_string().await?;

        Ok(ChatCommand { packet_id, command })
    }
}

/// PacketID 0x06
pub struct ChatMessage {
    pub packet_id: VarInt,
    pub message: String,
    pub timestamp: i64,
    pub salt: i64,
    pub has_signature: bool,
    pub signature: Option<[u8; 256]>,
    pub message_count: VarInt,
    pub acknowledged: [u8; 20],
}

impl PacketActions for ChatMessage {
    fn serialize(self) -> UncompressedPacket {
        let packet = PacketBuilder::new(self.packet_id)
            .write_string(self.message)
            .write_int(self.timestamp)
            .write_int(self.salt)
            .write_bool(self.has_signature);

        let packet = match self.signature {
            Some(s) => packet.write_buffer(&s),
            None => packet,
        };

        packet
            .write_var_int(self.message_count)
            .write_buffer(&self.acknowledged)
            .build()
    }

    async fn deserialize(packet: &UncompressedPacket) -> io::Result<Self> {
        let mut packet_reader = PacketReader::new(packet);
        let packet_id = packet.packet_id.clone();

        let message = packet_reader.read_string().await?;
        let timestamp: i64 = packet_reader.read_int()?;
        let salt: i64 = packet_reader.read_int()?;
        let has_signature = packet_reader.read_bool()?;
        let signature = match has_signature {
            true => {
                let mut buf = [0u8; 256];
                packet_reader.read_exact(&mut buf)?;
                Some(buf)
            }
            false => None,
        };
        let message_count = packet_reader.read_var_int().await?;
        let acknowledged = {
            let mut buf = [0u8; 20];
            packet_reader.read_exact(&mut buf)?;
            buf
        };
        Ok(ChatMessage {
            packet_id,
            message,
            timestamp,
            salt,
            has_signature,
            signature,
            message_count,
            acknowledged,
        })
    }
}

/// PacketID 0x00
pub struct Status {
    pub packet_id: VarInt,
    pub status: String,
}

impl PacketActions for Status {
    fn serialize(self) -> UncompressedPacket {
        PacketBuilder::new(self.packet_id.clone())
            .write_string(self.status)
            .build()
    }

    async fn deserialize(packet: &UncompressedPacket) -> io::Result<Self> {
        let mut packet_reader = PacketReader::new(packet);
        let packet_id = packet.packet_id.clone();

        let status = packet_reader.read_string().await?;

        Ok(Status { packet_id, status })
    }
}
