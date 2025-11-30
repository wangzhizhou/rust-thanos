use byteorder::{ByteOrder, LittleEndian};
use rust_thanos::mca::entry::{decode_lz4_blocks, lz4_checksum};

#[test]
fn lz4_roundtrip() {
    let payload = b"Hello, LZ4!".to_vec();
    let compressed = lz4_flex::block::compress_prepend_size(&payload);
    let mut header = Vec::new();
    header.extend_from_slice(b"LZ4Block");
    header.push(0x20); // token: method LZ4, level base
    let mut buf = [0u8; 4];
    LittleEndian::write_u32(&mut buf, (compressed.len() - 4) as u32);
    header.extend_from_slice(&buf);
    LittleEndian::write_u32(&mut buf, payload.len() as u32);
    header.extend_from_slice(&buf);
    let checksum = lz4_checksum(&payload);
    LittleEndian::write_u32(&mut buf, checksum);
    header.extend_from_slice(&buf);
    let mut stream = header;
    stream.extend_from_slice(&compressed[4..]);
    let decoded = decode_lz4_blocks(&stream).expect("decode");
    assert_eq!(decoded, payload);
}
