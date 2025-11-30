use crate::mca::entry::McaEntry;
use anyhow::{anyhow, Result};
use regex::Regex;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

pub struct McaReader {
    file: File,
    x_pos: i32,
    z_pos: i32,
    offsets: Option<Vec<u32>>,
    sizes: Option<Vec<u32>>,
    timestamps: Option<Vec<u32>>,
}

impl McaReader {
    pub fn open(path: &str) -> Result<Self> {
        let re = Regex::new(r#"r\.(-?\d+)\.(-?\d+)\.mca$"#)?;
        let caps = re
            .captures(path)
            .ok_or_else(|| anyhow!("invalid mca filename"))?;
        let x_pos: i32 = caps.get(1).unwrap().as_str().parse()?;
        let z_pos: i32 = caps.get(2).unwrap().as_str().parse()?;
        let file = File::open(path)?;
        Ok(Self {
            file,
            x_pos,
            z_pos,
            offsets: None,
            sizes: None,
            timestamps: None,
        })
    }

    fn read_header(&mut self) -> Result<()> {
        self.file.seek(SeekFrom::Start(0))?;
        let mut loc = vec![0u8; 4096];
        self.file.read_exact(&mut loc)?;
        let mut time = vec![0u8; 4096];
        self.file.read_exact(&mut time)?;
        let mut offsets = Vec::with_capacity(1024);
        let mut sizes = Vec::with_capacity(1024);
        for i in 0..1024 {
            let base = i * 4;
            let v = u32::from_be_bytes([loc[base], loc[base + 1], loc[base + 2], loc[base + 3]]);
            let off = (v >> 8) * 4096;
            let size = (v & 0xFF) * 4096;
            offsets.push(off);
            sizes.push(size);
        }
        let mut timestamps = Vec::with_capacity(1024);
        for i in 0..1024 {
            let base = i * 4;
            let v =
                u32::from_be_bytes([time[base], time[base + 1], time[base + 2], time[base + 3]]);
            timestamps.push(v);
        }
        self.offsets = Some(offsets);
        self.sizes = Some(sizes);
        self.timestamps = Some(timestamps);
        Ok(())
    }

    fn ensure(&mut self) -> Result<()> {
        if self.offsets.is_none() {
            self.read_header()?;
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn x_pos(&self) -> i32 {
        self.x_pos
    }
    #[allow(dead_code)]
    pub fn z_pos(&self) -> i32 {
        self.z_pos
    }

    pub fn entries(&mut self) -> Result<Vec<McaEntry>> {
        self.ensure()?;
        let offsets = self.offsets.as_ref().unwrap();
        let sizes = self.sizes.as_ref().unwrap();
        let timestamps = self.timestamps.as_ref().unwrap();
        let mut out = Vec::new();
        for i in 0..1024 {
            let off = offsets[i];
            let size = sizes[i];
            let ts = timestamps[i];
            if off == 0 || size == 0 {
                continue;
            }
            out.push(McaEntry::new(
                self.file.try_clone()?,
                off as u64,
                size as usize,
                i as u32,
                ts,
                self.x_pos,
                self.z_pos,
            ));
        }
        Ok(out)
    }

    pub fn get(&mut self, index: usize) -> Result<Option<McaEntry>> {
        self.ensure()?;
        let offsets = self.offsets.as_ref().unwrap();
        let sizes = self.sizes.as_ref().unwrap();
        let timestamps = self.timestamps.as_ref().unwrap();
        let off = offsets[index];
        let size = sizes[index];
        let ts = timestamps[index];
        if off == 0 || size == 0 {
            return Ok(None);
        }
        Ok(Some(McaEntry::new(
            self.file.try_clone()?,
            off as u64,
            size as usize,
            index as u32,
            ts,
            self.x_pos,
            self.z_pos,
        )))
    }
}
