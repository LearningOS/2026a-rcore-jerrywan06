use super::layout::NAME_LENGTH_LIMIT;
use super::{
    block_cache_sync_all, get_block_cache, BlockDevice, DirEntry, DiskInode, DiskInodeType,
    EasyFileSystem, DIRENT_SZ,
};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::{Mutex, MutexGuard};
/// Virtual filesystem layer over easy-fs
pub struct Inode {
    block_id: usize,
    block_offset: usize,
    fs: Arc<Mutex<EasyFileSystem>>,
    block_device: Arc<dyn BlockDevice>,
}

impl Inode {
    /// Create a vfs inode
    pub(crate) fn new(
        block_id: u32,
        block_offset: usize,
        fs: Arc<Mutex<EasyFileSystem>>,
        block_device: Arc<dyn BlockDevice>,
    ) -> Self {
        Self {
            block_id: block_id as usize,
            block_offset,
            fs,
            block_device,
        }
    }
    /// Call a function over a disk inode to read it
    fn read_disk_inode<V>(&self, f: impl FnOnce(&DiskInode) -> V) -> V {
        get_block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .read(self.block_offset, f)
    }
    /// Call a function over a disk inode to modify it
    fn modify_disk_inode<V>(&self, f: impl FnOnce(&mut DiskInode) -> V) -> V {
        get_block_cache(self.block_id, Arc::clone(&self.block_device))
            .lock()
            .modify(self.block_offset, f)
    }
    /// Find inode under a disk inode by name
    fn find_inode_id(&self, name: &str, disk_inode: &DiskInode) -> Option<u32> {
        // assert it is a directory
        assert!(disk_inode.is_dir());
        let file_count = (disk_inode.size as usize) / DIRENT_SZ;
        let mut dirent = DirEntry::empty();
        for i in 0..file_count {
            assert_eq!(
                disk_inode.read_at(DIRENT_SZ * i, dirent.as_bytes_mut(), &self.block_device,),
                DIRENT_SZ,
            );
            if !dirent.name().is_empty() && dirent.name() == name {
                return Some(dirent.inode_id() as u32);
            }
        }
        None
    }
    /// Find inode under current inode by name
    pub fn find(&self, name: &str) -> Option<Arc<Inode>> {
        if !Self::valid_name(name) {
            return None;
        }
        let mut fs = self.fs.lock();
        let inode_id = self.read_disk_inode(|disk_inode| self.find_inode_id(name, disk_inode))?;
        Some(self.cached_inode(inode_id, &mut fs))
    }

    fn valid_name(name: &str) -> bool {
        !name.is_empty()
            && name.len() <= NAME_LENGTH_LIMIT
            && !name.as_bytes().contains(&0)
            && !name.contains('/')
            && name != "."
            && name != ".."
    }

    fn cached_inode(&self, inode_id: u32, fs: &mut MutexGuard<EasyFileSystem>) -> Arc<Inode> {
        if let Some(inode) = fs
            .inode_cache
            .get(&inode_id)
            .and_then(|inode| inode.upgrade())
        {
            return inode;
        }
        let (block_id, block_offset) = fs.get_disk_inode_pos(inode_id);
        let inode = Arc::new(Self::new(
            block_id,
            block_offset,
            self.fs.clone(),
            self.block_device.clone(),
        ));
        fs.inode_cache.insert(inode_id, Arc::downgrade(&inode));
        inode
    }

    fn insert_dir_entry(&self, name: &str, inode_id: u32, fs: &mut MutexGuard<EasyFileSystem>) {
        self.modify_disk_inode(|root| {
            let count = root.size as usize / DIRENT_SZ;
            let mut entry = DirEntry::empty();
            let slot = (0..count)
                .find(|index| {
                    root.read_at(index * DIRENT_SZ, entry.as_bytes_mut(), &self.block_device);
                    entry.name().is_empty()
                })
                .unwrap_or(count);
            if slot == count {
                self.increase_size(((count + 1) * DIRENT_SZ) as u32, root, fs);
            }
            root.write_at(
                slot * DIRENT_SZ,
                DirEntry::new(name, inode_id).as_bytes(),
                &self.block_device,
            );
        });
    }

    fn link_count(&self, inode_id: u32, fs: &EasyFileSystem) -> u32 {
        if inode_id == 0 {
            return 1;
        }
        let (block_id, offset) = fs.get_disk_inode_pos(0);
        get_block_cache(block_id as usize, self.block_device.clone())
            .lock()
            .read(offset, |root: &DiskInode| {
                let mut links = 0;
                let mut entry = DirEntry::empty();
                for index in 0..root.size as usize / DIRENT_SZ {
                    root.read_at(index * DIRENT_SZ, entry.as_bytes_mut(), &self.block_device);
                    if !entry.name().is_empty() && entry.inode_id() == inode_id {
                        links += 1;
                    }
                }
                links
            })
    }

    fn reclaim_inode(&self, inode_id: u32, fs: &mut MutexGuard<EasyFileSystem>) {
        let (block_id, offset) = fs.get_disk_inode_pos(inode_id);
        let blocks = get_block_cache(block_id as usize, self.block_device.clone())
            .lock()
            .modify(offset, |inode: &mut DiskInode| {
                inode.clear_size(&self.block_device)
            });
        for block in blocks {
            fs.dealloc_data(block);
        }
        fs.dealloc_inode(inode_id);
    }

    /// Get the stable inode number, hard-link count, and directory flag.
    pub fn stat(&self) -> (u32, u32, bool) {
        let fs = self.fs.lock();
        let inode_id = fs.get_inode_id(self.block_id, self.block_offset);
        let directory = self.read_disk_inode(|inode| inode.is_dir());
        (inode_id, self.link_count(inode_id, &fs), directory)
    }

    /// Add another directory name for the same inode, without copying file data.
    /// Prepared with Codex assistance; see reports/lab4.md.
    pub fn link(&self, old_name: &str, new_name: &str) -> bool {
        if !Self::valid_name(old_name) || !Self::valid_name(new_name) || old_name == new_name {
            return false;
        }
        let mut fs = self.fs.lock();
        let Some(inode_id) = self.read_disk_inode(|root| self.find_inode_id(old_name, root)) else {
            return false;
        };
        if self
            .read_disk_inode(|root| self.find_inode_id(new_name, root))
            .is_some()
        {
            return false;
        }
        self.insert_dir_entry(new_name, inode_id, &mut fs);
        block_cache_sync_all();
        true
    }

    /// Remove a name and reclaim an unlinked inode once no open handle remains.
    pub fn unlink(&self, name: &str) -> bool {
        if !Self::valid_name(name) {
            return false;
        }
        let mut fs = self.fs.lock();
        let removed = self.modify_disk_inode(|root| {
            let mut entry = DirEntry::empty();
            for index in 0..root.size as usize / DIRENT_SZ {
                root.read_at(index * DIRENT_SZ, entry.as_bytes_mut(), &self.block_device);
                if entry.name() == name {
                    let inode_id = entry.inode_id();
                    root.write_at(
                        index * DIRENT_SZ,
                        DirEntry::empty().as_bytes(),
                        &self.block_device,
                    );
                    return Some(inode_id);
                }
            }
            None
        });
        let Some(inode_id) = removed else {
            return false;
        };
        if self.link_count(inode_id, &fs) == 0 && !fs.inode_cache.contains_key(&inode_id) {
            self.reclaim_inode(inode_id, &mut fs);
        }
        block_cache_sync_all();
        true
    }
    /// Increase the size of a disk inode
    fn increase_size(
        &self,
        new_size: u32,
        disk_inode: &mut DiskInode,
        fs: &mut MutexGuard<EasyFileSystem>,
    ) {
        if new_size < disk_inode.size {
            return;
        }
        let blocks_needed = disk_inode.blocks_num_needed(new_size);
        let mut v: Vec<u32> = Vec::new();
        for _ in 0..blocks_needed {
            v.push(fs.alloc_data());
        }
        disk_inode.increase_size(new_size, v, &self.block_device);
    }
    /// Create inode under current inode by name
    pub fn create(&self, name: &str) -> Option<Arc<Inode>> {
        if !Self::valid_name(name) {
            return None;
        }
        let mut fs = self.fs.lock();
        let op = |root_inode: &DiskInode| {
            // assert it is a directory
            assert!(root_inode.is_dir());
            // has the file been created?
            self.find_inode_id(name, root_inode)
        };
        if self.read_disk_inode(op).is_some() {
            return None;
        }
        // create a new file
        // alloc a inode with an indirect block
        let new_inode_id = fs.try_alloc_inode()?;
        // initialize inode
        let (new_inode_block_id, new_inode_block_offset) = fs.get_disk_inode_pos(new_inode_id);
        get_block_cache(new_inode_block_id as usize, Arc::clone(&self.block_device))
            .lock()
            .modify(new_inode_block_offset, |new_inode: &mut DiskInode| {
                new_inode.initialize(DiskInodeType::File);
            });
        self.insert_dir_entry(name, new_inode_id, &mut fs);
        block_cache_sync_all();
        Some(self.cached_inode(new_inode_id, &mut fs))
        // release efs lock automatically by compiler
    }
    /// List inodes under current inode
    pub fn ls(&self) -> Vec<String> {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            let file_count = (disk_inode.size as usize) / DIRENT_SZ;
            let mut v: Vec<String> = Vec::new();
            for i in 0..file_count {
                let mut dirent = DirEntry::empty();
                assert_eq!(
                    disk_inode.read_at(i * DIRENT_SZ, dirent.as_bytes_mut(), &self.block_device,),
                    DIRENT_SZ,
                );
                if !dirent.name().is_empty() {
                    v.push(String::from(dirent.name()));
                }
            }
            v
        })
    }
    /// Read data from current inode
    pub fn read_at(&self, offset: usize, buf: &mut [u8]) -> usize {
        let _fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| disk_inode.read_at(offset, buf, &self.block_device))
    }
    /// Write data to current inode
    pub fn write_at(&self, offset: usize, buf: &[u8]) -> usize {
        let mut fs = self.fs.lock();
        let size = self.modify_disk_inode(|disk_inode| {
            self.increase_size((offset + buf.len()) as u32, disk_inode, &mut fs);
            disk_inode.write_at(offset, buf, &self.block_device)
        });
        block_cache_sync_all();
        size
    }
    /// Clear the data in current inode
    pub fn clear(&self) {
        let mut fs = self.fs.lock();
        self.modify_disk_inode(|disk_inode| {
            let size = disk_inode.size;
            let data_blocks_dealloc = disk_inode.clear_size(&self.block_device);
            assert!(data_blocks_dealloc.len() == DiskInode::total_blocks(size) as usize);
            for data_block in data_blocks_dealloc.into_iter() {
                fs.dealloc_data(data_block);
            }
        });
        block_cache_sync_all();
    }
}

impl Drop for Inode {
    fn drop(&mut self) {
        let mut fs = self.fs.lock();
        let inode_id = fs.get_inode_id(self.block_id, self.block_offset);
        if inode_id == 0 {
            return;
        }
        // A concurrent lookup can have replaced an expired weak cache entry.
        if fs
            .inode_cache
            .get(&inode_id)
            .map_or(false, |inode| inode.as_ptr() == self as *const Self)
        {
            fs.inode_cache.remove(&inode_id);
        }
        if !fs.inode_cache.contains_key(&inode_id) && self.link_count(inode_id, &fs) == 0 {
            self.reclaim_inode(inode_id, &mut fs);
            block_cache_sync_all();
        }
    }
}
