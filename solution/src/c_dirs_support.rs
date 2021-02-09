//! File system with directory support
//!
//! Create a filesystem that has a notion of blocks, inodes and directory inodes, by implementing the [`FileSysSupport`], the [`BlockSupport`], the [`InodeSupport`] and the [`DirectorySupport`] traits together (again, all earlier traits are supertraits of the later ones).
//!
//! [`FileSysSupport`]: ../../cplfs_api/fs/trait.FileSysSupport.html
//! [`BlockSupport`]: ../../cplfs_api/fs/trait.BlockSupport.html
//! [`InodeSupport`]: ../../cplfs_api/fs/trait.InodeSupport.html
//! [`DirectorySupport`]: ../../cplfs_api/fs/trait.DirectorySupport.html
//! Make sure this file does not contain any unaddressed `TODO`s anymore when you hand it in.
//!
//! # Status
//!
//! COMPLETED: YES
//!
//! COMMENTS:
//!
//! ...
//!

use crate::a_block_support::{BlockFS, BlockFSError};
use crate::b_inode_support::InodeFSError;
use crate::c_dirs_support::DirFSError::{DirectorySystemError, SearchedDirectoryDoesntExist, InodeNotDirectoryError, DirEntryNameAlreadyExists, InodeNotInUse};
use cplfs_api::controller::Device;
use cplfs_api::error_given::APIError;
use cplfs_api::fs::{BlockSupport, DirectorySupport, FileSysSupport, InodeSupport};
use cplfs_api::types::{Block, DInode, DirEntry, FType, Inode, SuperBlock, DINODE_SIZE, DIRENTRY_SIZE, DIRNAME_SIZE};
use std::path::Path;
use thiserror::Error;

///File system name
pub type FSName = DirFS;

///Main struct file for Directory file system
struct DirFS {
    device: Device,
}

///Main error file for Directory file system
#[derive(Error, Debug)]
pub enum DirFSError {
    ///Errors that deal with the errors caused by Controller error
    #[error("API errors that can occur dealing with Controller layer!")]
    DeviceSystemError(#[from] APIError),

    ///Wrapper error that's going to wrap all Inode errors from previous Inode layer
    #[error("Directory system error.")]
    DirectorySystemError(#[from] InodeFSError),

    ///Error that is thrown when we do a lookup or linkup on a inode that's not an directory
    #[error("Lookup on inode that isn't directory!")]
    InodeNotDirectoryError(),

    ///Error that is thrown when we search for a directory that doesn't exist
    #[error("Search directory doesn't exist!")]
    SearchedDirectoryDoesntExist(),

    ///If we want to create a directory with a name that already exists
    #[error("Directory with a given name already exists!")]
    DirEntryNameAlreadyExists(),

    ///When we want to link the dir entry to inode that's not in use
    #[error("Inode is not in use!")]
    InodeNotInUse(),
}

impl FileSysSupport for DirFS {
    type Error = DirFSError;

    fn sb_valid(sb: &SuperBlock) -> bool {
        return BlockFS::sb_valid(sb);
    }

    fn mkfs<P: AsRef<Path>>(path: P, sb: &SuperBlock) -> Result<Self, Self::Error> {
        if !DirFS::sb_valid(sb) {
            return Err(DirectorySystemError(InodeFSError::InodeSystemError(
                BlockFSError::SuperBlockInvalid(),
            )));
        }

        let mut device = Device::new(&path, sb.block_size, sb.nblocks)?;

        //serializing superblock into block and writing it at the position zero on the device
        let mut super_block = Block::new_zero(0, sb.block_size);
        super_block.serialize_into(sb, 0)?;
        device.write_block(&super_block)?;

        //calculating number of inodes per block and number of inodes blocks that will be required
        //to save those inodes
        let n_inodes_per_block = sb.block_size / (*DINODE_SIZE);
        let n_inodes_blocks = sb.ninodes / n_inodes_per_block;

        //going through all the blocks and all the inodes we have to add into the blocks
        //we loop over number of block we are going to fill with inodes and the number of inodes we are
        //going to write into block.
        for i in 0..n_inodes_blocks + 1 {
            let mut inode_block = Block::new_zero(i + 1, sb.block_size);
            for j in 0..n_inodes_per_block {
                //checking whether the inode we are currently trying to write is outside of the number of the
                //inodes we have to initialize
                if i * n_inodes_per_block + (j + 1) <= sb.ninodes {
                    //initializing the ROOT node if the index is at 1st positon
                    if i == 0 && j == 1 {
                        let mut root_inode = DInode::default();
                        root_inode.ft = FType::TDir;
                        root_inode.nlink = 1;
                        inode_block.serialize_into(&root_inode, *DINODE_SIZE * j)?;
                    } else {
                        inode_block.serialize_into(&DInode::default(), *DINODE_SIZE * j)?;
                    }
                }
            }
            device.write_block(&inode_block)?;
        }

        let rustfs = DirFS { device };

        return Ok(rustfs);
    }

    fn mountfs(dev: Device) -> Result<Self, Self::Error> {
        let block_at_zero: Block = dev.read_block(0).unwrap();
        let superblock = block_at_zero.deserialize_from::<SuperBlock>(0).unwrap();

        //checking whether the superblock in the device is valid
        if !BlockFS::sb_valid(&superblock) {
            return Err(DirectorySystemError(InodeFSError::InodeSystemError(
                BlockFSError::SuperBlockInvalid(),
            )));
        }

        //checking whether the block size in superblock and device are matching
        if !((superblock.block_size == dev.block_size) && (superblock.nblocks == dev.nblocks)) {
            return Err(DirectorySystemError(InodeFSError::InodeSystemError(
                BlockFSError::DeviceConfigurationInvalid(),
            )));
        }

        let rustfs = DirFS { device: dev };
        return Ok(rustfs);
    }

    fn unmountfs(self) -> Device {
        return self.device;
    }
}

impl BlockSupport for DirFS {
    fn b_get(&self, i: u64) -> Result<Block, Self::Error> {
        let block = self.device.read_block(i);

        return if block.is_ok() {
            Ok(block.unwrap())
        } else {

            Err(DirectorySystemError(InodeFSError::InodeSystemError(
                BlockFSError::FileSystemError(block.unwrap_err()),
            )))
        };
    }

    fn b_put(&mut self, b: &Block) -> Result<(), Self::Error> {
        let result = self.device.write_block(b);

        if result.is_err() {
            return Err(DirectorySystemError(InodeFSError::InodeSystemError(
                BlockFSError::FileSystemError(result.unwrap_err()),
            )));
        }

        return Ok(());
    }

    fn b_free(&mut self, i: u64) -> Result<(), Self::Error> {
        let sb: SuperBlock = self.sup_get()?;

        if i >= sb.ndatablocks {
            return Err(DirectorySystemError(InodeFSError::InodeSystemError(
                BlockFSError::OutsideOfTheBoundariesError(),
            )));
        }

        //searching for the block that contains the bit we want to change
        //NOTE: We assume that the first bit starts from 0 to 8*block_size bits, and then the second
        //block starts at 8*block_size+1 - 8*2*block_size.
        let bitmap_block_number = i / (sb.block_size * 8);

        //reading the block from the device and
        //getting the buffer data from the bitmap block
        let mut current_block = self.device.read_block(sb.bmapstart + bitmap_block_number)?;
        let data = current_block.contents_as_ref();

        //we position ourself on the index of the bitmap block we want to check.
        //after we position ourself, we start by finding the corresponding bit we need to free.
        //if we find that the bit is already free (zero), then we throw an error, otherwise
        //we set it to zero.
        let index = (i - (sb.block_size * 8) * bitmap_block_number) / 8;
        let mut changed_data: Vec<u8> = Vec::new();
        for j in 0..data.len() {
            if j == index as usize {
                let k = (i - (sb.block_size * 8) * bitmap_block_number) % 8;

                if data[j] & (1 << k) > 0 {
                    let new_data: u8 = data[j] ^ (1 << k);
                    changed_data.push(new_data);
                } else {
                    return Err(DirectorySystemError(InodeFSError::InodeSystemError(
                        BlockFSError::MemoryAlreadyDeallocated(),
                    )));
                }
            } else {
                changed_data.push(data[j]);
            }
        }

        current_block.write_data(&changed_data, 0)?;
        self.device.write_block(&current_block)?;

        return Ok(());
    }

    fn b_zero(&mut self, i: u64) -> Result<(), Self::Error> {
        let sb: SuperBlock = self.sup_get()?;

        if i >= sb.ndatablocks {
            return Err(DirectorySystemError(InodeFSError::InodeSystemError(
                BlockFSError::OutsideOfTheBoundariesError(),
            )));
        }

        let zero_data_block = Block::new_zero(i + sb.datastart, sb.block_size);
        self.device.write_block(&zero_data_block)?;

        return Ok(());
    }

    fn b_alloc(&mut self) -> Result<u64, Self::Error> {
        let superblock: SuperBlock = self.sup_get()?;
        let bitmap_blocks = superblock.ndatablocks / (superblock.block_size * 8);

        //going through all the bitmap blocks, and then going through all the bytes of data inside those blocks.
        //we are looking for first bit that's zero and then we allocate that block of data.
        for i in 0..bitmap_blocks + 1 {
            let mut current_block = self.device.read_block(superblock.bmapstart + i)?;
            let data = current_block.contents_as_ref();

            let mut changed_data: Vec<u8> = Vec::new();
            let mut counter: u64 = 0;
            let mut is_data_changed: bool = false;
            let mut index: u64 = 0;

            //going through all the bytes of data inside the current block
            for j in 0..data.len() {
                //checking whether the data was changed and whether the data is 255.
                //if the data hasn't been changed and data isn't 255, it means that there is some zero in the byte, and
                //that's the location where we are going to allocate free memory.
                if !is_data_changed && data[j] != 255 {
                    let mut k: u32 = 0;
                    while (data[j] & (1 << k)) > 0 {
                        k += 1;
                    }

                    let bit_mask = u8::pow(2, k);
                    let new_value = data[j] | bit_mask;
                    changed_data.push(new_value);
                    is_data_changed = true;

                    //index of the data block we just initialized
                    index = superblock.datastart + i * superblock.block_size + counter + (k as u64);

                    //if the index is larger than the range for data blocks,
                    //that means that there is an error
                    if index > superblock.ndatablocks + superblock.datastart - 1 {
                        return Err(DirectorySystemError(InodeFSError::InodeSystemError(
                            BlockFSError::OutsideOfTheBoundariesError(),
                        )));
                    }

                    self.b_zero(index%superblock.ndatablocks)?;
                    index = (k + 8*(j as u32)) as u64  + superblock.block_size*8*i;
                    continue;
                }

                //if the that has been changed or the data block is equal to 255, we just pass the
                //unchanged data and increase the counter
                changed_data.push(data[j]);
                counter += 8;
            }

            //At the end we are checking whether we changed anything in the block. If we did, then we
            //have to write that in the block and change it on the device. After that we return the index
            //at which we allocated this new memory.
            if is_data_changed {
                current_block.write_data(&changed_data, 0)?;
                self.device.write_block(&current_block)?;
                return Ok(index);
            }
        }

        return Err(DirectorySystemError(InodeFSError::InodeSystemError(
            BlockFSError::OutsideOfTheBoundariesError(),
        )));
    }

    fn sup_get(&self) -> Result<SuperBlock, Self::Error> {
        let block = self.device.read_block(0);

        return if block.is_ok() {
            let superblock = block.unwrap().deserialize_from::<SuperBlock>(0);

            if superblock.is_ok() {
                Ok(superblock.unwrap())
            } else {
                Err(DirectorySystemError(InodeFSError::InodeSystemError(
                    BlockFSError::FileSystemError(superblock.unwrap_err()),
                )))
            }
        } else {
            Err(DirectorySystemError(InodeFSError::InodeSystemError(
                BlockFSError::FileSystemError(block.unwrap_err()),
            )))
        };
    }

    fn sup_put(&mut self, sup: &SuperBlock) -> Result<(), Self::Error> {
        let mut block: Block = Block::new_zero(0, sup.block_size);

        block.serialize_into(&sup, 0)?;
        self.device.write_block(&block)?;

        return Ok(());
    }
}

impl InodeSupport for DirFS {
    type Inode = Inode;

    fn i_get(&self, i: u64) -> Result<Self::Inode, Self::Error> {
        let sb: SuperBlock = self.sup_get()?;
        if i+1 > sb.ninodes {
            return Err(DirectorySystemError(InodeFSError::InodeSystemError(
                BlockFSError::OutsideOfTheBoundariesError(),
            )));
        }

        //here we are calculating the index of the block that will contain the i-th inode
        let n_inodes_per_block = sb.block_size / (*DINODE_SIZE);
        let index_of_block = i / (n_inodes_per_block);


        //here we calculate the index of the i-th inode inside of the block
        let inode_block = self.b_get(sb.inodestart + index_of_block)?;
        let index_of_inode = i - (n_inodes_per_block * index_of_block);

        //here we deserialize the block into dinode
        let dinode = inode_block
            .deserialize_from::<DInode>(*DINODE_SIZE * index_of_inode)
            .unwrap();

        return Ok(Inode::new(i, dinode));
    }

    fn i_put(&mut self, ino: &Self::Inode) -> Result<(), Self::Error> {
        let sb: SuperBlock = self.sup_get()?;
        if ino.inum + 1 > sb.ninodes {
            return Err(DirectorySystemError(InodeFSError::InodeSystemError(
                BlockFSError::OutsideOfTheBoundariesError(),
            )));
        }

        //here we are calculating the index of the block that will contain the i-th inode
        let n_inodes_per_block = sb.block_size / (*DINODE_SIZE);
        let index_of_block = ino.inum / (n_inodes_per_block);

        //here we calculate the index of the i-th inode inside of the block
        let mut inode_block = self.b_get(sb.inodestart + index_of_block)?;
        let index_of_inode = ino.inum - (n_inodes_per_block * index_of_block);

        //writing the passed dinode into the place where the i-th inode would be
        inode_block.serialize_into(&ino.disk_node, *DINODE_SIZE * index_of_inode)?;
        self.device.write_block(&inode_block)?;

        return Ok(());
    }

    fn i_free(&mut self, i: u64) -> Result<(), Self::Error> {
        let sb = self.sup_get()?;
        if i+1 > sb.ninodes {
            return Err(DirectorySystemError(InodeFSError::InodeSystemError(
                BlockFSError::OutsideOfTheBoundariesError(),
            )));
        }

        let requested_inode = self.i_get(i).unwrap();

        if requested_inode.inum != i {
            return Err(DirectorySystemError(
                InodeFSError::InodeInitializationError(),
            ));
        }

        if requested_inode.disk_node.ft == FType::TFree {
            return Err(DirectorySystemError(
                InodeFSError::InodeAlreadyDeallocatedError(),
            ));
        }

        //checking whether the nlink is equal to zero. if it is, then we are modifying the dinode in the
        //following ways: 1.) we are setting it to TFree 2.) we are going through all direct pointers,
        //and deallocating all the blocks. 3.) setting all direct pointers to zero. 4.) returing the
        //modified inode to the disc.
        if requested_inode.disk_node.nlink == 0 {
            let mut modified_dinode = requested_inode.disk_node;
            modified_dinode.ft = FType::TFree;

            let n_valid_blocks =
                (modified_dinode.size as f64 / sb.block_size as f64).ceil() as usize;
            for i in 0..n_valid_blocks {
                if modified_dinode.direct_blocks[i] != 0 {
                    self.b_free(modified_dinode.direct_blocks[i] % sb.datastart)?;
                }
            }

            modified_dinode.direct_blocks = Default::default();
            self.i_put(&Inode::new(requested_inode.inum, modified_dinode))?;
        }

        return Ok(());
    }

    fn i_alloc(&mut self, ft: FType) -> Result<u64, Self::Error> {
        let sb = self.sup_get().unwrap();

        //calculating number of inodes per block and number of inodes blocks that will be required
        //to save those inodes
        let n_inodes_per_block = sb.block_size / (*DINODE_SIZE);
        let n_inodes_blocks = sb.ninodes / n_inodes_per_block;

        for i in 0..n_inodes_blocks + 1 {
            for j in 0..n_inodes_per_block {
                //checking whether the inode we are currently trying to allocate is outside of the number of the inodes
                if i * n_inodes_per_block + (j + 1) > sb.ninodes {
                    return Err(DirectorySystemError(InodeFSError::InodeSystemError(
                        BlockFSError::OutsideOfTheBoundariesError(),
                    )));
                }

                //to skip the zero-th inode
                if i == 0 && j == 0 {
                    continue;
                }

                let inode = self.i_get(i * n_inodes_per_block + j).unwrap();
                let mut disc_inode = inode.disk_node;
                let index = inode.inum;

                if disc_inode.ft == FType::TFree {
                    disc_inode.ft = ft;
                    disc_inode.size = Default::default();
                    disc_inode.nlink = Default::default();

                    self.i_put(&Inode::new(index, disc_inode))?;
                    return Ok(index);
                }
            }
        }

        return Err(DirectorySystemError(InodeFSError::InodeSystemError(
            BlockFSError::OutsideOfTheBoundariesError(),
        )));
    }

    fn i_trunc(&mut self, inode: &mut Self::Inode) -> Result<(), Self::Error> {
        let sb = self.sup_get().unwrap();

        let n_valid_blocks = (inode.disk_node.size as f64 / sb.block_size as f64).ceil() as usize;
        for i in 0..n_valid_blocks {
            if inode.disk_node.direct_blocks[i] != 0 {
                self.b_free(inode.disk_node.direct_blocks[i] % sb.datastart)?;
            }
        }

        inode.disk_node.size = Default::default();
        inode.disk_node.direct_blocks = Default::default();
        self.i_put(inode)?;
        return Ok(());
    }
}

impl DirectorySupport for DirFS {
    fn new_de(inum: u64, name: &str) -> Option<DirEntry> {
        let mut dir_entry = DirEntry {
            inum,
            name: Default::default(),
        };

        let result = DirFS::set_name_str(&mut dir_entry, name);

        return if result.is_some() {
            Some(dir_entry)
        } else {
            None
        };
    }

    fn get_name_str(de: &DirEntry) -> String {
        let mut name: String = String::new();
        let characters = de.name;

        for i in 0..characters.len() {
            if characters[i] == '\0' {
                break;
            }
            name.push(characters[i]);
        }

        return name;
    }

    fn set_name_str(de: &mut DirEntry, name: &str) -> Option<()> {
        if name.len() == 0 {
            return None;
        }

        if name.len() >= DIRNAME_SIZE {
            return None;
        }

        for c in name.chars() {
            if !c.is_alphanumeric() {
                return None;
            }
        }

        let mut dir_name: [char; DIRNAME_SIZE] = Default::default();
        for i in 0..dir_name.len() {
            dir_name[i] = '\0';
        }

        let mut i = 0;
        for c in name.chars() {
            dir_name[i] = c;
            i += 1;
        }

        de.name = dir_name;
        return Some(());
    }

    fn dirlookup(
        &self,
        inode: &Self::Inode,
        name: &str,
    ) -> Result<(Self::Inode, u64), Self::Error> {
        let sb = self.sup_get()?;

        //checking whether the inode is different type than directory
        if inode.disk_node.ft != FType::TDir {
            return Err(InodeNotDirectoryError());
        }

        //going through all the valid blocks and looking for a inode with a name that was passed as an argument
        let n_valid_blocks = (inode.disk_node.size as f64 / (sb.block_size-(sb.block_size%*DIRENTRY_SIZE)) as f64).ceil() as usize;
        for i in 0..n_valid_blocks {
            if inode.disk_node.direct_blocks[i] == 0 {
                break;
            }

            let current_block = self.b_get(inode.disk_node.direct_blocks[i])?;
            let mut offset = 0;

            //looking through each block and all dir entries to find the one that corresponds to the passed name
            while *DIRENTRY_SIZE + offset < sb.block_size {
                let current_dir_entry: DirEntry = current_block.deserialize_from::<DirEntry>(offset).unwrap();

                //when we find the dir entry we return the inode from that entry and the offset where we found it
                let dir_name = DirFS::get_name_str(&current_dir_entry);
                if dir_name == name {
                    let searched_inode = self.i_get(current_dir_entry.inum)?;
                    return Ok((searched_inode, offset + (i as u64)*(sb.block_size-(sb.block_size%*DIRENTRY_SIZE))));
                }

                offset += *DIRENTRY_SIZE;
            }
        }
        return Err(SearchedDirectoryDoesntExist());
    }

    fn dirlink(
        &mut self,
        inode: &mut Self::Inode,
        name: &str,
        inum: u64,
    ) -> Result<u64, Self::Error> {
        let sb = self.sup_get()?;

        //checking whether the inode is directory
        if inode.disk_node.ft != FType::TDir {
            return Err(InodeNotDirectoryError());
        }

        //checking whether the inode with the given name already exists
        let lookup_results = self.dirlookup(inode, name);
        if lookup_results.is_ok() {
            return Err(DirEntryNameAlreadyExists());
        }

        //checking whether the inode is valid
        let inum_inode = self.i_get(inum);
        if inum_inode.is_err() {
            return Err(DirectorySystemError(InodeFSError::InodeInitializationError()));
        } else {
            //checking whether the inode is free and out of use
            if inum_inode.unwrap().disk_node.ft == FType::TFree {
                return Err(InodeNotInUse());
            }
        }

        //generating dir entry we are going to link
        let dir_entry = DirFS::new_de(inum, name).unwrap();

        //going through all valid blocks and finding the one that has first available space to save new directory entry
        let n_valid_blocks = (inode.disk_node.size as f64 / (sb.block_size-(sb.block_size%*DIRENTRY_SIZE)) as f64).ceil() as usize;
        for i in 0..n_valid_blocks {
            let mut current_block = self.b_get(inode.disk_node.direct_blocks[i]%sb.datastart+sb.datastart)?;
            let mut offset = 0;

            //going through block by increasing offset by the size of the dir entry
            while offset + *DIRENTRY_SIZE <= sb.block_size {
                let current_direntry: DirEntry = current_block.deserialize_from::<DirEntry>(offset).unwrap();

                //when we find free space we save the dir entry into that place
                if current_direntry.inum == 0 {

                    current_block.serialize_into(&dir_entry, offset)?;
                    self.b_put(&current_block)?;

                    //checking whether we need to increase the size of inode
                    //if the inode size is already larger than current offset it means that that was already allocated space and
                    //we do not have to increase the size
                    if inode.disk_node.size < offset + (i as u64)*(sb.block_size-(sb.block_size%*DIRENTRY_SIZE)) + *DIRENTRY_SIZE {
                        inode.disk_node.size += *DIRENTRY_SIZE;
                    }
                    self.i_put(inode)?;

                    //if inode numbers aren't the same, we have to increase the nlink number in other inode
                    if inum != inode.inum {
                        let mut ref_inode = self.i_get(inum).unwrap();
                        ref_inode.disk_node.nlink += 1;
                        self.i_put(&ref_inode)?;
                    }

                    return Ok(offset + (i as u64)*(sb.block_size-(sb.block_size%*DIRENTRY_SIZE)));
                }

                offset += *DIRENTRY_SIZE;
            }
        }

        //if there isn't space in available we have to allocate another block
        let index = self.b_alloc()?;
        let mut allocated_block = self.b_get(index+sb.datastart)?;
        allocated_block.serialize_into(&dir_entry, 0)?;
        self.b_put(&allocated_block)?;

        //adding that allocated block to the list of available blocks
        inode.disk_node.direct_blocks[n_valid_blocks as usize] = index+sb.datastart;
        inode.disk_node.size += *DIRENTRY_SIZE;
        self.i_put(inode)?;

        //if inode numbers aren't the same, we have to increase the nlink number in other inode
        if inum != inode.inum {
            let mut ref_inode = self.i_get(inum).unwrap();
            ref_inode.disk_node.nlink += 1;
            self.i_put(&ref_inode)?;

        }

        Ok(inode.disk_node.size - *DIRENTRY_SIZE)
    }
}


#[cfg(test)]
#[path = "../../api/fs-tests"]
mod test_with_utils {
    use std::path::PathBuf;
    use cplfs_api::fs::{FileSysSupport, InodeSupport, DirectorySupport};
    use cplfs_api::types::{SuperBlock, FType, InodeLike, DIRENTRY_SIZE};
    use crate::c_dirs_support::FSName;

    static BLOCK_SIZE: u64 = 250;
    static NBLOCKS: u64 = 10;
    static SUPERBLOCK_GOOD_MULTIPLE_INODES_BLOCK: SuperBlock = SuperBlock {
        block_size: BLOCK_SIZE,
        nblocks: NBLOCKS,
        ninodes: 6,
        inodestart: 1,
        ndatablocks: 5,
        bmapstart: 4,
        datastart: 5,
    };

    #[path = "utils.rs"]
    mod utils;

    fn disk_prep_path(name: &str) -> PathBuf {
        utils::disk_prep_path(&("fs-images-c-".to_string() + name), "img")
    }

    #[test]
    fn multiblock_allocations(){
        let path = disk_prep_path("mutliblock_lookup");
        let mut my_fs = FSName::mkfs(&path, &SUPERBLOCK_GOOD_MULTIPLE_INODES_BLOCK).unwrap();

        //inode with two blocks free with direntries
        let mut i1 = <<FSName as InodeSupport>::Inode as InodeLike>::new(
            5,
            &FType::TDir,
            0,
            0,
            &[],
        ).unwrap();
        my_fs.i_put(&i1).unwrap();

        //Allocate inodes 2,3,4
        for i in 0..3 {
            assert_eq!(my_fs.i_alloc(FType::TFile).unwrap(), i + 2);
        }

        //filling the dir entries in inode (4 extra blocks are allocated)
        for i in 0..36 {
            assert_eq!(
                my_fs.dirlink(&mut i1, &i.to_string(), 3).unwrap(),
                i * *DIRENTRY_SIZE
            );
        }

        //checking whether did all entires saved correctly
        for i in 0..36{
            assert_eq!(my_fs.dirlookup(&i1,&i.to_string()).unwrap().1, i* *DIRENTRY_SIZE);
        }

        assert_eq!(my_fs.i_get(3).unwrap().disk_node.nlink, 36);
        let dev = my_fs.unmountfs();
        utils::disk_destruct(dev);
    }
}

// WARNING: DO NOT TOUCH THE BELOW CODE -- IT IS REQUIRED FOR TESTING -- YOU WILL LOSE POINTS IF I MANUALLY HAVE TO FIX YOUR TESTS
#[cfg(all(test, any(feature = "c", feature = "all")))]
#[path = "../../api/fs-tests/c_test.rs"]
mod tests;
