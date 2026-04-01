use std::collections::BTreeMap;

use super::working_copy::PatchWorkingCopy;

/// A specific DMX address in a specific universe.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct UniverseAddress {
    pub universe: usize,
    /// 1-indexed DMX address.
    pub address: usize,
}

pub(crate) type GroupName = String;

/// Maps each DMX address to the group names occupying it.
pub(crate) struct AddressMap(pub(crate) BTreeMap<UniverseAddress, Vec<GroupName>>);

impl AddressMap {
    pub fn from_working_copy(wc: &PatchWorkingCopy) -> Self {
        let mut map = BTreeMap::new();
        for group in &wc.groups {
            let name = group.config.key().to_string();
            for (pi, block) in group.config.patches.iter().enumerate() {
                let (start, count) = block.start_count();
                let Some(start_addr) = start else { continue };
                let ch_count = group.channel_counts.get(pi).copied().unwrap_or(0);
                if ch_count == 0 {
                    continue;
                }
                let mut addr = start_addr;
                for _ in 0..count {
                    let base = addr.dmx_index() + 1;
                    for ch in 0..ch_count {
                        let key = UniverseAddress {
                            universe: block.universe,
                            address: base + ch,
                        };
                        map.entry(key).or_insert_with(Vec::new).push(name.clone());
                    }
                    addr = addr + ch_count;
                }
            }
        }
        Self(map)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Check if a specific address has a collision (occupied by 2+ fixtures).
    pub fn collision_at(&self, addr: UniverseAddress) -> Option<String> {
        self.0.get(&addr).and_then(|names| {
            if names.len() > 1 {
                Some(names.join(", "))
            } else {
                None
            }
        })
    }

    /// Find an available contiguous run of `channel_count` free addresses
    /// in the given universe, starting the search from `start_after`.
    /// If nothing is available after that point, wraps around to addr 1.
    pub fn find_available(
        &self,
        universe: usize,
        channel_count: usize,
        start_after: usize,
    ) -> Option<usize> {
        if channel_count == 0 {
            return None;
        }
        let scan = |from: usize, to: usize| -> Option<usize> {
            let mut addr = from;
            while addr + channel_count - 1 <= to {
                let free = (0..channel_count).all(|offset| {
                    !self.0.contains_key(&UniverseAddress {
                        universe,
                        address: addr + offset,
                    })
                });
                if free {
                    return Some(addr);
                }
                addr += 1;
            }
            None
        };
        scan(start_after, 512).or_else(|| scan(1, start_after.saturating_sub(1)))
    }

    /// Return all universes that have at least one address in use.
    pub fn universes(&self) -> Vec<usize> {
        self.0
            .keys()
            .map(|k| k.universe)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// Iterate entries for a given universe.
    pub fn range_for_universe(
        &self,
        universe: usize,
    ) -> impl Iterator<Item = (&UniverseAddress, &Vec<GroupName>)> {
        let start = UniverseAddress {
            universe,
            address: 1,
        };
        let end = UniverseAddress {
            universe,
            address: 512,
        };
        self.0.range(start..=end)
    }
}
