//! Proves Elyra's Cache / Storage / Queue facades satisfy the shared
//! `substrate-core` contract — the "one ecosystem" guarantee.

use elyra::cache::Cache;
use elyra::queue::Queue;
use elyra::storage::Storage;
use elyra::substrate::{
    Cache as CacheContract, Queue as QueueContract, Storage as StorageContract,
};

#[test]
fn cache_conforms_to_contract() {
    fn exercise<C: CacheContract>(c: &C) {
        c.put("greeting", b"\"hello\"", None); // opaque JSON bytes
        assert_eq!(c.get("greeting").as_deref(), Some(&b"\"hello\""[..]));
        assert!(c.has("greeting"));
        assert!(!c.add("greeting", b"\"other\"", None));
        assert_eq!(c.increment("hits", 2), 2);
        assert_eq!(c.decrement("hits", 1), 1);
        assert!(c.forget("greeting"));
        c.flush();
    }
    exercise(&Cache::new());
}

#[test]
fn storage_conforms_to_contract() {
    let dir = std::env::temp_dir().join(format!("elyra-substrate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let disk = Storage::new(&dir);

    fn exercise<S: StorageContract>(s: &S) {
        assert!(!s.exists("a.txt"));
        s.put("a.txt", b"hi").unwrap();
        assert!(s.exists("a.txt"));
        assert_eq!(s.get("a.txt").unwrap(), b"hi");
        assert_eq!(s.size("a.txt").unwrap(), 2);
        assert_eq!(s.files("").unwrap(), vec!["a.txt".to_string()]);
        s.delete("a.txt").unwrap();
        assert!(s.put("../escape", b"no").is_err()); // jail enforced through the trait
    }
    exercise(&disk);
}

#[test]
fn queue_conforms_to_contract() {
    fn exercise<Q: QueueContract>(q: &Q) {
        q.push("log", b"{\"n\":1}"); // buffered until a worker starts
    }
    exercise(&Queue::new());
}
