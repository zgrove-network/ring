use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use zcash_client_backend::data_api::chain::{error, BlockCache, BlockSource};
use zcash_client_backend::data_api::scanning::ScanRange;
use zcash_client_backend::proto::compact_formats::CompactBlock;
use zcash_protocol::consensus::BlockHeight;

/// Somewhere to put compact blocks between downloading them and scanning them.
///
/// No published crate implements `BlockCache`, so it is written here. It is
/// held in memory on purpose: the sync loop downloads a batch, scans it, and
/// deletes it, so nothing here outlives a batch. What the scan *learned* goes
/// into the wallet database, which is on disk; these blocks are scaffolding.
///
/// A BTreeMap rather than a list, so blocks come back in height order and a
/// block delivered twice replaces itself instead of being scanned twice.
#[derive(Clone, Default)]
pub struct MemoryBlockCache {
    blocks: Arc<Mutex<BTreeMap<u32, CompactBlock>>>,
}

impl MemoryBlockCache {
    pub fn new() -> Self {
        Self::default()
    }
}

impl BlockSource for MemoryBlockCache {
    type Error = std::convert::Infallible;

    fn with_blocks<F, WalletErrT>(
        &self,
        from_height: Option<BlockHeight>,
        limit: Option<usize>,
        mut with_block: F,
    ) -> Result<(), error::Error<WalletErrT, Self::Error>>
    where
        F: FnMut(CompactBlock) -> Result<(), error::Error<WalletErrT, Self::Error>>,
    {
        let held = self.blocks.lock().expect("the cache mutex was poisoned");
        let from = from_height.map_or(0, u32::from);

        // Contiguity matters: a gap means a block was missed, and scanning
        // across it would build a commitment tree that does not match the
        // chain. Stop at the first gap rather than carry on.
        let mut expected: Option<u32> = None;
        for (height, block) in held.range(from..) {
            if let Some(next) = expected {
                if *height != next {
                    break;
                }
            }
            expected = Some(height + 1);
            with_block(block.clone())?;
            if let Some(limit) = limit {
                if usize::try_from(height - from + 1).unwrap_or(usize::MAX) >= limit {
                    break;
                }
            }
        }

        Ok(())
    }
}

#[async_trait]
impl BlockCache for MemoryBlockCache {
    fn get_tip_height(
        &self,
        range: Option<&ScanRange>,
    ) -> Result<Option<BlockHeight>, Self::Error> {
        let held = self.blocks.lock().expect("the cache mutex was poisoned");
        let highest = match range {
            None => held.keys().next_back().copied(),
            Some(range) => held
                .keys()
                .copied()
                .rfind(|h| range.block_range().contains(&BlockHeight::from_u32(*h))),
        };
        Ok(highest.map(BlockHeight::from_u32))
    }

    async fn read(&self, range: &ScanRange) -> Result<Vec<CompactBlock>, Self::Error> {
        let held = self.blocks.lock().expect("the cache mutex was poisoned");
        Ok(held
            .values()
            .filter(|b| {
                range
                    .block_range()
                    .contains(&BlockHeight::from_u32(b.height as u32))
            })
            .cloned()
            .collect())
    }

    async fn insert(&self, compact_blocks: Vec<CompactBlock>) -> Result<(), Self::Error> {
        let mut held = self.blocks.lock().expect("the cache mutex was poisoned");
        for block in compact_blocks {
            held.insert(block.height as u32, block);
        }
        Ok(())
    }

    async fn delete(&self, range: ScanRange) -> Result<(), Self::Error> {
        let mut held = self.blocks.lock().expect("the cache mutex was poisoned");
        held.retain(|h, _| !range.block_range().contains(&BlockHeight::from_u32(*h)));
        Ok(())
    }
}
