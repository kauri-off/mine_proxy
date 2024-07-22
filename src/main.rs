use std::io::{self, Error};

use proxy::packet_proxy::create_proxy;
use tokio::net::TcpSocket;

mod packet;
mod packets;
mod proxy;
mod types;

#[tokio::main]
async fn main() {
    loop {
        tokio::task::spawn(packet_proxy());
    }
}

async fn packet_proxy() -> io::Result<()> {
    let local_addr = "127.0.0.1:25566".parse().unwrap();
    let remote_addr = "127.0.0.1:25565";

    let socket = TcpSocket::new_v4()?;
    socket.set_reuseaddr(true)?;
    socket.bind(local_addr)?;

    let listener = socket.listen(1024)?;
    println!("Waiting on {}", local_addr);

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

        if let Err(e) = create_proxy(stream, remote_stream, None, None).await {
            println!("Error, {:?}", e);
        }
    }

    Ok(())
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
