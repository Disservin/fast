//! Lines already found for a position, kept so returning to it shows them at once.

use super::*;

/// Positions remembered per engine before the cache starts over.
pub(super) const CACHE_LIMIT: usize = 50_000;

/// Lines per engine and position hash. Keyed by position, not by node, so a
/// transposition reached by another move order finds the same analysis.
#[derive(Default)]
pub struct LineCache {
    entries: HashMap<(EngineId, u64), BTreeMap<u32, LiveLine>>,
}

impl LineCache {
    pub fn get(&self, engine: EngineId, hash: u64) -> Option<&BTreeMap<u32, LiveLine>> {
        self.entries.get(&(engine, hash))
    }

    /// Stores lines, keeping deeper ones already known for the position.
    pub fn store(&mut self, engine: EngineId, hash: u64, lines: &BTreeMap<u32, LiveLine>) {
        if lines.is_empty() {
            return;
        }
        if self.entries.len() >= CACHE_LIMIT {
            self.entries.clear();
        }
        let deepest =
            |lines: &BTreeMap<u32, LiveLine>| lines.get(&1).map_or(0, |line| line.update.depth);
        let key = (engine, hash);
        if self.entries.get(&key).is_some_and(|known| {
            known.get(&1).map(|line| line.position_context.as_str())
                == lines.get(&1).map(|line| line.position_context.as_str())
                && deepest(known) > deepest(lines)
        }) {
            return;
        }
        let stored: BTreeMap<_, _> = lines
            .iter()
            .filter(|(_, line)| !line.stale)
            .map(|(multipv, line)| {
                let mut line = line.clone();
                line.cached = true;
                (*multipv, line)
            })
            .collect();
        if stored.is_empty() {
            return;
        }
        self.entries.insert(key, stored);
    }

    pub fn forget_engine(&mut self, engine: EngineId) {
        self.entries.retain(|(id, _), _| *id != engine);
    }

    /// The best line's White-relative eval and depth for a position from any engine,
    /// preferring the order given.
    pub fn eval(
        &self,
        engines: &[EngineId],
        hash: u64,
        position_context: &str,
    ) -> Option<(i32, u32)> {
        engines.iter().find_map(|&id| {
            let line = self.get(id, hash)?.get(&1)?;
            if line.position_context != position_context {
                return None;
            }
            Some((score_cp(line.score?), line.update.depth))
        })
    }
}
