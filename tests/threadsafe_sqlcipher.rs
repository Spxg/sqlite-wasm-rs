//! Workers sharing one SQLCipher build, each keyed connection on its own worker.

#[path = "support/scenarios.rs"]
mod scenarios;
#[path = "support/workers.rs"]
mod workers;

use scenarios::{init, Connection};
use sqlite_wasm_rs::vfs::memvfs::{MemVfsContext, MemVfsUtil};
use sqlite_wasm_rs::vfs::transfer::DbTransfer;
use sqlite_wasm_rs::SQLITE_NOTADB;
use std::sync::atomic::{AtomicBool, Ordering};
use wasm_bindgen_test::{wasm_bindgen_test, wasm_bindgen_test_configure};

wasm_bindgen_test_configure!(run_in_dedicated_worker);

// A low work factor keeps each keyed open cheap, and every connection to one file must agree on it.
const KEY: &str = "PRAGMA key = 'first passphrase'; PRAGMA kdf_iter = 4000;";
const NEW_KEY: &str = "PRAGMA key = 'second passphrase'; PRAGMA kdf_iter = 4000;";

fn open_keyed(name: &str, key: &str) -> Connection {
    let connection = Connection::open(name);
    connection.exec(key);
    connection
}

/// Count and sum of the pairs table, or the result code of a refused read.
fn pair_totals(connection: &Connection) -> Result<[i64; 2], i32> {
    connection
        .try_query_row("SELECT count(*), coalesce(sum(v), 0) FROM t")
        .map(|row| row.expect("an aggregate returns one row"))
}

fn assert_cipher_integrity(connection: &Connection) {
    // The pragma returns one row per problem.
    assert_eq!(
        connection.try_query_row::<1>("PRAGMA cipher_integrity_check"),
        Ok(None)
    );
}

/// Creates `name` keyed with `KEY`, holding `pairs` pairs that sum to zero.
fn keyed_pairs(name: &str, pairs: i64) -> Connection {
    let connection = open_keyed(name, KEY);
    scenarios::create_pairs(&connection);
    connection.exec(&format!(
        "WITH RECURSIVE n(k) AS (SELECT 1 UNION ALL SELECT k + 1 FROM n WHERE k < {pairs})
         INSERT INTO t SELECT k, k FROM n UNION ALL SELECT -k, -k FROM n;"
    ));
    connection
}

#[wasm_bindgen_test]
async fn every_worker_draws_its_own_entropy() {
    const WORKERS: usize = 8;

    init();
    scenarios::run_together(WORKERS, |worker, start| {
        start.wait();
        // A new keyed database takes its salt from Fortuna, seeded by this worker's getentropy.
        let connection = keyed_pairs(&format!("entropy-{worker}.db"), 10);
        assert_eq!(pair_totals(&connection), Ok([20, 0]));
    })
    .await;

    let context = MemVfsContext::try_new().unwrap();
    let util = MemVfsUtil::get(&context).unwrap();
    let mut salts: Vec<Vec<u8>> = (0..WORKERS)
        .map(|worker| util.export_db(&format!("entropy-{worker}.db")).unwrap()[..16].to_vec())
        .collect();
    assert!(
        salts.iter().all(|salt| salt.iter().any(|&byte| byte != 0)),
        "an all-zero salt"
    );
    salts.sort();
    salts.dedup();
    assert_eq!(salts.len(), WORKERS, "two workers produced the same salt");
}

#[wasm_bindgen_test]
async fn concurrent_keyed_reads() {
    const WORKERS: usize = 8;
    const OPENS: usize = 25;
    const DB: &str = "sqlcipher-reads.db";

    init();
    let setup = keyed_pairs(DB, 500);
    scenarios::run_together(WORKERS, |_, start| {
        start.wait();
        // Opening and closing each time also churns SQLCipher's provider activation count.
        for _ in 0..OPENS {
            assert_eq!(pair_totals(&open_keyed(DB, KEY)), Ok([1000, 0]));
        }
    })
    .await;
    assert_eq!(pair_totals(&setup), Ok([1000, 0]));
    assert_cipher_integrity(&setup);
}

#[wasm_bindgen_test]
async fn keyed_increments_lose_no_update() {
    init();
    let setup =
        scenarios::concurrent_increments(|| open_keyed("sqlcipher-increments.db", KEY), 8, 50)
            .await;
    assert_cipher_integrity(&setup);
}

#[wasm_bindgen_test]
async fn keyed_writers_with_readers() {
    init();
    let setup = scenarios::readers_see_only_committed_states(
        || open_keyed("sqlcipher-writers.db", KEY),
        2,
        4,
        100,
    )
    .await;
    assert_cipher_integrity(&setup);
}

#[wasm_bindgen_test]
async fn rekey_while_others_read() {
    const READERS: usize = 6;
    const DB: &str = "sqlcipher-rekey.db";

    init();
    drop(keyed_pairs(DB, 2_000));
    let rekeyed = AtomicBool::new(false);
    scenarios::run_together(READERS + 1, move |index, start| {
        if index == READERS {
            let connection = open_keyed(DB, KEY);
            start.wait();
            connection.exec("PRAGMA rekey = 'second passphrase';");
            rekeyed.store(true, Ordering::Release);
            return;
        }
        start.wait();
        let mut reads_after_rekey = 0;
        while reads_after_rekey < 5 {
            let done = rekeyed.load(Ordering::Acquire);
            match pair_totals(&open_keyed(DB, if done { NEW_KEY } else { KEY })) {
                // A read is either the full committed table or a clean refusal, never other data.
                Ok(totals) => {
                    assert_eq!(totals, [4_000, 0], "read other data during rekey");
                    reads_after_rekey += usize::from(done);
                }
                Err(SQLITE_NOTADB) => assert!(!done, "the new key was refused after the rekey"),
                Err(rc) => panic!("read failed with {rc} during rekey"),
            }
        }
    })
    .await;

    let fresh = open_keyed(DB, NEW_KEY);
    assert_eq!(pair_totals(&fresh), Ok([4_000, 0]));
    assert_cipher_integrity(&fresh);
    assert_eq!(pair_totals(&open_keyed(DB, KEY)), Err(SQLITE_NOTADB));
}
