use easy_fs::{BlockDevice, EasyFileSystem};
use std::sync::{Arc, Mutex};

struct MemoryDisk(Mutex<Vec<[u8; 512]>>);

impl BlockDevice for MemoryDisk {
    fn read_block(&self, block_id: usize, buffer: &mut [u8]) {
        buffer.copy_from_slice(&self.0.lock().unwrap()[block_id]);
    }

    fn write_block(&self, block_id: usize, buffer: &[u8]) {
        self.0.lock().unwrap()[block_id].copy_from_slice(buffer);
    }
}

#[test]
fn hard_links_survive_unlink_and_recycle_storage() {
    let device: Arc<dyn BlockDevice> = Arc::new(MemoryDisk(Mutex::new(vec![[0; 512]; 4096])));
    let fs = EasyFileSystem::create(device.clone(), 4096, 1);
    let root = EasyFileSystem::root_inode(&fs);
    for name in ["", ".", "..", "a/b", "a\0b", "abcdefghijklmnopqrstuvwxyz12"] {
        assert!(root.create(name).is_none());
        assert!(root.find(name).is_none());
        assert!(!root.unlink(name));
    }
    let file = root.create("original").unwrap();
    let original_id = file.stat().0;
    assert_eq!(file.stat().1, 1);
    assert_eq!(file.write_at(0, b"shared data"), 11);
    assert!(!root.link("original", "original"));
    assert!(!root.link("missing", "alias"));
    assert!(root.link("original", "alias"));
    assert!(!root.link("original", "alias"));
    let alias = root.find("alias").unwrap();
    assert!(Arc::ptr_eq(&file, &alias));
    assert_eq!(alias.stat(), (original_id, 2, false));
    let mut buffer = [0; 11];
    assert_eq!(alias.read_at(0, &mut buffer), 11);
    assert_eq!(&buffer, b"shared data");
    assert!(root.unlink("original"));
    assert!(!root.unlink("original"));
    assert!(root.find("original").is_none());
    assert_eq!(file.stat().1, 1);
    assert!(root.unlink("alias"));
    assert_eq!(file.stat().1, 0);
    assert!(root.ls().is_empty());
    assert_eq!(alias.read_at(0, &mut buffer), 11);
    assert_eq!(&buffer, b"shared data");

    let replacement = root.create("original").unwrap();
    assert_ne!(replacement.stat().0, original_id);
    assert_eq!(replacement.read_at(0, &mut buffer), 0);
    drop(alias);
    assert_eq!(file.read_at(0, &mut buffer), 11);
    drop(file);
    let reused = root.create("reused").unwrap();
    assert_eq!(reused.stat().0, original_id);
    assert_eq!(reused.read_at(0, &mut buffer), 0);
    drop(reused);
    drop(replacement);
    assert!(root.unlink("original"));
    assert!(root.unlink("reused"));

    // Exercise direct, single-indirect, and double-indirect block reclamation.
    let payload = vec![0x63; 200 * 512 + 13];
    for _ in 0..32 {
        let file = root.create("large").unwrap();
        assert_eq!(file.write_at(0, &payload), payload.len());
        let mut tail = [0; 13];
        assert_eq!(file.read_at(200 * 512, &mut tail), 13);
        assert_eq!(tail, [0x63; 13]);
        drop(file);
        assert!(root.unlink("large"));
    }

    // Exceed the total inode count without exhausting the recycled bitmap.
    for _ in 0..5000 {
        let file = root.create("cycle").unwrap();
        assert_eq!(file.write_at(0, b"data"), 4);
        drop(file);
        assert!(root.unlink("cycle"));
    }
    assert!(root.ls().is_empty());

    let persistent = root.create("persistent").unwrap();
    persistent.write_at(0, b"on disk");
    let persistent_id = persistent.stat().0;
    assert!(root.link("persistent", "persisted_alias"));
    drop(persistent);
    drop(root);
    drop(fs);
    let reopened = EasyFileSystem::open(device);
    let root = EasyFileSystem::root_inode(&reopened);
    let alias = root.find("persisted_alias").unwrap();
    assert_eq!(alias.stat(), (persistent_id, 2, false));
    assert!(root.unlink("persistent"));
    assert_eq!(alias.stat().1, 1);
    assert!(root.unlink("persisted_alias"));
    assert_eq!(alias.stat().1, 0);
    let mut data = [0; 7];
    assert_eq!(alias.read_at(0, &mut data), 7);
    assert_eq!(&data, b"on disk");
    drop(alias);
    assert!(root.ls().is_empty());
}
