use std::io::{self, Error};

use tokio::net::{
    tcp::{OwnedReadHalf, OwnedWriteHalf},
    TcpStream,
};

use crate::{
    packet::packet::{Packet, UncompressedPacket},
    packets::packets::{ChatCommand, Handshake, PacketActions, SetCompression, Status},
};

pub async fn create_proxy(
    mut local: TcpStream,
    mut remote: TcpStream,
    server_dns: Option<String>,
    server_port: Option<i32>,
) -> io::Result<()> {
    // C→S: Handshake with Next State set to 2 (login)
    let handshake = Packet::read_uncompressed(&mut local).await?;

    let mut packet = Handshake::deserialize(&handshake).await?;
    packet.server_address = match server_dns {
        Some(a) => a,
        None => packet.server_address,
    };
    packet.server_port = match server_port {
        Some(p) => p as u16,
        None => packet.server_port,
    };

    if packet.next_state.0 != 0x02 {
        Packet::UnCompressed(packet.serialize())
            .write(&mut remote, None)
            .await?;
        return start_proxy(local, remote, State::Status, None).await;
    }

    Packet::UnCompressed(packet.serialize())
        .write(&mut remote, None)
        .await?;

    // C→S: Login Start
    let login_start = Packet::read_uncompressed(&mut local).await?;
    login_start.write(&mut remote).await?;

    // S→C: Unknown
    let unknown = Packet::read_uncompressed(&mut remote).await?;
    unknown.write(&mut local).await?;

    match unknown.packet_id.0 {
        0x00 => Err(Error::new(io::ErrorKind::Other, "Disconnect")),
        0x01 => Err(Error::new(io::ErrorKind::Other, "Encryption request")),
        0x02 => start_proxy(local, remote, State::Transit, None).await,
        0x03 => {
            let compression = SetCompression::deserialize(&unknown).await?;

            let login_success = Packet::read(&mut remote, Some(compression.threshold.0)).await?;
            if login_success.packet_id().await?.0 != 0x02 {
                return Err(Error::new(io::ErrorKind::Other, "Packet unknown"));
            }
            login_success
                .write(&mut local, Some(compression.threshold.0))
                .await?;
            start_proxy(local, remote, State::Transit, Some(compression.threshold.0)).await
        }
        _ => Err(Error::new(io::ErrorKind::Other, "Packet unknown")),
    }
}

async fn start_proxy(
    local: TcpStream,
    remote: TcpStream,
    state: State,
    compression: Option<i32>,
) -> io::Result<()> {
    let (local_reader, local_writer) = local.into_split();
    let (remote_reader, remote_writer) = remote.into_split();

    let server_bound_proxy = Proxy::new(
        local_reader,
        remote_writer,
        Bound::Server,
        state.clone(),
        compression,
    );
    let client_bound_proxy = Proxy::new(
        remote_reader,
        local_writer,
        Bound::Client,
        state.clone(),
        compression,
    );

    let server_task = tokio::task::spawn(server_bound_proxy.run());
    let client_task = tokio::task::spawn(client_bound_proxy.run());

    server_task.await??;
    client_task.await??;

    Ok(())
}

#[allow(dead_code)]
#[derive(Clone, PartialEq, Eq)]
enum State {
    Handshake,
    Status,
    Login,
    Transit,
}

#[derive(Debug)]
enum Bound {
    Server,
    Client,
}

struct Proxy {
    input: OwnedReadHalf,
    output: OwnedWriteHalf,
    bound: Bound,
    state: State,
    count: usize,
    threshold: Option<i32>,
}

impl Proxy {
    pub fn new(
        input: OwnedReadHalf,
        remote: OwnedWriteHalf,
        bound: Bound,
        state: State,
        threshold: Option<i32>,
    ) -> Self {
        Proxy {
            input,
            output: remote,
            bound,
            state,
            count: 0,
            threshold,
        }
    }
    pub async fn run(mut self) -> io::Result<()> {
        loop {
            let packet = Packet::read(&mut self.input, self.threshold).await?;
            self.count += 1;

            let packet = self.handle_packet(packet).await;
            packet.write(&mut self.output, self.threshold).await?;
        }
    }

    pub async fn handle_packet(&mut self, packet: Packet) -> Packet {
        // println!("#{} {:?} => {:?}", self.count, &self.bound, &packet);

        if let Packet::UnCompressed(packet) = packet {
            let packet = match self.bound {
                Bound::Server => self.handle_server_bound(packet).await,
                Bound::Client => self.handle_cliend_bound(packet).await,
            };
            Packet::UnCompressed(packet)
        } else {
            packet
        }
    }

    pub async fn handle_server_bound(&mut self, packet: UncompressedPacket) -> UncompressedPacket {
        // println!("#{} {:?} => {:?}", self.count, &self.bound, &packet);
        match packet.packet_id.0 {
            p if p == 0x04 && self.state == State::Transit => {
                self.sb_0x04_chat_command(packet).await
            }
            _ => packet,
        }
    }

    pub async fn handle_cliend_bound(&mut self, packet: UncompressedPacket) -> UncompressedPacket {
        match packet.packet_id.0 {
            p if p == 0x00 && self.state == State::Status => self.cb_0x00_status(packet).await,
            _ => packet,
        }
    }

    pub async fn cb_0x00_status(&mut self, packet: UncompressedPacket) -> UncompressedPacket {
        if let Ok(mut status) = Status::deserialize(&packet).await {
            println!("Status: {}", status.status);
            status.status = "{\"version\":{\"name\":\"Paper 1.20.4\",\"protocol\":765},\"description\":\"Hello from mine_proxy\",\"players\":{\"max\":1000,\"online\":999}}".to_string();
            status.serialize()
        } else {
            packet
        }
    }

    pub async fn sb_0x04_chat_command(&mut self, packet: UncompressedPacket) -> UncompressedPacket {
        if let Ok(chat_message) = ChatCommand::deserialize(&packet).await {
            println!("Player send command: {}", chat_message.command);
        }
        packet
    }
}
