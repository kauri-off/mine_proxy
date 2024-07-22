use std::io::{self, Cursor, Read, Write};

use crate::types::var_int::VarInt;
use flate2::{bufread::ZlibDecoder, write::ZlibEncoder, Compression};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub struct Packet {
    pub packet_id: VarInt,
    pub data: Vec<u8>,
}

impl Packet {
    pub async fn read<R: AsyncRead + Unpin>(
        reader: &mut R,
        threshold: Option<i32>,
    ) -> io::Result<Self> {
        let length = VarInt::read(reader).await?;
        let mut body = vec![0; length.0 as usize];

        reader.read_exact(&mut body).await?;

        let body = match threshold {
            Some(_) => {
                let mut stream = &body[..];
                let data_lenght = VarInt::read(&mut stream).await?;

                match data_lenght.0 {
                    0 => stream.to_vec(),
                    _ => Packet::decompress_data(stream).await?,
                }
            }
            None => body,
        };

        let mut stream = &body[..];
        let packet_id = VarInt::read(&mut stream).await?;

        let data = stream.to_vec();

        Ok(Packet { packet_id, data })
    }

    pub async fn write<W: AsyncWrite + Unpin>(
        self: &Self,
        writer: &mut W,
        threshold: Option<i32>,
    ) -> io::Result<()> {
        let mut data = Vec::new();
        self.packet_id.write(&mut data).await?;

        data.extend(&self.data);

        let data = match threshold {
            Some(threshold) => {
                if data.len() as i32 >= threshold {
                    let compressed_data = Packet::compress_data(&data).await?;

                    let mut data = Vec::new();
                    VarInt(compressed_data.len() as i32)
                        .write(&mut data)
                        .await?;
                    data.extend(&compressed_data);

                    data
                } else {
                    let mut buf = Vec::new();
                    VarInt(0).write(&mut buf).await?;
                    buf.extend(data);

                    buf
                }
            }
            None => data,
        };

        VarInt(data.len() as i32).write(writer).await?;
        writer.write_all(&data).await?;

        Ok(())
    }

    pub async fn compress_data(data: &[u8]) -> io::Result<Vec<u8>> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::new(9));
        encoder.write_all(data)?;
        let compressed_data = encoder.finish()?;
        Ok(compressed_data)
    }

    pub async fn decompress_data(data: &[u8]) -> io::Result<Vec<u8>> {
        let mut decoder = ZlibDecoder::new(Cursor::new(data));
        let mut decompressed_data = Vec::new();
        decoder.read_to_end(&mut decompressed_data)?;
        Ok(decompressed_data)
    }
}
