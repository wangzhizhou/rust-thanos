pub mod inhabited;
pub mod list;
pub mod range;

use crate::mca::entry::McaEntry;

pub trait ChunkPattern {
    fn matches(&self, entry: &mut McaEntry) -> anyhow::Result<bool>;
}
