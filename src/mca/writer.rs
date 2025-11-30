use crate::mca::entry::McaEntry;
use anyhow::Result;
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};

pub struct McaWriter {
    file: File,
    data_offset: u64,
    offsets: Vec<u32>,
    sizes: Vec<u32>,
    timestamps: Vec<u32>,
}

impl McaWriter {
    pub fn open(path: &str) -> Result<Self> {
        let mut f = File::create(path)?;
        f.write_all(&[0u8; 8192])?;
        Ok(Self {
            file: f,
            data_offset: 8192,
            offsets: vec![0; 1024],
            sizes: vec![0; 1024],
            timestamps: vec![0; 1024],
        })
    }

    pub fn write_entry(&mut self, entry: &mut McaEntry) -> Result<()> {
        let serialized = entry.serialized_bytes()?;
        let start = self.data_offset;
        self.file.seek(SeekFrom::Start(start))?;
        self.file.write_all(&serialized)?;
        let written = serialized.len() as u64;
        let pad = (4096 - (written % 4096)) % 4096;
        if pad > 0 {
            self.file.write_all(vec![0u8; pad as usize].as_slice())?;
        }
        self.data_offset += written + pad;
        let idx = entry.region_index() as usize;
        self.offsets[idx] = start as u32;
        self.sizes[idx] = (written + pad) as u32;
        self.timestamps[idx] = entry.modified_time();
        Ok(())
    }

    pub fn finalize(&mut self) -> Result<()> {
        let mut loc = Vec::with_capacity(4096);
        for i in 0..1024 {
            let off = self.offsets[i] / 4096;
            let size = self.sizes[i] / 4096;
            let v = (off << 8) | (size & 0xFF);
            loc.extend_from_slice(&v.to_be_bytes());
        }
        let mut time = Vec::with_capacity(4096);
        for i in 0..1024 {
            time.extend_from_slice(&self.timestamps[i].to_be_bytes());
        }
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(&loc)?;
        self.file.write_all(&time)?;
        Ok(())
    }
}
