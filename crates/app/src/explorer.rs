//! The opening tree's database: which database the Reference column of "Moves from
//! here" counts, and a small cache of its answers so stepping back and forth through a
//! game costs nothing.

use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
};

use db::MoveStat;

/// Positions remembered per Analyse tab.
const CACHE_ENTRIES: usize = 512;

/// Picks the opening tree's database: the tab's own choice, then the database the open
/// game came from, then the last database picked anywhere, then `fallback` (the first
/// database that is not My games). Choices that no longer exist are passed over.
pub fn pick_database(
    tab_choice: Option<&Path>,
    game_database: Option<&Path>,
    last_choice: Option<&Path>,
    fallback: Option<&Path>,
    databases: &[PathBuf],
) -> Option<PathBuf> {
    let exists = |path: &&Path| databases.iter().any(|known| known == path);
    tab_choice
        .filter(exists)
        .or(game_database.filter(exists))
        .or(last_choice.filter(exists))
        .or(fallback.filter(exists))
        .map(Path::to_path_buf)
}

/// What one query asked: the database, the position and the player its games are
/// limited to (empty for everyone).
type CacheKey = (PathBuf, u64, String);

/// The most recent opening tree answers, oldest dropped first.
#[derive(Default)]
pub struct MoveCache {
    entries: HashMap<CacheKey, Vec<MoveStat>>,
    order: VecDeque<CacheKey>,
}

impl MoveCache {
    /// The stored answer for a position, if it is still remembered.
    pub fn get(&self, database: &Path, hash: u64, player: &str) -> Option<&Vec<MoveStat>> {
        self.entries
            .get(&(database.to_path_buf(), hash, player.to_owned()))
    }

    /// Remembers an answer, forgetting the oldest one when the cache is full.
    pub fn insert(&mut self, database: PathBuf, hash: u64, player: String, moves: Vec<MoveStat>) {
        let key = (database, hash, player);
        if self.entries.insert(key.clone(), moves).is_none() {
            self.order.push_back(key);
        }
        while self.order.len() > CACHE_ENTRIES {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
    }

    /// Forgets everything, as after an import, edit or deletion changed a database.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn the_tabs_choice_comes_first() {
        let all = paths(&["mine", "masters", "rep"]);
        let pick = pick_database(
            Some(Path::new("rep")),
            Some(Path::new("mine")),
            Some(Path::new("masters")),
            Some(Path::new("masters")),
            &all,
        );
        assert_eq!(pick, Some(PathBuf::from("rep")));
    }

    #[test]
    fn the_open_games_database_comes_next() {
        let all = paths(&["mine", "masters"]);
        let pick = pick_database(
            None,
            Some(Path::new("mine")),
            Some(Path::new("masters")),
            Some(Path::new("masters")),
            &all,
        );
        assert_eq!(pick, Some(PathBuf::from("mine")));
    }

    #[test]
    fn deleted_databases_are_passed_over() {
        let all = paths(&["mine", "masters"]);
        let pick = pick_database(
            Some(Path::new("gone")),
            Some(Path::new("also-gone")),
            Some(Path::new("deleted")),
            Some(Path::new("masters")),
            &all,
        );
        assert_eq!(pick, Some(PathBuf::from("masters")));
        let pick = pick_database(None, None, Some(Path::new("mine")), None, &all);
        assert_eq!(pick, Some(PathBuf::from("mine")));
    }

    #[test]
    fn no_databases_means_no_tree() {
        let pick = pick_database(
            Some(Path::new("rep")),
            None,
            None,
            Some(Path::new("masters")),
            &[],
        );
        assert_eq!(pick, None);
    }

    #[test]
    fn cache_forgets_the_oldest_answer() {
        let mut cache = MoveCache::default();
        for hash in 0..(CACHE_ENTRIES as u64 + 10) {
            cache.insert(PathBuf::from("db"), hash, String::new(), Vec::new());
        }
        assert_eq!(cache.len(), CACHE_ENTRIES);
        assert!(cache.get(Path::new("db"), 0, "").is_none());
        assert!(
            cache
                .get(Path::new("db"), CACHE_ENTRIES as u64, "")
                .is_some()
        );
        assert!(
            cache
                .get(Path::new("db"), CACHE_ENTRIES as u64, "me")
                .is_none()
        );
        cache.clear();
        assert_eq!(cache.len(), 0);
    }
}
