use rsqlite_vfs::{MemChunksFile, VfsFile};

#[test]
fn truncated_bytes_do_not_reappear_after_sparse_writes() {
    // Retained tail, chunk crossing, multi-chunk gap, aligned EOF, empty file.
    for (retained, offset) in [(1usize, 2usize), (7, 9), (9, 26), (8, 16), (0, 8)] {
        for inferred_chunks in [false, true] {
            let mut file = if inferred_chunks {
                MemChunksFile::waiting_for_write()
            } else {
                MemChunksFile::new(8)
            };
            file.write(&[7; 24], 0).unwrap();
            file.truncate(retained as u64).unwrap();
            file.write(&[9; 3], offset as u64).unwrap();

            let mut expected = std::vec![7; retained];
            expected.resize(offset, 0);
            expected.extend_from_slice(&[9; 3]);
            let mut actual = std::vec![99; expected.len()];
            assert_eq!(file.size().unwrap(), expected.len() as u64);
            assert_eq!(file.read(&mut actual, 0).unwrap(), expected.len());
            assert_eq!(
                actual, expected,
                "retained={retained}, offset={offset}, inferred={inferred_chunks}"
            );
        }
    }
}
