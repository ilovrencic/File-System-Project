//! File system with inode support + read and write operations on inodes
//!
//! Create a filesystem that has a notion of inodes and blocks, by implementing the [`FileSysSupport`], the [`BlockSupport`] and the [`InodeSupport`] traits together (again, all earlier traits are supertraits of the later ones).
//! Additionally, implement the [`InodeRWSupport`] trait to provide operations to read from and write to inodes
//!
//! [`FileSysSupport`]: ../../cplfs_api/fs/trait.FileSysSupport.html
//! [`BlockSupport`]: ../../cplfs_api/fs/trait.BlockSupport.html
//! [`InodeSupport`]: ../../cplfs_api/fs/trait.InodeSupport.html
//! [`InodeRWSupport`]: ../../cplfs_api/fs/trait.InodeRWSupport.html
//! Make sure this file does not contain any unaddressed `TODO`s anymore when you hand it in.
//!
//! # Status
//!
//! **TODO**: Replace the question mark below with YES, NO, or PARTIAL to
//! indicate the status of this assignment. If you want to tell something
//! about this assignment to the grader, e.g., you have a bug you can't fix,
//! or you want to explain your approach, write it down after the comments
//! section. If you had no major issues and everything works, there is no need to write any comments.
//!
//! COMPLETED: PARTIAL
//!
//! COMMENTS:
//!
//! ...
//!

use thiserror::Error;
use cplfs_api::controller::Device;
use cplfs_api::error_given::APIError;
use crate::b_inode_support::InodeFSError;
use cplfs_api::fs::{FileSysSupport, BlockSupport, InodeSupport, InodeRWSupport};
use cplfs_api::types::{SuperBlock, Block, DInode, Inode, FType, DINODE_SIZE, Buffer};
use crate::a_block_support::{BlockFS, BlockFSError};
use std::path::Path;
use crate::e_inode_RW_support::RWInodeFSError::{InodeRWSystemError, OffsetOutsideOfInode};
use crate::b_inode_support::InodeFSError::{InodeSystemError, InodeInitializationError, InodeAlreadyDeallocatedError};
use crate::a_block_support::BlockFSError::OutsideOfTheBoundariesError;

///File system name
pub type FSName = RWInodeFS;

///Main struct file for the InodeRW File System
pub struct RWInodeFS {
    device: Device,
}

///Main error file for InodeRW File system
#[derive(Error, Debug)]
pub enum RWInodeFSError {
    ///Errors that deal with the errors caused by Controller error
    #[error("API errors that can occur dealing with Controller layer!")]
    DeviceSystemError(#[from] APIError),

    ///Common File System error that is trigger with interaction with controller and device
    #[error("File system error!")]
    InodeRWSystemError(#[from] InodeFSError),

    ///todo: finishi this
    #[error("Not allowed initialization of Inode!")]
    RandomError(),

    ///todo:finish this
    #[error("Reading outside of the inode")]
    OffsetOutsideOfInode(),
}

impl FileSysSupport for RWInodeFS {
    type Error = RWInodeFSError;

    fn sb_valid(sb: &SuperBlock) -> bool {
        return BlockFS::sb_valid(sb);
    }

    fn mkfs<P: AsRef<Path>>(path: P, sb: &SuperBlock) -> Result<Self, Self::Error> {
        if !RWInodeFS::sb_valid(sb) {
            return Err(InodeRWSystemError(InodeSystemError(BlockFSError::SuperBlockInvalid())));
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

        let rustfs = RWInodeFS { device };

        return Ok(rustfs);
    }

    fn mountfs(dev: Device) -> Result<Self, Self::Error> {
        let block_at_zero: Block = dev.read_block(0).unwrap();
        let superblock = block_at_zero.deserialize_from::<SuperBlock>(0).unwrap();

        //checking whether the superblock in the device is valid
        if !BlockFS::sb_valid(&superblock) {
            return Err(InodeRWSystemError(InodeSystemError(BlockFSError::SuperBlockInvalid())));
        }

        //checking whether the block size in superblock and device are matching
        if !((superblock.block_size == dev.block_size) && (superblock.nblocks == dev.nblocks)) {
            return Err(InodeRWSystemError(InodeSystemError(BlockFSError::DeviceConfigurationInvalid())));
        }

        let rustfs = RWInodeFS { device: dev };
        return Ok(rustfs);
    }

    fn unmountfs(self) -> Device {
        return self.device;
    }
}

impl BlockSupport for RWInodeFS {
    fn b_get(&self, i: u64) -> Result<Block, Self::Error> {
        let block = self.device.read_block(i);

        return if block.is_ok() {
            Ok(block.unwrap())
        } else {
            Err(InodeRWSystemError(InodeSystemError(BlockFSError::FileSystemError(
                block.unwrap_err(),
            ))))
        };
    }

    fn b_put(&mut self, b: &Block) -> Result<(), Self::Error> {
        let result = self.device.write_block(b);

        if result.is_err() {
            return Err(InodeRWSystemError(InodeSystemError(BlockFSError::FileSystemError(
                result.unwrap_err(),
            ))));
        }

        return Ok(());
    }

    fn b_free(&mut self, i: u64) -> Result<(), Self::Error> {
        let sb: SuperBlock = self.sup_get()?;

        if i >= sb.ndatablocks {
            return Err(InodeRWSystemError(InodeSystemError(BlockFSError::OutsideOfTheBoundariesError())));
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
                    return Err(InodeRWSystemError(InodeSystemError(BlockFSError::MemoryAlreadyDeallocated())));
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
            return Err(InodeRWSystemError(InodeSystemError(BlockFSError::OutsideOfTheBoundariesError())));
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
                        return Err(InodeRWSystemError(InodeSystemError(BlockFSError::OutsideOfTheBoundariesError())));
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

        return Err(InodeRWSystemError(InodeSystemError(BlockFSError::OutsideOfTheBoundariesError())));
    }

    fn sup_get(&self) -> Result<SuperBlock, Self::Error> {
        let block = self.device.read_block(0);

        return if block.is_ok() {
            let superblock = block.unwrap().deserialize_from::<SuperBlock>(0);

            if superblock.is_ok() {
                Ok(superblock.unwrap())
            } else {
                Err(InodeRWSystemError(InodeSystemError(BlockFSError::FileSystemError(
                    superblock.unwrap_err(),
                ))))
            }
        } else {
            Err(InodeRWSystemError(InodeSystemError(BlockFSError::FileSystemError(
                block.unwrap_err(),
            ))))
        };
    }

    fn sup_put(&mut self, sup: &SuperBlock) -> Result<(), Self::Error> {
        let mut block: Block = Block::new_zero(0, sup.block_size);

        block.serialize_into(&sup, 0)?;
        self.device.write_block(&block)?;

        return Ok(());
    }
}

impl InodeSupport for RWInodeFS{
    type Inode = Inode;

    fn i_get(&self, i: u64) -> Result<Self::Inode, Self::Error> {
        let sb: SuperBlock = self.sup_get()?;
        if i > sb.ninodes - 1 {
            return Err(InodeRWSystemError(InodeSystemError(OutsideOfTheBoundariesError())));
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
            return Err(InodeRWSystemError(InodeSystemError(OutsideOfTheBoundariesError())));
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
            return Err(InodeRWSystemError(InodeSystemError(OutsideOfTheBoundariesError())));
        }

        let requested_inode = self.i_get(i).unwrap();

        if requested_inode.inum != i {
            return Err(InodeRWSystemError(InodeInitializationError()));
        }

        if requested_inode.disk_node.ft == FType::TFree {
            return Err(InodeRWSystemError(InodeAlreadyDeallocatedError()));
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
                    return Err(InodeRWSystemError(InodeSystemError(OutsideOfTheBoundariesError())));
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

        return Err(InodeRWSystemError(InodeSystemError(BlockFSError::OutsideOfTheBoundariesError())));
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

impl InodeRWSupport for RWInodeFS {

    fn i_read(&self, inode: &Self::Inode, buf: &mut Buffer, off: u64, n: u64) -> Result<u64, Self::Error> {
        let sb: SuperBlock = self.sup_get()?;

        //return error if we start reading more than there is saved in inode
        if inode.disk_node.size < off {
            return Err(OffsetOutsideOfInode());
        }

        //if we read from inode.get_size() we return 0
        if off == inode.disk_node.size {
            return Ok(0);
        }

        //block and offset from which we start to read
        let starting_block = off/sb.block_size;
        let starting_offset = off%sb.block_size;

        //creating the buff vector in which we will add all the slices we read
        let mut buff_vector: Vec<u8> = Vec::new();
        let mut current_n = n;

        //going through all blocks we can read
        let n_valid_blocks = (inode.disk_node.size as f64 / sb.block_size as f64).ceil() as usize;
        for i in starting_block as usize..n_valid_blocks {
            let mut offset = 0;
            let current_block = self.b_get(inode.disk_node.direct_blocks[i]%sb.datastart+sb.datastart)?;

            if i as u64 == starting_block{
                offset = starting_offset;
            }

            //reading the data from offset to the end of the block
            let mut data: Box<[u8]>;
            if sb.block_size-offset < current_n{
                data = vec![0 as u8; (sb.block_size - offset) as usize].into_boxed_slice();
                current_block.read_data(&mut data,offset)?;
                current_n -= sb.block_size-offset;
            } else {
                data = vec![0 as u8; current_n as usize].into_boxed_slice();
                current_block.read_data(&mut data,offset)?;
                current_n = 0;
            }

            //appending that data to the vector
            let mut data_vector = data.to_vec();
            buff_vector.append(&mut data_vector);

            //if we have read all the bytes, we can stop with the function
            if current_n == 0 {
                buf.write_data(&buff_vector,0)?;
                return Ok(n);
            }
        }

        buf.write_data(&buff_vector,0)?;
        return Ok(n-current_n);
    }

    fn i_write(&mut self, inode: &mut Self::Inode, buf: &Buffer, off: u64, n: u64) -> Result<(), Self::Error> {
        let sb: SuperBlock = self.sup_get()?;

        //return error if we start reading more than there is saved in inode
        if inode.disk_node.size < off {
            return Err(OffsetOutsideOfInode());
        }

        //block and offset from which we start to read
        let starting_block = off/sb.block_size;
        let starting_offset = off%sb.block_size;

        let mut current_n = n;
        let mut starting_point = 0;

        //going through all blocks we can read
        let n_valid_blocks = (inode.disk_node.size as f64 / sb.block_size as f64).ceil() as usize;

        for i in starting_block as usize..n_valid_blocks {
            let mut offset = 0;
            let mut current_block = self.b_get(inode.disk_node.direct_blocks[i]%sb.datastart+sb.datastart)?;

            if i as u64 == starting_block{
                offset = starting_offset;
            }

            if sb.block_size-offset < current_n{
                let current_data = &buf.contents_as_ref()[starting_point as usize..(sb.block_size-offset) as usize];
                current_block.write_data(&current_data, offset)?;
                starting_point += sb.block_size-offset;
                current_n -= sb.block_size-offset;

            } else {
                let current_data = &buf.contents_as_ref()[starting_point as usize..n as usize];
                current_block.write_data(&current_data, offset)?;
                starting_point = n;
                current_n -= 0;
            }


            self.b_put(&current_block)?;

            if current_n == 0{

                return Ok(());
            }
        }

        let index = self.b_alloc()?;
        let mut allocated_block = self.b_get(index+sb.datastart)?;
        let current_data = &buf.contents_as_ref()[starting_point as usize..n as usize];
        allocated_block.write_data(&current_data,0)?;
        self.b_put(&allocated_block)?;

        //adding that allocated block to the list of available blocks
        inode.disk_node.direct_blocks[n_valid_blocks as usize] = index+sb.datastart;
        inode.disk_node.size += current_data.len() as u64;
        self.i_put(inode)?;

        return Ok(());
    }
}


// WARNING: DO NOT TOUCH THE BELOW CODE -- IT IS REQUIRED FOR TESTING -- YOU WILL LOSE POINTS IF I MANUALLY HAVE TO FIX YOUR TESTS
#[cfg(all(test, any(feature = "e", feature = "all")))]
#[path = "../../api/fs-tests/e_test.rs"]
mod tests;
