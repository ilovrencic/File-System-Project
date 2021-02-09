//! File system with inode support
//!
//! Create a filesystem that has a notion of inodes and blocks, by implementing the [`FileSysSupport`], the [`BlockSupport`] and the [`InodeSupport`] traits together (again, all earlier traits are supertraits of the later ones).
//!
//! [`FileSysSupport`]: ../../cplfs_api/fs/trait.FileSysSupport.html
//! [`BlockSupport`]: ../../cplfs_api/fs/trait.BlockSupport.html
//! [`InodeSupport`]: ../../cplfs_api/fs/trait.InodeSupport.html
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

use crate::a_block_support::BlockFSError::OutsideOfTheBoundariesError;
use crate::a_block_support::{BlockFS, BlockFSError};
use crate::b_inode_support::InodeFSError::{
    InodeAlreadyDeallocatedError, InodeInitializationError, InodeSystemError,
};
use cplfs_api::controller::Device;
use cplfs_api::error_given::APIError;
use cplfs_api::fs::{BlockSupport, FileSysSupport, InodeSupport};
use cplfs_api::types::{Block, DInode, FType, Inode, SuperBlock, DINODE_SIZE};
use std::path::Path;
use thiserror::Error;

///File system name
pub type FSName = InodeFS;

///Main struct file for the Inode File System
pub struct InodeFS {
    device: Device,
}

///Main error file for Inode File system
#[derive(Error, Debug)]
pub enum InodeFSError {
    ///Error type that represents all errors that came from handling the Device
    #[error("API errors that can occur dealing with Device")]
    DeviceSystemError(#[from] APIError),

    ///Common File System error that is trigger with interaction with controller and device
    #[error("File system error!")]
    InodeSystemError(#[from] BlockFSError),

    ///Error that's thrown when we are initializing the Inode that's not valid
    #[error("Not allowed initialization of Inode!")]
    InodeInitializationError(),

    ///Error that's thrown when we are deallocating a inode that's already free
    #[error("Inode already deallocated")]
    InodeAlreadyDeallocatedError(),
}

impl FileSysSupport for InodeFS {
    type Error = InodeFSError;

    fn sb_valid(sb: &SuperBlock) -> bool {
        return BlockFS::sb_valid(sb);
    }

    fn mkfs<P: AsRef<Path>>(path: P, sb: &SuperBlock) -> Result<Self, Self::Error> {
        if !InodeFS::sb_valid(sb) {
            return Err(InodeSystemError(BlockFSError::SuperBlockInvalid()));
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
                    inode_block.serialize_into(&DInode::default(), *DINODE_SIZE * j)?;
                }
            }
            device.write_block(&inode_block)?;
        }

        let rustfs = InodeFS { device };

        return Ok(rustfs);
    }

    fn mountfs(dev: Device) -> Result<Self, Self::Error> {
        let block_at_zero: Block = dev.read_block(0).unwrap();
        let superblock = block_at_zero.deserialize_from::<SuperBlock>(0).unwrap();

        //checking whether the superblock in the device is valid
        if !BlockFS::sb_valid(&superblock) {
            return Err(InodeSystemError(BlockFSError::SuperBlockInvalid()));
        }

        //checking whether the block size in superblock and device are matching
        if !((superblock.block_size == dev.block_size) && (superblock.nblocks == dev.nblocks)) {
            return Err(InodeSystemError(BlockFSError::DeviceConfigurationInvalid()));
        }

        let rustfs = InodeFS { device: dev };
        return Ok(rustfs);
    }

    fn unmountfs(self) -> Device {
        return self.device;
    }
}

impl BlockSupport for InodeFS {
    fn b_get(&self, i: u64) -> Result<Block, Self::Error> {
        let block = self.device.read_block(i);

        return if block.is_ok() {
            Ok(block.unwrap())
        } else {
            Err(InodeSystemError(BlockFSError::FileSystemError(
                block.unwrap_err(),
            )))
        };
    }

    fn b_put(&mut self, b: &Block) -> Result<(), Self::Error> {
        let result = self.device.write_block(b);

        if result.is_err() {
            return Err(InodeSystemError(BlockFSError::FileSystemError(
                result.unwrap_err(),
            )));
        }

        return Ok(());
    }

    fn b_free(&mut self, i: u64) -> Result<(), Self::Error> {
        let sb: SuperBlock = self.sup_get()?;

        if i >= sb.ndatablocks {
            return Err(InodeSystemError(BlockFSError::OutsideOfTheBoundariesError()));
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
                    return Err(InodeSystemError(BlockFSError::MemoryAlreadyDeallocated()));
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
            return Err(InodeSystemError(BlockFSError::OutsideOfTheBoundariesError()));
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
                        return Err(InodeSystemError(BlockFSError::OutsideOfTheBoundariesError()));
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

        return Err(InodeSystemError(BlockFSError::OutsideOfTheBoundariesError()));
    }

    fn sup_get(&self) -> Result<SuperBlock, Self::Error> {
        let block = self.device.read_block(0);

        return if block.is_ok() {
            let superblock = block.unwrap().deserialize_from::<SuperBlock>(0);

            if superblock.is_ok() {
                Ok(superblock.unwrap())
            } else {
                Err(InodeSystemError(BlockFSError::FileSystemError(
                    superblock.unwrap_err(),
                )))
            }
        } else {
            Err(InodeSystemError(BlockFSError::FileSystemError(
                block.unwrap_err(),
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

impl InodeSupport for InodeFS {
    type Inode = Inode;

    fn i_get(&self, i: u64) -> Result<Self::Inode, Self::Error> {
        let sb: SuperBlock = self.sup_get()?;
        if i > sb.ninodes - 1 {
            return Err(InodeSystemError(OutsideOfTheBoundariesError()));
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
        if ino.inum > sb.ninodes - 1 {
            return Err(InodeSystemError(OutsideOfTheBoundariesError()));
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
        if i > sb.ninodes - 1 {
            return Err(InodeSystemError(OutsideOfTheBoundariesError()));
        }

        let requested_inode = self.i_get(i).unwrap();

        if requested_inode.inum != i {
            return Err(InodeInitializationError());
        }

        if requested_inode.disk_node.ft == FType::TFree {
            return Err(InodeAlreadyDeallocatedError());
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
                    return Err(InodeSystemError(OutsideOfTheBoundariesError()));
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

        return Err(InodeSystemError(BlockFSError::OutsideOfTheBoundariesError()));
    }

    fn i_trunc(&mut self, inode: &mut Self::Inode) -> Result<(), Self::Error> {
        let sb = self.sup_get().unwrap();

        //going through all valid blocks and freeing them
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

#[cfg(test)]
#[path = "../../api/fs-tests"]
mod test_with_utils {
    use std::path::PathBuf;
    use crate::b_inode_support::FSName;
    use cplfs_api::fs::{FileSysSupport, InodeSupport};
    use cplfs_api::types::{SuperBlock, FType, InodeLike};

    static BLOCK_SIZE: u64 = 300;
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
        utils::disk_prep_path(&("fs-images-b-".to_string() + name), "img")
    }

    #[test]
    fn multiple_blocks_test(){
        let path = disk_prep_path("multiple_blocks");
        let mut my_fs = FSName::mkfs(&path, &SUPERBLOCK_GOOD_MULTIPLE_INODES_BLOCK).unwrap();

        //Allocate inodes
        for i in 0..(SUPERBLOCK_GOOD_MULTIPLE_INODES_BLOCK.ninodes - 1) {
            assert_eq!(my_fs.i_alloc(FType::TFile).unwrap(), i + 1);
        }

        //inodes
        let i1 = <<FSName as InodeSupport>::Inode as InodeLike>::new(
            1,
            &FType::TFile,
            1,
            (2.5 * (BLOCK_SIZE as f32)) as u64,
            &[2, 3, 4],
        ).unwrap();
        let i2 = <<FSName as InodeSupport>::Inode as InodeLike>::new(
            2,
            &FType::TFile,
            1,
            (2.5 * (BLOCK_SIZE as f32)) as u64,
            &[2, 3, 4],
        ).unwrap();
        let i3 = <<FSName as InodeSupport>::Inode as InodeLike>::new(
            3,
            &FType::TFile,
            1,
            (2.5 * (BLOCK_SIZE as f32)) as u64,
            &[2, 3, 4],
        ).unwrap();
        let i4 = <<FSName as InodeSupport>::Inode as InodeLike>::new(
            4,
            &FType::TFile,
            1,
            (2.5 * (BLOCK_SIZE as f32)) as u64,
            &[2, 3, 4],
        ).unwrap();

        //Putting inodes into allocated inodes
        my_fs.i_put(&i1).unwrap();
        my_fs.i_put(&i2).unwrap();
        my_fs.i_put(&i3).unwrap();
        my_fs.i_put(&i4).unwrap();

        //checking whether the nodes are intact
        assert_eq!(my_fs.i_get(1).unwrap(),i1);
        assert_eq!(my_fs.i_get(2).unwrap(),i2);
        assert_eq!(my_fs.i_get(3).unwrap(),i3);
        assert_eq!(my_fs.i_get(4).unwrap(),i4);

        let dev = my_fs.unmountfs();
        utils::disk_destruct(dev);
    }

}

// WARNING: DO NOT TOUCH THE BELOW CODE -- IT IS REQUIRED FOR TESTING -- YOU WILL LOSE POINTS IF I MANUALLY HAVE TO FIX YOUR TESTS
#[cfg(all(test, any(feature = "b", feature = "all")))]
#[path = "../../api/fs-tests/b_test.rs"]
mod tests;
