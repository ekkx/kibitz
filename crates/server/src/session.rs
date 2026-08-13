//! Sessions — one variation tree per game.
//!
//! In-process memory is enough for a local single-user tool:
//! `Arc<RwLock<HashMap<..>>>`.

use kibitz_core::tree::GameTree;
use std::collections::HashMap;
use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

#[derive(Clone, Default)]
pub struct SessionStore {
    inner: Arc<RwLock<HashMap<String, Session>>>,
}

#[derive(Clone)]
pub struct Session {
    pub id: String,
    pub tree: GameTree,
    pub headers: HashMap<String, String>,
}

impl SessionStore {
    pub fn new() -> SessionStore {
        SessionStore::default()
    }

    /// Store a new tree and return the generated session id.
    pub fn create(&self, tree: GameTree, headers: HashMap<String, String>) -> Session {
        let session = Session {
            id: new_id(),
            tree,
            headers,
        };
        self.inner
            .write()
            .expect("session store poisoned")
            .insert(session.id.clone(), session.clone());
        session
    }

    pub fn get(&self, id: &str) -> Option<Session> {
        self.inner
            .read()
            .expect("session store poisoned")
            .get(id)
            .cloned()
    }

    /// Read under the lock without cloning the whole tree.
    pub fn with<R>(&self, id: &str, f: impl FnOnce(&Session) -> R) -> Option<R> {
        self.inner
            .read()
            .expect("session store poisoned")
            .get(id)
            .map(f)
    }

    /// Mutate under the lock. **Never hold this across an `await`** — the guard is
    /// a `std::sync` one and the analysis pass is long-running.
    pub fn update<R>(&self, id: &str, f: impl FnOnce(&mut Session) -> R) -> Option<R> {
        self.inner
            .write()
            .expect("session store poisoned")
            .get_mut(id)
            .map(f)
    }

    pub fn remove(&self, id: &str) -> Option<Session> {
        self.inner
            .write()
            .expect("session store poisoned")
            .remove(id)
    }

    pub fn len(&self) -> usize {
        self.inner.read().expect("session store poisoned").len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Crockford-base32 ULID-alike: 48 bits of millisecond timestamp followed by 80
/// bits of randomness, so ids sort by creation time and read like the `01J...`
/// values in `docs/API.md`.
///
/// Rolled by hand rather than pulling in `ulid` + `rand` for twenty-six
/// characters. Randomness comes from `RandomState`, which the standard library
/// seeds from the OS, mixed with a process-local counter so two ids minted in
/// the same millisecond can never collide.
fn new_id() -> String {
    const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
        & 0x0000_ffff_ffff_ffff;

    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let low = RandomState::new().hash_one((millis, seq, &COUNTER as *const _ as usize));
    let high = RandomState::new().hash_one((low, seq));

    // 26 base32 characters = 130 bits; the top 2 are always zero.
    let value: u128 = ((millis as u128) << 80) | (((high as u128) & 0xffff) << 64) | low as u128;
    let mut out = [b'0'; 26];
    for (i, slot) in out.iter_mut().enumerate() {
        let shift = 5 * (25 - i);
        *slot = ALPHABET[((value >> shift) & 0x1f) as usize];
    }
    String::from_utf8(out.to_vec()).expect("base32 alphabet is ascii")
}

#[cfg(test)]
mod tests {
    use super::*;
    use shakmaty::Chess;

    fn tree() -> GameTree {
        GameTree::new(&Chess::default())
    }

    #[test]
    fn ids_are_unique_and_well_formed() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..10_000 {
            let id = new_id();
            assert_eq!(id.len(), 26, "{id}");
            assert!(id.bytes().all(|b| b.is_ascii_alphanumeric()), "{id}");
            assert!(seen.insert(id.clone()), "duplicate id {id}");
        }
    }

    #[test]
    fn ids_sort_by_creation_time() {
        let first = new_id();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let second = new_id();
        assert!(first < second, "{first} !< {second}");
    }

    #[test]
    fn create_get_update_remove() {
        let store = SessionStore::new();
        let session = store.create(tree(), HashMap::new());
        assert_eq!(store.len(), 1);
        assert!(store.get(&session.id).is_some());
        assert!(store.get("nope").is_none());

        store
            .update(&session.id, |s| s.tree.play_san(s.tree.root, "e4").unwrap())
            .unwrap();
        let after = store.get(&session.id).unwrap();
        assert_eq!(after.tree.nodes.len(), 2);

        // The clone handed out earlier is a snapshot, not a live view.
        assert_eq!(session.tree.nodes.len(), 1);

        assert!(store.remove(&session.id).is_some());
        assert!(store.is_empty());
    }

    #[test]
    fn clones_share_the_same_map() {
        let store = SessionStore::new();
        let clone = store.clone();
        let session = store.create(tree(), HashMap::new());
        assert!(clone.get(&session.id).is_some());
    }
}
