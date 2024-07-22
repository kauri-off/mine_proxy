use std::io::{self, Error};

use packet::{
    packet::{Packet, UncompressedPacket},
    packet_reader::PacketReader,
};
use packets::packets::{ChatCommand, Handshake, PacketActions, SetCompression};
use tokio::net::{
    tcp::{OwnedReadHalf, OwnedWriteHalf},
    TcpSocket, TcpStream,
};

mod packet;
mod packets;
mod types;

#[tokio::main]
async fn main() -> io::Result<()> {
    let local_addr = "127.0.0.1:25566".parse().unwrap();
    let remote_addr = "127.0.0.1:25565";

    let socket = TcpSocket::new_v4()?;
    socket.set_reuseaddr(true)?;
    socket.bind(local_addr)?;

    let listener = socket.listen(1024)?;
    println!("Waiting on {}", local_addr);

    let mut tasks = Vec::new();

    for _ in 0..1024 {
        if let Ok((stream, addr)) = listener.accept().await {
            println!("Starting proxy {} => {}", addr.to_string(), remote_addr);
            let socket = TcpSocket::new_v4()?;
            let remote_stream = socket
                .connect(
                    remote_addr
                        .parse()
                        .map_err(|e| Error::new(io::ErrorKind::Other, e))?,
                )
                .await?;

            tasks.push(tokio::task::spawn(create_proxy(stream, remote_stream)));
        }
    }

    for task in tasks {
        let _ = task.await;
    }

    Ok(())
}

async fn create_proxy(mut local: TcpStream, mut remote: TcpStream) -> io::Result<()> {
    // C→S: Handshake with Next State set to 2 (login)
    let handshake = Packet::read_uncompressed(&mut local).await?;

    let mut packet = Handshake::deserialize(&handshake).await?;
    packet.server_port = 25565;

    if packet.next_state.0 != 0x02 {
        packet.serialize().write(&mut remote, None).await?;
        return start_proxy(local, remote, State::Status, None).await;
    }

    packet.serialize().write(&mut remote, None).await?;

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
        println!("#{} {:?} => {:?}", self.count, &self.bound, &packet);
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
        let mut packet_reader = PacketReader::new(&packet);
        println!(
            "Server info: {}",
            packet_reader.read_string().await.unwrap()
        );
        packet
    }

    pub async fn sb_0x04_chat_command(&mut self, packet: UncompressedPacket) -> UncompressedPacket {
        if let Ok(chat_message) = ChatCommand::deserialize(&packet).await {
            println!("Player send command: {}", chat_message.command);
        }
        packet
    }
}

#[allow(dead_code)]
#[allow(unused_imports)]
mod tests {
    use std::io::Write;

    use tokio::{fs::File, io::AsyncReadExt};

    use crate::{
        packet::{packet::Packet, packet_builder::PacketBuilder},
        types::var_int::VarInt,
    };

    #[tokio::test]
    async fn stupid_test2() {
        let mut test: &[u8] = &[0x01, 0x02, 0x03, 0x04];
        test.read_u8().await.unwrap();
        assert_eq!(test, &[0x02, 0x03, 0x04]);
    }

    #[tokio::test]
    async fn stupid_test() {
        let mut test: &[u8] = &[0x01, 0x02, 0x03, 0x04];
        let mut test2 = Vec::new();
        test2.write_all(&mut test).unwrap();
        assert_eq!(test, &test2);
    }

    // #[tokio::test]
    // async fn zlib_test() {
    //     let mut file = File::open("packets.bin").await.unwrap();
    //     let mut packet = Vec::new();
    //     file.read_to_end(&mut packet).await.unwrap();

    //     let decompressed = Packet::decompress_data(&packet).await.unwrap();
    //     let compressed = Packet::compress_data(&decompressed).await.unwrap();

    //     let original_first = &packet[..100];
    //     let compressed_first = &compressed[..100];

    //     assert_eq!(original_first, compressed_first, "Data differs");
    //     assert_eq!(packet.len(), compressed.len());
    // }

    #[tokio::test]
    async fn packet_compress_test() {
        let packet = PacketBuilder::new(VarInt(11))
            .write_string("ASJBIBDIVBDIBIUBWRUWBRIWUBUIUWFDBVI".to_string())
            .write_int(23312313)
            .write_var_int(VarInt(1231231))
            .build();
        let compressed = packet.compress(1).await.unwrap();

        let mut buf = Vec::new();
        compressed.write(&mut buf).await.unwrap();

        let mut stream = &buf[..];
        let packet_new = Packet::read(&mut stream, Some(1)).await.unwrap();
        if let Packet::Compressed(packet_new) = packet_new {
            assert_eq!(packet.data, packet_new.decompress().await.unwrap().data);
        }
    }

    #[tokio::test]
    async fn test_write_sync() {
        let tests: Vec<Test> = vec![
            Test::new(&[0x01], 1),
            Test::new(&[0x02], 2),
            Test::new(&[0x7F], 127),
            Test::new(&[0x80, 0x01], 128),
            Test::new(&[0xFF, 0x01], 255),
            Test::new(&[0xAC, 0x02], 300),
            Test::new(&[0x80, 0x80, 0x01], 16384),
            Test::new(&[0xDD, 0xC7, 0x01], 25565),
            Test::new(&[0xFF, 0xFF, 0x7F], 2097151),
            Test::new(&[0xFF, 0xFF, 0xFF, 0xFF, 0x07], 2147483647),
        ];
        for test in tests {
            let mut stream: Vec<u8> = Vec::new();

            VarInt(test.int).write_sync(&mut stream).unwrap();
            assert_eq!(stream, test.var_int);
        }
    }
    #[tokio::test]
    async fn test_read_sync() {
        let tests: Vec<Test> = vec![
            Test::new(&[0x01], 1),
            Test::new(&[0x02], 2),
            Test::new(&[0x7F], 127),
            Test::new(&[0x80, 0x01], 128),
            Test::new(&[0xFF, 0x01], 255),
            Test::new(&[0xAC, 0x02], 300),
            Test::new(&[0x80, 0x80, 0x01], 16384),
            Test::new(&[0xDD, 0xC7, 0x01], 25565),
            Test::new(&[0xFF, 0xFF, 0x7F], 2097151),
            Test::new(&[0xFF, 0xFF, 0xFF, 0xFF, 0x07], 2147483647),
        ];

        for test in tests {
            let mut stream = &test.var_int[..];
            assert_eq!(VarInt::read_sync(&mut stream).unwrap().0, test.int);
        }
    }

    #[tokio::test]
    async fn test_write() {
        let tests: Vec<Test> = vec![
            Test::new(&[0x01], 1),
            Test::new(&[0x02], 2),
            Test::new(&[0x7F], 127),
            Test::new(&[0x80, 0x01], 128),
            Test::new(&[0xFF, 0x01], 255),
            Test::new(&[0xAC, 0x02], 300),
            Test::new(&[0x80, 0x80, 0x01], 16384),
            Test::new(&[0xDD, 0xC7, 0x01], 25565),
            Test::new(&[0xFF, 0xFF, 0x7F], 2097151),
            Test::new(&[0xFF, 0xFF, 0xFF, 0xFF, 0x07], 2147483647),
        ];
        for test in tests {
            let mut stream: Vec<u8> = Vec::new();

            VarInt(test.int).write(&mut stream).await.unwrap();
            assert_eq!(stream, test.var_int);
        }
    }
    #[tokio::test]
    async fn test_read() {
        let tests: Vec<Test> = vec![
            Test::new(&[0x01], 1),
            Test::new(&[0x02], 2),
            Test::new(&[0x7F], 127),
            Test::new(&[0x80, 0x01], 128),
            Test::new(&[0xFF, 0x01], 255),
            Test::new(&[0xAC, 0x02], 300),
            Test::new(&[0x80, 0x80, 0x01], 16384),
            Test::new(&[0xDD, 0xC7, 0x01], 25565),
            Test::new(&[0xFF, 0xFF, 0x7F], 2097151),
            Test::new(&[0xFF, 0xFF, 0xFF, 0xFF, 0x07], 2147483647),
        ];

        for test in tests {
            let mut stream = &test.var_int[..];
            assert_eq!(VarInt::read(&mut stream).await.unwrap().0, test.int);
        }
    }

    struct Test {
        var_int: Vec<u8>,
        int: i32,
    }

    impl Test {
        fn new(var_int: &[u8], expect: i32) -> Self {
            Test {
                var_int: Vec::from(var_int),
                int: expect,
            }
        }
    }
}
