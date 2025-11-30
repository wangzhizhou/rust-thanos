use byteorder::{ByteOrder, LittleEndian};
use rust_thanos::mca::entry::decode_lz4_blocks;

#[test]
fn lz4_invalid_checksum() {
    let payload = b"Bad checksum".to_vec();
    let compressed = lz4_flex::block::compress_prepend_size(&payload);
    let mut header = Vec::new();
    header.extend_from_slice(b"LZ4Block");
    header.push(0x20);
    let mut buf = [0u8; 4];
    LittleEndian::write_u32(&mut buf, (compressed.len() - 4) as u32);
    header.extend_from_slice(&buf);
    LittleEndian::write_u32(&mut buf, payload.len() as u32);
    header.extend_from_slice(&buf);
    // wrong checksum
    LittleEndian::write_u32(&mut buf, 0);
    header.extend_from_slice(&buf);
    let mut stream = header;
    stream.extend_from_slice(&compressed[4..]);
    assert!(decode_lz4_blocks(&stream).is_err());
}
