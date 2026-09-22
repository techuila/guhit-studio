//! Deterministic element ids.
//!
//! A new id is a pure function of (project state, command): the generator is
//! seeded with the project id, every id already in the project and the
//! command JSON, then counts up. `preview` and `apply` therefore assign the
//! same ids. The result is shaped like a UUID v4 string.
//!
//! One generator is made per leaf command, from the project state right
//! before that leaf runs. A `Batch` never takes part in the seed, so the ids
//! made by `Batch[a, b]` for `a` are the ids made by `Batch[a]` and by `a`
//! alone. The AI copilot relies on this while it stages a batch step by step.

use std::collections::BTreeSet;

use guhit_model::*;

pub struct IdGen {
    seed: u64,
    counter: u64,
    used: BTreeSet<Id>,
}

fn fnv1a(hash: &mut u64, bytes: &[u8]) {
    for b in bytes {
        *hash ^= *b as u64;
        *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    // Field separator, so ("ab", "c") and ("a", "bc") differ.
    *hash ^= 0xff;
    *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
}

fn splitmix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

impl IdGen {
    pub fn new(project: &Project, command: &Command) -> Self {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        let mut used = BTreeSet::new();
        fnv1a(&mut hash, project.id.as_bytes());
        for el in &project.elements {
            fnv1a(&mut hash, el.id().as_bytes());
            used.insert(el.id().clone());
        }
        for m in &project.materials {
            fnv1a(&mut hash, m.id.as_bytes());
            used.insert(m.id.clone());
        }
        for l in &project.levels {
            used.insert(l.id.clone());
        }
        let json = serde_json::to_string(command).unwrap_or_default();
        fnv1a(&mut hash, json.as_bytes());
        Self {
            seed: hash,
            counter: 0,
            used,
        }
    }

    /// Mark an id supplied by the caller as taken.
    pub fn reserve(&mut self, id: &Id) {
        self.used.insert(id.clone());
    }

    pub fn next_id(&mut self) -> Id {
        loop {
            let hi = splitmix(self.seed ^ self.counter.wrapping_mul(0x2545_f491_4f6c_dd1d));
            let lo = splitmix(hi ^ self.seed.rotate_left(32) ^ self.counter);
            self.counter += 1;
            // Version 4, variant 10xx, like a random UUID.
            let hi = (hi & 0xffff_ffff_ffff_0fff) | 0x0000_0000_0000_4000;
            let lo = (lo & 0x3fff_ffff_ffff_ffff) | 0x8000_0000_0000_0000;
            let id = format!(
                "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
                hi >> 32,
                (hi >> 16) & 0xffff,
                hi & 0xffff,
                lo >> 48,
                lo & 0xffff_ffff_ffff
            );
            if self.used.insert(id.clone()) {
                return id;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_uuid_shaped_deterministic_and_unique() {
        let project = defaults::new_project("t");
        let cmd = Command::DeleteElements { ids: vec![] };
        let mut a = IdGen::new(&project, &cmd);
        let mut b = IdGen::new(&project, &cmd);
        let mut seen = BTreeSet::new();
        for _ in 0..500 {
            let id = a.next_id();
            assert_eq!(id, b.next_id());
            assert_eq!(id.len(), 36);
            let parts: Vec<&str> = id.split('-').collect();
            assert_eq!(
                parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
                vec![8, 4, 4, 4, 12]
            );
            assert!(parts[2].starts_with('4'));
            assert!(matches!(
                parts[3].chars().next().unwrap(),
                '8' | '9' | 'a' | 'b'
            ));
            assert!(seen.insert(id));
        }
        let other = Command::DeleteElements {
            ids: vec!["x".into()],
        };
        assert_ne!(
            IdGen::new(&project, &other).next_id(),
            IdGen::new(&project, &cmd).next_id()
        );
    }
}
