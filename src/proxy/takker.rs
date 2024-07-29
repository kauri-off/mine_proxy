use tokio::{
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpSocket, TcpStream,
    },
    sync::{
        mpsc::{self, Receiver, Sender},
        Mutex, Notify,
    },
};

use crate::packets::packets::{Handshake, LoginStart, PacketActions, SetCompression};
use std::{
    io::{self, Error},
    ops::Deref,
    sync::Arc,
};

use crate::{packet::packet::Packet, packets::packets::Status, types::var_int::VarInt};

pub async fn start_takker() -> io::Result<()> {
    let proxy_config = ProxyConfig {
        remote_ip: IP {
            ip: "mc.funtime.su".to_string(),
            port: 25565,
        },
        cheat_ip: IP {
            ip: "127.0.0.1".to_string(),
            port: 25566,
        },
        legit_ip: IP {
            ip: "127.0.0.1".to_string(),
            port: 25567,
        },
        status: ProxyServerStatus {
            name: "Takker proxy".to_string(),
            protocol: "765".to_string(),
            description: "My implementation of takker".to_string(),
        },
    };

    let (ctx, crx) = mpsc::channel(32); // CheatTX, CheatRX
    let (ltx, lrx) = mpsc::channel(32); // LegitTX, LegitRX
    tokio::spawn(wait_for_sockets(crx, lrx, proxy_config.clone()));

    let cheat_reader = tokio::spawn(read_port(
        ctx,
        proxy_config.cheat_ip.clone(),
        proxy_config.status.clone(),
    ));
    let legit_reader = tokio::spawn(read_port(
        ltx,
        proxy_config.legit_ip.clone(),
        proxy_config.status.clone(),
    ));

    let _ = tokio::join!(cheat_reader, legit_reader);

    Ok(())
}

async fn read_port(tx: Sender<TcpStream>, ip: IP, status: ProxyServerStatus) -> io::Result<()> {
    let tcp_socket = TcpSocket::new_v4()?;
    tcp_socket.set_reuseaddr(true)?;
    tcp_socket.bind(ip.pack().parse().unwrap())?;

    let listener = tcp_socket.listen(32)?;

    loop {
        if let Ok((socket, _addr)) = listener.accept().await {
            println!("New connection from {}", _addr);
            tokio::spawn(process_socket(socket, tx.clone(), status.clone()));
        }
    }
}

async fn process_socket(
    mut socket: TcpStream,
    tx: Sender<TcpStream>,
    proxy_status: ProxyServerStatus,
) -> io::Result<()> {
    let packet = Packet::read_uncompressed(&mut socket).await?;
    let handshake = Handshake::deserialize(&packet).await?;

    match handshake.next_state.0 {
        0x01 => status(&mut socket, &proxy_status).await,
        0x02 => Ok(tx.send(socket).await.unwrap()),
        _ => Err(Error::new(io::ErrorKind::Other, "Error, next_state")),
    }
}

async fn wait_for_sockets(
    mut crx: Receiver<TcpStream>,
    mut lrx: Receiver<TcpStream>,
    proxy_config: ProxyConfig,
) {
    loop {
        if let Err(e) = recieve_streams(&mut crx, &mut lrx, &proxy_config).await {
            println!("Error, {:?}", e);
        }
    }
}

async fn recieve_streams(
    crx: &mut Receiver<TcpStream>,
    lrx: &mut Receiver<TcpStream>,
    proxy_config: &ProxyConfig,
) -> io::Result<()> {
    let mut cheat_stream = crx
        .recv()
        .await
        .ok_or(Error::new(io::ErrorKind::Other, "none"))?;
    let mut legit_stream = lrx
        .recv()
        .await
        .ok_or(Error::new(io::ErrorKind::Other, "none"))?;

    let socket1_packet = Packet::read_uncompressed(&mut cheat_stream).await?;
    let socket2_packet = Packet::read_uncompressed(&mut legit_stream).await?;

    let socket1_login_start = LoginStart::deserialize(&socket1_packet).await?;
    println!(
        "Login: {} | UUID: {:x}",
        socket1_login_start.name, socket1_login_start.uuid
    );

    let mut remote_stream = TcpStream::connect(proxy_config.remote_ip.pack()).await?;

    let handshake = Handshake {
        packet_id: VarInt(0),
        protocol_version: VarInt(proxy_config.status.protocol.parse().unwrap()),
        server_address: proxy_config.remote_ip.ip.clone(),
        server_port: proxy_config.remote_ip.port,
        next_state: VarInt(0x02),
    }
    .serialize();

    handshake.write(&mut remote_stream).await?; // C→S: Handshake with Next State set to 2 (login)
    socket1_login_start
        .serialize()
        .write(&mut remote_stream)
        .await?; // C→S: Login Start

    let packet = Packet::read_uncompressed(&mut remote_stream).await?;
    let (compression, login_success) = match packet.packet_id.0 {
        0x02 => (None, Packet::UnCompressed(packet)),
        0x03 => {
            let compression = SetCompression::deserialize(&packet).await?;

            let login_success =
                Packet::read(&mut remote_stream, Some(compression.threshold.0)).await?;
            if login_success.packet_id().await?.0 != 0x02 {
                return Err(Error::new(io::ErrorKind::Other, "Packet unknown"));
            }
            (Some(compression), login_success)
        }
        _ => return Err(Error::new(io::ErrorKind::Other, "Packet id error")),
    };

    if let Some(compression) = compression.clone() {
        let compression = compression.serialize(); // S→C: Set Compression (optional)
        compression.write(&mut cheat_stream).await?;
        compression.write(&mut legit_stream).await?;
    }

    let threshold = match compression {
        Some(t) => Some(t.threshold.0),
        None => None,
    };

    // S→C: Login Success
    login_success.write(&mut cheat_stream, threshold).await?;
    login_success.write(&mut legit_stream, threshold).await?;

    let (cheat_reader, cheat_writer) = cheat_stream.into_split();
    let (legit_reader, legit_writer) = legit_stream.into_split();
    let (remote_reader, remote_writer) = remote_stream.into_split();

    let (tx, rx) = mpsc::channel(32);

    let read_from = Arc::new(Mutex::new(ReadFrom::Cheat));
    let cheat2server = Client2Server::new(
        ReadFrom::Cheat,
        threshold,
        cheat_reader,
        tx.clone(),
        read_from.clone(),
    );
    let legit2server = Client2Server::new(
        ReadFrom::Legit,
        threshold,
        legit_reader,
        tx.clone(),
        read_from.clone(),
    );
    let server2client = Server2Client::new(
        remote_reader,
        cheat_writer,
        legit_writer,
        threshold,
        read_from.clone(),
    );

    let notify = Arc::new(Notify::new());

    let cheat2server = tokio::spawn(cheat2server.run());
    let legit2server = tokio::spawn(legit2server.run());

    let notify_clone = notify.clone();
    let server2client = tokio::spawn(async move {
        tokio::select! {
        result = server2client.run() => {
            if let Err(e) = result {
                println!("Error in server2client, {:?}", e);
                notify_clone.notify_waiters();
            }
        },
        _ = notify_clone.notified() => {}
        }
    });

    let notify_clone = notify.clone();
    let rx2server = tokio::spawn(async move {
        tokio::select! {
        result = rx2server(rx, remote_writer, threshold) => {
            if let Err(e) = result {
                println!("Error in rx2server, {:?}", e);
                notify_clone.notify_waiters();
            }
        },
        _ = notify_clone.notified() => {}
        }
    });

    let _ = tokio::join!(cheat2server, legit2server, server2client, rx2server);

    Ok(())
}

async fn rx2server(
    mut rx: Receiver<Packet>,
    mut remote_writer: OwnedWriteHalf,
    threshold: Option<i32>,
) -> io::Result<()> {
    while let Some(packet) = rx.recv().await {
        packet.write(&mut remote_writer, threshold).await?;
    }
    Err(Error::new(io::ErrorKind::Other, "RX to server error"))
}

struct Server2Client {
    remote_reader: OwnedReadHalf,
    cheat_writer: Option<OwnedWriteHalf>,
    legit_writer: Option<OwnedWriteHalf>,
    threshold: Option<i32>,
    read_from_state: Arc<Mutex<ReadFrom>>,
}

impl Server2Client {
    fn new(
        remote_reader: OwnedReadHalf,
        cheat_writer: OwnedWriteHalf,
        legit_writer: OwnedWriteHalf,
        threshold: Option<i32>,
        read_from_state: Arc<Mutex<ReadFrom>>,
    ) -> Self {
        Server2Client {
            remote_reader,
            cheat_writer: Some(cheat_writer),
            legit_writer: Some(legit_writer),
            threshold,
            read_from_state,
        }
    }

    async fn run(mut self) -> io::Result<()> {
        loop {
            if self.cheat_writer.is_none() && self.legit_writer.is_none() {
                return Err(Error::new(io::ErrorKind::Other, "all clients died"));
            }
            let packet = Packet::read(&mut self.remote_reader, self.threshold).await?;
            // println!("Server | {:?}", packet);

            if let Some(ref mut cheat_writer) = self.cheat_writer {
                if let Err(_) = packet.write(cheat_writer, self.threshold).await {
                    let mut read_from = self.read_from_state.lock().await;
                    *read_from = ReadFrom::Legit;
                    self.cheat_writer = None;
                }
            }

            if let Some(ref mut legit_writer) = self.legit_writer {
                if let Err(_) = packet.write(legit_writer, self.threshold).await {
                    self.legit_writer = None;
                }
            }
        }
    }
}

struct Client2Server {
    read_from: ReadFrom,
    threshold: Option<i32>,
    reader: OwnedReadHalf,
    tx: Sender<Packet>,
    read_from_state: Arc<Mutex<ReadFrom>>,
}

impl Client2Server {
    fn new(
        read_from: ReadFrom,
        threshold: Option<i32>,
        reader: OwnedReadHalf,
        tx: Sender<Packet>,
        read_from_state: Arc<Mutex<ReadFrom>>,
    ) -> Self {
        Client2Server {
            read_from,
            threshold,
            reader,
            tx,
            read_from_state,
        }
    }

    async fn run(mut self) -> io::Result<()> {
        loop {
            let packet = Packet::read(&mut self.reader, self.threshold).await?;
            // println!("{:?} | {:?}", self.read_from, packet);

            let read_from = self.read_from_state.lock().await;
            if read_from.deref() == &self.read_from {
                drop(read_from);
                self.tx
                    .send(packet)
                    .await
                    .map_err(|_| Error::new(io::ErrorKind::Other, "TX send error"))?;
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ReadFrom {
    Cheat,
    Legit,
}

#[derive(Clone)]
struct ProxyConfig {
    remote_ip: IP,
    cheat_ip: IP,
    legit_ip: IP,
    status: ProxyServerStatus,
}

#[derive(Clone)]
struct IP {
    ip: String,
    port: u16,
}

impl IP {
    fn pack(&self) -> String {
        format!("{}:{}", self.ip.clone(), self.port)
    }
}

#[derive(Clone)]
struct ProxyServerStatus {
    name: String,
    protocol: String,
    description: String,
}

impl ProxyServerStatus {
    fn serialize(&self) -> String {
        format!(
            "{}{}{}{}{}{}{}",
            "{\"version\":{\"name\":\"",
            self.name,
            "\",\"protocol\":",
            self.protocol,
            "},\"description\":\"",
            self.description,
            "\",\"players\":{\"max\":1000,\"online\":999}}"
        ) // FUCK this
    }
}

async fn status(socket: &mut TcpStream, status: &ProxyServerStatus) -> io::Result<()> {
    let _status_req = Packet::read_uncompressed(socket).await?;

    let response = Status {
        packet_id: VarInt(0x00),
        status: status.serialize(),
    };
    response.serialize().write(socket).await?;

    let ping_req = Packet::read_uncompressed(socket).await?;
    ping_req.write(socket).await
}
