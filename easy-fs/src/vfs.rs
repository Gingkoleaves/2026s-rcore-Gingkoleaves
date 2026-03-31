use crate::BLOCK_SZ;

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
    pub fn new(
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
            if dirent.name() == name {
                return Some(dirent.inode_id() as u32);
            }
        }
        None
    }
    /// Delete entry from name
    fn delete_entry_from_name(&self, name: &str, disk_inode: &mut DiskInode) -> Option<u32> {
        // assert it is a directory
        assert!(disk_inode.is_dir());
        let file_count = (disk_inode.size as usize) / DIRENT_SZ;
        let mut dirent = DirEntry::empty();
        for i in 0..file_count {
            assert_eq!(
                disk_inode.read_at(DIRENT_SZ * i, dirent.as_bytes_mut(), &self.block_device,),
                DIRENT_SZ,
            );
            if dirent.name() == name {
                let mut replace_entry=DirEntry::empty();
                if i!=file_count-1{
                    assert_eq!(
                        disk_inode.read_at(DIRENT_SZ*(file_count-1), replace_entry.as_bytes_mut(), &self.block_device,),
                        DIRENT_SZ,
                    );
                }
                assert_eq!(
                    disk_inode.write_at(DIRENT_SZ * i, replace_entry.as_bytes_mut(), &self.block_device,),
                    DIRENT_SZ,
                );
                disk_inode.size-=DIRENT_SZ as u32;
                return Some(dirent.inode_id())
            }
        }
        None
    }
    /// Find inode under current inode by name
    pub fn find(&self, name: &str) -> Option<Arc<Inode>> {
        let fs = self.fs.lock();
        self.read_disk_inode(|disk_inode| {
            self.find_inode_id(name, disk_inode).map(|inode_id| {
                let (block_id, block_offset) = fs.get_disk_inode_pos(inode_id);
                Arc::new(Self::new(
                    block_id,
                    block_offset,
                    self.fs.clone(),
                    self.block_device.clone(),
                ))
            })
        })
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
        let new_inode_id = fs.alloc_inode();
        // initialize inode
        let (new_inode_block_id, new_inode_block_offset) = fs.get_disk_inode_pos(new_inode_id);
        get_block_cache(new_inode_block_id as usize, Arc::clone(&self.block_device))
            .lock()
            .modify(new_inode_block_offset, |new_inode: &mut DiskInode| {
                new_inode.initialize(DiskInodeType::File);
            });
        self.modify_disk_inode(|root_inode| {
            // append file in the dirent
            let file_count = (root_inode.size as usize) / DIRENT_SZ;
            let new_size = (file_count + 1) * DIRENT_SZ;
            // increase size
            self.increase_size(new_size as u32, root_inode, &mut fs);
            // write dirent
            let dirent = DirEntry::new(name, new_inode_id);
            root_inode.write_at(
                file_count * DIRENT_SZ,
                dirent.as_bytes(),
                &self.block_device,
            );
        });

        let (block_id, block_offset) = fs.get_disk_inode_pos(new_inode_id);
        block_cache_sync_all();
        // return inode
        Some(Arc::new(Self::new(
            block_id,
            block_offset,
            self.fs.clone(),
            self.block_device.clone(),
        )))
        // release efs lock automatically by compiler
    }
    /// Hard link current inode to another name under root directory
    pub fn linkat(&self,name:&str)-> bool{
        let mut fs = self.fs.lock();
        let inode_id=fs.get_disk_inode_id(self.block_id as u32,self.block_offset);
        get_block_cache(self.block_id as usize, Arc::clone(&self.block_device))
            .lock()
            .modify(self.block_offset, |inode:&mut DiskInode|{
                inode.nlink+=1;
            });

        let (root_block_id, root_block_offset) = fs.get_disk_inode_pos(0);
        let res= get_block_cache(root_block_id as usize, Arc::clone(&self.block_device))
            .lock()
            .modify(root_block_offset, |root_node: &mut DiskInode| {
                assert!(root_node.is_dir());

                if self.find_inode_id(name, root_node).is_some(){
                    return false;
                }

                let size=root_node.size;
                let new_size=root_node.size+DIRENT_SZ as u32;
                self.increase_size(new_size, root_node, &mut fs);

                let dirent = DirEntry::new(name, inode_id);
                root_node.write_at(
                    size as usize,
                    dirent.as_bytes(),
                    &self.block_device,
                );
                true
            });
        block_cache_sync_all();
        res
    }
    /// Unlink an entry from root with name
    pub fn unlinkat(&self,name:&str)->bool{
        let mut fs = self.fs.lock();

        let (root_block_id, root_block_offset) = fs.get_disk_inode_pos(0);
        let target_inode_id=get_block_cache(root_block_id as usize, Arc::clone(&self.block_device))
            .lock()
            .modify(root_block_offset, |root_node: &mut DiskInode| {
                assert!(root_node.is_dir());

                // Find name in root directory
                // Replace entry with latest entry
                // Decrease root directory
                self.delete_entry_from_name(name,root_node)
            });
                
        // Decrease target disknode nlink              
        let inode_id = match target_inode_id {
            Some(id) => id,
            None => return false,
        };

        let (target_block,target_offset)=fs.get_disk_inode_pos(inode_id);
        get_block_cache(target_block as usize, Arc::clone(&self.block_device))
            .lock()
            .modify(target_offset, |inode:&mut DiskInode|{
                inode.nlink-=1;
                if inode.nlink==0{
                    let size = inode.size;
                    let data_blocks_dealloc = inode.clear_size(&self.block_device);
                    assert!(data_blocks_dealloc.len() == DiskInode::total_blocks(size) as usize);
                    for data_block in data_blocks_dealloc.into_iter() {
                        fs.dealloc_data(data_block);
                    }
                }
            });
                
        block_cache_sync_all();
        true
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
                v.push(String::from(dirent.name()));
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
    /// get inode info
    pub fn stat(&self)->(u64,u64,Option<bool>,Option<u32>){
        let fs = self.fs.lock();
        let inode = fs.get_disk_inode_id(self.block_id as u32,self.block_offset);
        let dev=get_block_cache(self.block_id as usize, Arc::clone(&self.block_device)).lock().which_device();
        let mut is_dir=None;
        let mut nlink=None;
        self.modify_disk_inode(|disk_inode| {
            is_dir = Some(disk_inode.is_dir());
            nlink=Some(disk_inode.nlink);
        });
        (dev,inode as u64,is_dir,nlink)
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
