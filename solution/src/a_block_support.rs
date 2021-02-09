//! File system with block support
//!
//! Create a filesystem that only has a notion of blocks, by implementing the [`FileSysSupport`] and the [`BlockSupport`] traits together (you have no other choice, as the first one is a supertrait of the second).
//!
//! [`FileSysSupport`]: ../../cplfs_api/fs/trait.FileSysSupport.html
//! [`BlockSupport`]: ../../cplfs_api/fs/trait.BlockSupport.html
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

use crate::a_block_support::BlockFSError::{DeviceConfigurationInvalid, FileSystemError, MemoryAlreadyDeallocated, OutsideOfTheBoundariesError, SuperBlockInvalid};
use cplfs_api::controller::Device;
use cplfs_api::error_given::APIError;
use cplfs_api::fs::{BlockSupport, FileSysSupport};
use cplfs_api::types::{Block, SuperBlock, DINODE_SIZE};
use std::path::Path;
use thiserror::Error;

///File system name
pub type FSName = BlockFS;

///Main structure of the File System object
pub struct BlockFS {
    device: Device,
}
///File System Error
#[derive(Error, Debug)]
pub enum BlockFSError {
    ///Common File System error that is trigger with interaction with controller and device
    #[error("File system error!")]
    FileSystemError(#[from] APIError),

    ///Error that pops up when we are dealing with invalid SuperBlock segment in the File System
    #[error("Superblock is not valid!")]
    SuperBlockInvalid(),

    ///Error that's triggered when the device configuration and superblock that's written on device
    /// in a mismatch.
    #[error("Device configuration mismatched with superblock!")]
    DeviceConfigurationInvalid(),

    ///Error that signals that wanted action is resulting outside of the boundaries of access. E.q.
    /// when we are accessing the block that doesn't exist.
    #[error("Wanted action resulted in outside of the boundaries access!")]
    OutsideOfTheBoundariesError(),

    ///Error that's triggered when we are trying to deallocated the memory that's already deallocated.
    #[error("Memory, at this address, is already deallocated!")]
    MemoryAlreadyDeallocated(),
}

impl FileSysSupport for BlockFS {
    type Error = BlockFSError;

    fn sb_valid(sb: &SuperBlock) -> bool {
        //calculating number of blocks for bitmap part and inodes
        let n_super_blocks = 1;
        let n_bitmap_blocks = (sb.ndatablocks / (sb.block_size * 8)) + 1;
        let n_of_inode_in_block = sb.block_size / (*DINODE_SIZE);
        let n_inode_blocks = if sb.ninodes % n_of_inode_in_block == 0 {
            sb.ninodes / n_of_inode_in_block
        } else {
            sb.ninodes / n_of_inode_in_block + 1
        };

        //checking whether it is physically possible to store all the elements on the disk
        if sb.nblocks < (sb.ndatablocks + n_inode_blocks + n_bitmap_blocks + n_super_blocks) {
            return false;
        }

        //checking whether there is wrong order of elements, i.e whether the elements, on the disk, are overlapping
        return if !(sb.inodestart < sb.bmapstart && sb.bmapstart < sb.datastart) {
            false
        } else if sb.inodestart + n_inode_blocks > sb.bmapstart {
            false
        } else if sb.bmapstart + n_bitmap_blocks > sb.datastart {
            false
        } else if sb.datastart + sb.ndatablocks > sb.nblocks {
            false
        } else {
            true
        };
    }

    fn mkfs<P: AsRef<Path>>(path: P, sb: &SuperBlock) -> Result<Self, Self::Error> {
        if !BlockFS::sb_valid(sb) {
            return Err(SuperBlockInvalid());
        }

        let mut device = Device::new(&path, sb.block_size, sb.nblocks)?;

        //serializing superblock into block and writing it at the position zero on the device
        let mut super_block = Block::new_zero(0, sb.block_size);
        super_block.serialize_into(sb, 0)?;
        device.write_block(&super_block)?;

        //initializing the file system with the device and returning it
        let rushfs = BlockFS { device };
        return Ok(rushfs);
    }

    fn mountfs(dev: Device) -> Result<Self, Self::Error> {
        let block_at_zero: Block = dev.read_block(0).unwrap();
        let superblock = block_at_zero.deserialize_from::<SuperBlock>(0).unwrap();

        //checking whether the superblock in the device is valid
        if !BlockFS::sb_valid(&superblock) {
            return Err(SuperBlockInvalid());
        }

        //checking whether the block size in superblock and device are matching
        if !((superblock.block_size == dev.block_size) && (superblock.nblocks == dev.nblocks)) {
            return Err(DeviceConfigurationInvalid());
        }

        let rustfs = BlockFS { device: dev };
        return Ok(rustfs);
    }

    fn unmountfs(self) -> Device {
        return self.device;
    }
}

impl BlockSupport for BlockFS {
    fn b_get(&self, i: u64) -> Result<Block, Self::Error> {
        let block = self.device.read_block(i);

        return if block.is_ok() {
            Ok(block.unwrap())
        } else {
            Err(FileSystemError(block.unwrap_err()))
        };
    }

    fn b_put(&mut self, b: &Block) -> Result<(), Self::Error> {
        let result = self.device.write_block(b);

        if result.is_err() {
            return Err(FileSystemError(result.unwrap_err()));
        }

        return Ok(());
    }

    fn b_free(&mut self, i: u64) -> Result<(), Self::Error> {
        let sb: SuperBlock = self.sup_get()?;

        if i >= sb.ndatablocks {
            return Err(OutsideOfTheBoundariesError());
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
        // we set it to zero.
        let index = (i - (sb.block_size * 8) * bitmap_block_number) / 8;
        let mut changed_data: Vec<u8> = Vec::new();
        for j in 0..data.len() {
            if j == index as usize {
                let k = (i - (sb.block_size * 8) * bitmap_block_number) % 8;

                if data[j] & (1 << k) > 0 {
                    let new_data: u8 = data[j] ^ (1 << k);
                    changed_data.push(new_data);
                } else {
                    return Err(MemoryAlreadyDeallocated());
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
            return Err(OutsideOfTheBoundariesError());
        }

        let zero_data_block = Block::new_zero(i+sb.datastart, sb.block_size);
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
                        return Err(OutsideOfTheBoundariesError());
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
        return Err(OutsideOfTheBoundariesError());
    }

    fn sup_get(&self) -> Result<SuperBlock, Self::Error> {
        let block = self.device.read_block(0);

        return if block.is_ok() {
            let superblock = block.unwrap().deserialize_from::<SuperBlock>(0);

            if superblock.is_ok() {
                Ok(superblock.unwrap())
            } else {
                Err(FileSystemError(superblock.unwrap_err()))
            }
        } else {
            Err(FileSystemError(block.unwrap_err()))
        };
    }

    fn sup_put(&mut self, sup: &SuperBlock) -> Result<(), Self::Error> {
        let mut block: Block = Block::new_zero(0, sup.block_size);

        block.serialize_into(&sup, 0)?;
        self.device.write_block(&block)?;

        return Ok(());
    }
}

// Here we define a submodule, called `my_tests`, that will contain your unit
// tests for this module.
// You can define more tests in different modules, and change the name of this module
//
// The `test` in the `#[cfg(test)]` annotation ensures that this code is only compiled when we're testing the code.
// To run these tests, run the command `cargo test` in the `solution` directory
//
// To learn more about testing, check the Testing chapter of the Rust
// Book: https://doc.rust-lang.org/book/testing.html
#[cfg(test)]
mod my_tests {
    use crate::a_block_support::BlockFS;
    use cplfs_api::fs::FileSysSupport;
    use cplfs_api::types::SuperBlock;

    /// Testing whether will FileSystem return false for the superblock where the file system regions
    /// are not in the right order. E.q. Inode region is staring after bitmap region.
    #[test]
    fn bad_order_superblock_test() {
        let bad_order_superblock1 = SuperBlock {
            block_size: 1000,
            nblocks: 100,
            ninodes: 10,
            inodestart: 10,
            ndatablocks: 5,
            bmapstart: 1,
            datastart: 5,
        };

        assert_eq!(BlockFS::sb_valid(&bad_order_superblock1), false);

        let bad_order_superblock2 = SuperBlock {
            block_size: 1000,
            nblocks: 100,
            ninodes: 10,
            inodestart: 1,
            ndatablocks: 5,
            bmapstart: 10,
            datastart: 7,
        };

        assert_eq!(BlockFS::sb_valid(&bad_order_superblock2), false);

        let bad_order_superblock3 = SuperBlock {
            block_size: 1000,
            nblocks: 100,
            ninodes: 10,
            inodestart: 5,
            ndatablocks: 5,
            bmapstart: 10,
            datastart: 1,
        };

        assert_eq!(BlockFS::sb_valid(&bad_order_superblock3), false);
    }

    /// Testing FileSystem when superblock regions are overlapping over each other.
    /// E.g. Inode region starts at 1, and bitmap region starts at 5. However, the inode region has a block size
    /// of 6, which means that it will overlap with bitmap region.
    #[test]
    fn bad_region_size_superblock_test() {
        let bad_region_size_superblock1 = SuperBlock {
            block_size: 1000,
            nblocks: 100,
            ninodes: 10,
            inodestart: 1,
            ndatablocks: 20,
            bmapstart: 2,
            datastart: 10,
        };

        assert_eq!(BlockFS::sb_valid(&bad_region_size_superblock1), false);
    }

    /// Testing couple of examples of good superblock definitions.
    #[test]
    fn good_superblock_test() {
        let good_superblock1 = SuperBlock {
            block_size: 1000,
            nblocks: 100,
            ninodes: 10,
            inodestart: 1,
            ndatablocks: 20,
            bmapstart: 6,
            datastart: 7,
        };

        assert_eq!(BlockFS::sb_valid(&good_superblock1), true);

        let good_superblock2 = SuperBlock {
            block_size: 1000,
            nblocks: 10,
            ninodes: 6,
            inodestart: 1,
            ndatablocks: 5,
            bmapstart: 4,
            datastart: 5,
        };

        assert_eq!(BlockFS::sb_valid(&good_superblock2), true);
    }
}

// If you want to write more complicated tests that create actual files on your system, take a look at `utils.rs` in the assignment, and how it is used in the `fs_tests` folder to perform the tests. I have imported it below to show you how it can be used.
// The `utils` folder has a few other useful methods too (nothing too crazy though, you might want to write your own utility functions, or use a testing framework in rust, if you want more advanced features)
#[cfg(test)]
#[path = "../../api/fs-tests"]
mod test_with_utils {
    use std::path::PathBuf;
    use crate::a_block_support::FSName;
    use cplfs_api::fs::{FileSysSupport, BlockSupport};
    use cplfs_api::types::{SuperBlock, Block};

    static BLOCK_SIZE: u64 = 1000;
    static NBLOCKS: u64 = 10;
    static SUPERBLOCK_GOOD: SuperBlock = SuperBlock {
        block_size: BLOCK_SIZE,
        nblocks: NBLOCKS,
        ninodes: 6,
        inodestart: 1,
        ndatablocks: 5,
        bmapstart: 4,
        datastart: 5,
    };

    static SUPERBLOCK_GOOD_BIG: SuperBlock = SuperBlock {
        block_size: BLOCK_SIZE/2,
        nblocks: NBLOCKS*1000,
        ninodes: 10,
        inodestart: 1,
        ndatablocks: 5000,
        bmapstart: 25,
        datastart: 100,
    };

    #[path = "utils.rs"]
    mod utils;

    fn disk_prep_path(name: &str) -> PathBuf {
        utils::disk_prep_path(&("fs-images-a-".to_string() + name), "img")
    }

    #[test]
    fn check_allocated(){
        let path = disk_prep_path("zeroed");
        let mut my_fs = FSName::mkfs(&path, &SUPERBLOCK_GOOD).unwrap();
        let sb: SuperBlock = my_fs.sup_get().unwrap();

        //Allocate
        for i in 0..SUPERBLOCK_GOOD.ndatablocks {
            assert_eq!(my_fs.b_alloc().unwrap(), i); //Fill up all data blocks
        }

        //check if zeroed
        for i in 0..SUPERBLOCK_GOOD.ndatablocks {
            assert_eq!(my_fs.b_get(i+sb.datastart).unwrap(),Block::new_zero(i+sb.datastart,SUPERBLOCK_GOOD.block_size));
        }

        //putting a block into allocated place
        let mut data: [u8; 1] = [1];
        let mut block: Block = Block::new_zero(6,SUPERBLOCK_GOOD.block_size);

        //writing something into a block
        block.write_data(&mut data, 0).unwrap();
        my_fs.b_put(&block).unwrap();
        assert_eq!(my_fs.b_get(6).unwrap(),block);

        //freeing the put block
        my_fs.b_free(6-SUPERBLOCK_GOOD.datastart).unwrap();

        //checking the bitmap
        let mut byte: [u8; 1] = [0];
        let bb = my_fs.b_get(4).unwrap();
        bb.read_data(&mut byte, 0).unwrap();
        assert_eq!(byte[0], 0b0001_1101);

        //allocating again
        assert_eq!(my_fs.b_alloc().unwrap(),6-SUPERBLOCK_GOOD.datastart);

        //checking the bit block (again)
        let mut byte: [u8; 1] = [0];
        let bb = my_fs.b_get(4).unwrap();
        bb.read_data(&mut byte, 0).unwrap();
        assert_eq!(byte[0], 0b0001_1111);

        let dev = my_fs.unmountfs();
        utils::disk_destruct(dev);
    }

    #[test]
    fn big_superblock_test(){
        let path = disk_prep_path("big_superblock");
        let mut my_fs = FSName::mkfs(&path, &SUPERBLOCK_GOOD_BIG).unwrap();
        let _sb: SuperBlock = my_fs.sup_get().unwrap();

        //Allocate
        for i in 0..SUPERBLOCK_GOOD_BIG.ndatablocks {
            assert_eq!(my_fs.b_alloc().unwrap(), i); //Fill up all data blocks
        }

        //check for the bitmap block #1 (block size is 500)
        let mut byte: [u8; 500] = [0; 500];
        let bb = my_fs.b_get(25).unwrap();
        bb.read_data(&mut byte, 0).unwrap();

        //checking if all blocks are allocated (if all bits are set to one in first block)
        for i in 0..byte.len(){
            assert_eq!(byte[i], 0b1111_1111);
        }

        //check for the bitmap block #2 (125 is the number of defined bits bytes in second bitmap block)
        let mut byte: [u8; 126] = [0; 126];
        let bb = my_fs.b_get(26).unwrap();
        bb.read_data(&mut byte, 0).unwrap();

        //checking if all block are allocated
        for i in 0..byte.len()-1{
            assert_eq!(byte[i], 0b1111_1111);
        }

        //checking the first next byte in bitmap (it has to be empty)
        assert_eq!(byte[125],0b0000_0000);

        let dev = my_fs.unmountfs();
        utils::disk_destruct(dev);
    }
}

// Here we define a submodule, called `tests`, that will contain our unit tests
// Take a look at the specified path to figure out which tests your code has to pass.
// As with all other files in the assignment, the testing module for this file is stored in the API crate (this is the reason for the 'path' attribute in the code below)
// The reason I set it up like this is that it allows me to easily add additional tests when grading your projects, without changing any of your files, but you can still run my tests together with yours by specifying the right features (see below) :)
// directory.
//
// To run these tests, run the command `cargo test --features="X"` in the `solution` directory, with "X" a space-separated string of the features you are interested in testing.
//
// WARNING: DO NOT TOUCH THE BELOW CODE -- IT IS REQUIRED FOR TESTING -- YOU WILL LOSE POINTS IF I MANUALLY HAVE TO FIX YOUR TESTS
//The below configuration tag specifies the following things:
// 'cfg' ensures this module is only included in the source if all conditions are met
// 'all' is true iff ALL conditions in the tuple hold
// 'test' is only true when running 'cargo test', not 'cargo build'
// 'any' is true iff SOME condition in the tuple holds
// 'feature = X' ensures that the code is only compiled when the cargo command includes the flag '--features "<some-features>"' and some features includes X.
// I declared the necessary features in Cargo.toml
// (Hint: this hacking using features is not idiomatic behavior, but it allows you to run your own tests without getting errors on mine, for parts that have not been implemented yet)
// The reason for this setup is that you can opt-in to tests, rather than getting errors at compilation time if you have not implemented something.
// The "a" feature will run these tests specifically, and the "all" feature will run all tests.
#[cfg(all(test, any(feature = "a", feature = "all")))]
#[path = "../../api/fs-tests/a_test.rs"]
mod tests;
