use crate::mca::entry::McaEntry;
use crate::patterns::ChunkPattern;
use anyhow::Result;

#[allow(dead_code)]
pub struct RangePattern {
    sx: i32,
    sz: i32,
    ex: i32,
    ez: i32,
}

#[allow(dead_code)]
impl RangePattern {
    pub fn new(a: i32, b: i32, c: i32, d: i32) -> Self {
        let sx = a.min(c);
        let ex = a.max(c);
        let sz = b.min(d);
        let ez = b.max(d);
        Self { sx, sz, ex, ez }
    }
}

impl ChunkPattern for RangePattern {
    fn matches(&self, entry: &mut McaEntry) -> Result<bool> {
        let gx = entry.global_x();
        let gz = entry.global_z();
        Ok(gx >= self.sx && gx <= self.ex && gz >= self.sz && gz <= self.ez)
    }
}
