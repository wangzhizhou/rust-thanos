use anyhow::{anyhow, Result};
use byteorder::{ByteOrder, LittleEndian};
use flate2::read::{GzDecoder, ZlibDecoder};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use xxhash_rust::xxh32::xxh32;

pub struct McaEntry {
    file: File,
    start: u64,
    _length: usize,
    index: u32,
    modified: u32,
    region_x: i32,
    region_z: i32,
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum CompressionMethod {
    Gzip,
    Zlib,
    Raw,
    Lz4,
    Custom,
    ExternalGzip,
    ExternalZlib,
    ExternalRaw,
    ExternalLz4,
}

impl McaEntry {
    pub fn new(
        file: File,
        start: u64,
        length: usize,
        index: u32,
        modified: u32,
        region_x: i32,
        region_z: i32,
    ) -> Self {
        Self {
            file,
            start,
            _length: length,
            index,
            modified,
            region_x,
            region_z,
        }
    }
    pub fn region_index(&self) -> u32 {
        self.index
    }
    pub fn x_pos(&self) -> i32 {
        (self.index % 32) as i32
    }
    pub fn z_pos(&self) -> i32 {
        (self.index / 32) as i32
    }
    pub fn global_x(&self) -> i32 {
        self.region_x * 32 + self.x_pos()
    }
    pub fn global_z(&self) -> i32 {
        self.region_z * 32 + self.z_pos()
    }
    pub fn modified_time(&self) -> u32 {
        self.modified
    }

    pub fn read_header(&mut self) -> Result<(u32, CompressionMethod, Option<String>)> {
        self.file.seek(SeekFrom::Start(self.start))?;
        let mut buf = [0u8; 5];
        self.file.read_exact(&mut buf)?;
        let len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
        let method = buf[4] as i8;
        let cm = match method {
            1 => CompressionMethod::Gzip,
            2 => CompressionMethod::Zlib,
            3 => CompressionMethod::Raw,
            4 => CompressionMethod::Lz4,
            127 => CompressionMethod::Custom,
            -127 => CompressionMethod::ExternalGzip,
            -126 => CompressionMethod::ExternalZlib,
            -125 => CompressionMethod::ExternalRaw,
            -124 => CompressionMethod::ExternalLz4,
            _ => return Err(anyhow!("unknown compression")),
        };
        let mut custom = None;
        if cm == CompressionMethod::Custom {
            let mut lbuf = [0u8; 2];
            self.file.read_exact(&mut lbuf)?;
            let n = u16::from_be_bytes(lbuf) as usize;
            let mut s = vec![0u8; n];
            self.file.read_exact(&mut s)?;
            custom = Some(String::from_utf8_lossy(&s).to_string());
        }
        Ok((len, cm, custom))
    }

    pub fn serialized_bytes(&mut self) -> Result<Vec<u8>> {
        let (len, _, _) = self.read_header()?;
        let total = 4u64 + len as u64;
        self.file.seek(SeekFrom::Start(self.start))?;
        let mut out = Vec::new();
        std::io::Read::take(&mut self.file, total).read_to_end(&mut out)?;
        Ok(out)
    }

    pub fn data_bytes(&mut self) -> Result<(CompressionMethod, Vec<u8>, Option<String>)> {
        let (len, cm, custom) = self.read_header()?;
        let mut pos = self.start + 5;
        if cm == CompressionMethod::Custom {
            let mut lbuf = [0u8; 2];
            self.file.read_exact(&mut lbuf)?;
            let n = u16::from_be_bytes(lbuf) as usize;
            let mut s = vec![0u8; n];
            self.file.read_exact(&mut s)?;
            pos += 2 + n as u64;
        }
        let data_len = len as usize
            - 1
            - if cm == CompressionMethod::Custom {
                2 + custom.as_ref().map(|v| v.len()).unwrap_or(0)
            } else {
                0
            };
        self.file.seek(SeekFrom::Start(pos))?;
        let mut data = Vec::new();
        std::io::Read::take(&mut self.file, data_len as u64).read_to_end(&mut data)?;
        Ok((cm, data, custom))
    }

    pub fn all_data_uncompressed(&mut self) -> Result<Vec<u8>> {
        let (cm, data, _) = self.data_bytes()?;
        match cm {
            CompressionMethod::Raw => Ok(data),
            CompressionMethod::Zlib => {
                let mut d = ZlibDecoder::new(&data[..]);
                let mut out = Vec::new();
                std::io::copy(&mut d, &mut out)?;
                Ok(out)
            }
            CompressionMethod::Gzip => {
                let mut d = GzDecoder::new(&data[..]);
                let mut out = Vec::new();
                std::io::copy(&mut d, &mut out)?;
                Ok(out)
            }
            CompressionMethod::Lz4 => decode_lz4_blocks(&data),
            _ => Ok(Vec::new()),
        }
    }

    pub fn is_external(&mut self) -> Result<bool> {
        let (_, cm, _) = self.read_header()?;
        Ok(matches!(
            cm,
            CompressionMethod::ExternalGzip
                | CompressionMethod::ExternalZlib
                | CompressionMethod::ExternalRaw
                | CompressionMethod::ExternalLz4
        ))
    }
}

const LZ4_MAGIC: &[u8] = b"LZ4Block";
const LZ4_HEADER_LEN: usize = 8 + 1 + 4 + 4 + 4;
const LZ4_XXHASH_SEED: u32 = 0x9747b28c;

pub fn lz4_checksum(data: &[u8]) -> u32 {
    xxh32(data, LZ4_XXHASH_SEED) & 0x0FFFFFFF
}

pub fn decode_lz4_blocks(inp: &[u8]) -> Result<Vec<u8>> {
    let mut i = 0usize;
    let mut out = Vec::new();
    while i + LZ4_HEADER_LEN <= inp.len() {
        if &inp[i..i + 8] != LZ4_MAGIC {
            return Err(anyhow!("invalid LZ4 magic"));
        }
        let token = inp[i + 8];
        let method = token & 0xF0; // 0x10 RAW, 0x20 LZ4
        let comp_len = LittleEndian::read_u32(&inp[i + 9..i + 13]) as usize;
        let decomp_len = LittleEndian::read_u32(&inp[i + 13..i + 17]) as usize;
        let checksum_le = LittleEndian::read_u32(&inp[i + 17..i + 21]);
        let start = i + LZ4_HEADER_LEN;
        if start + comp_len > inp.len() {
            return Err(anyhow!("LZ4 block truncated"));
        }
        let block = &inp[start..start + comp_len];
        let decoded = if method == 0x10 {
            // RAW
            block.to_vec()
        } else if method == 0x20 {
            // LZ4
            let mut with_prepended = Vec::with_capacity(4 + block.len());
            let mut size_buf = [0u8; 4];
            LittleEndian::write_u32(&mut size_buf, decomp_len as u32);
            with_prepended.extend_from_slice(&size_buf);
            with_prepended.extend_from_slice(block);
            lz4_flex::block::decompress_size_prepended(&with_prepended)?
        } else {
            return Err(anyhow!("unsupported LZ4 method"));
        };
        let checksum = lz4_checksum(&decoded);
        if checksum != checksum_le {
            return Err(anyhow!("LZ4 checksum mismatch"));
        }
        out.extend_from_slice(&decoded);
        i = start + comp_len;
    }
    if i != inp.len() {
        return Err(anyhow!("dangling LZ4 bytes"));
    }
    Ok(out)
}
