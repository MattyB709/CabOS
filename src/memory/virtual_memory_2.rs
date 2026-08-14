use alloc::{boxed::Box, sync::Arc};

use intrusive_collections::{Bound, KeyAdapter, RBTree, RBTreeLink, intrusive_adapter};
use spin::Once;

use crate::{
    arch::{Arch, ArchTrait},
    fs::vfs::{FsError, VNode},
    memory::{
        freeset::FreeSet,
        page_cache::{PAGE_CACHE, PageKey},
        physical_memory,
        virtual_memory::{PageFaultConditions, PagingOptions},
    },
    sync::{IntMutex, MutexLike},
};
pub const USERSPACE_START: usize = 0x10000;
pub const USERSPACE_END: usize = 0x8000_0000_0000_0000;
static LIMINE_PAGE_TABLE: Once<usize> = Once::new();

pub struct Mapping {
    backing: MapBacking,
    length: usize,
    prot: PagingOptions,
    shared: bool,
    base: usize,
    link: RBTreeLink,
}

pub enum MapBacking {
    File(FileMapping),
    Device {
        paddr: usize,
        memory_info: PagingOptions, // device can decide exact memory attributes necessary
    },
    Anonymous,
}

pub struct FileMapping {
    pub vnode: Arc<dyn VNode>,
    pub file_offset: usize,
    pub file_length: Option<usize>,
}

intrusive_adapter!(MappingAdapter = Box<Mapping>: Mapping { link => RBTreeLink });
impl<'a> KeyAdapter<'a> for MappingAdapter {
    type Key = usize;
    fn get_key(&self, x: &'a Mapping) -> usize {
        x.base
    }
}

pub struct VirtualMemory {
    free_set: IntMutex<FreeSet>,
    active_set: IntMutex<RBTree<MappingAdapter>>,
    page_table: usize,
}

impl VirtualMemory {
    pub fn init() {
        let limine_page_table =
            LIMINE_PAGE_TABLE.call_once(|| Arch::get_user_address_space() as usize);
        assert!(limine_page_table.is_multiple_of(Arch::PAGE_SIZE));
    }

    pub fn mmap(
        &self,
        file: Option<FileMapping>,
        length: usize,
        prot: PagingOptions,
        shared: bool,
        preferred_base: Option<usize>,
    ) -> Result<usize, &'static str> {
        if !length.is_multiple_of(Arch::PAGE_SIZE) {
            return Err("map length must be aligned to page boundary");
        }

        let backing = match file {
            Some(fm) => {
                if !fm.file_offset.is_multiple_of(Arch::PAGE_SIZE) {
                    return Err("file offset must be aligned to page boundary");
                }
                match fm.vnode.prepare_mmap(fm.file_offset) {
                    Ok(map_backing) => map_backing,
                    Err(FsError::NotImplemented) => MapBacking::File(fm), // nothing special to do
                    _ => return Err("failed to prepare mmap"), // TODO use this for proper errno handling
                }
            }
            None => MapBacking::Anonymous,
        };

        if let MapBacking::File(fm) = &backing {
            if fm.file_length.is_some() && shared {
                return Err(
                    "we do not allow partial file maps if shared (this feature only affects the ELF loader)",
                );
            }
            if let Some(file_length) = fm.file_length
                && file_length > length
            {
                return Err("file length is bigger than length of map");
            }
        }

        let mut free_set = self.free_set.lock();
        let base = match preferred_base {
            Some(preferred_base) => {
                // note: linux mmap doesn't fail when preferred base in unavailable, for our impl we do
                free_set.remove_range_by_base(preferred_base, length)?;
                preferred_base
            }
            None => free_set.remove_range_by_length(length)?,
        };

        let mapping = Mapping {
            backing,
            length,
            prot,
            shared,
            base,
            link: RBTreeLink::new(),
        };

        let mut active_set = self.active_set.lock();
        active_set.insert(Box::new(mapping));

        Ok(base)
    }

    pub fn new() -> Self {
        let mut set = FreeSet::new();
        set.add_range(USERSPACE_START, USERSPACE_END - USERSPACE_START)
            .unwrap();
        Self {
            free_set: IntMutex::new(set),
            active_set: IntMutex::new(RBTree::new(MappingAdapter::new())),
            page_table: Self::new_page_table(),
        }
    }

    pub fn get_page_table(&self) -> usize {
        self.page_table
    }

    pub fn get_limine_page_table() -> usize {
        *LIMINE_PAGE_TABLE.get().unwrap()
    }
}

impl VirtualMemory {
    fn new_page_table() -> usize {
        let page_table = physical_memory::frame_alloc();
        unsafe {
            physical_memory::copy(Self::get_limine_page_table(), page_table, Arch::PAGE_SIZE)
        };
        page_table
    }
}

impl Default for VirtualMemory {
    fn default() -> Self {
        Self::new()
    }
}

impl VirtualMemory {
    // TODO: Implement per mapping locking (needed for multithreaded
    // processes).

    // TODO: Let's say we have two threads (X, Y) in a process. X
    // faults, and soon Y faults on the same address. X acquires the
    // mapping lock, and Y waits for X. X correctly vmaps such that X
    // and Y are satisfied. X releases lock. Y should now know that it
    // doesn't need to map anything. This hasn't been implemented.
    pub fn handle_page_fault(
        &self,
        cause: PageFaultConditions,
        address: usize,
    ) -> Result<(), &'static str> {
        let vaddr = address & !(Arch::PAGE_SIZE - 1);
        let active_set = self.active_set.lock();
        let mapping = active_set
            .upper_bound(Bound::Included(&vaddr))
            .get()
            .ok_or("nothing is mapped")?;
        if mapping.base + mapping.length <= vaddr {
            return Err("out of mapped range");
        }
        // TODO: This is too broad with how it deals with TLB
        // shootdowns. It does a shootdown, even when we are doing a
        // read->write promotion, which is not needed.
        if cause.contains(PageFaultConditions::PRESENT) {
            // TODO this can cause an infinite loop if the mapping is present but not writable.
            // Need to implement process termination for invalid memory access./
            self.invlpg(vaddr);
        }
        if mapping.shared {
            self.handle_mapping_shared(vaddr, mapping)
        } else {
            self.handle_mapping_private(cause, vaddr, mapping)
        }
    }

    fn handle_mapping_shared(&self, vaddr: usize, mapping: &Mapping) -> Result<(), &'static str> {
        assert!(vaddr.is_multiple_of(Arch::PAGE_SIZE));

        match &mapping.backing {
            MapBacking::File(fm) => {
                let key = PageKey {
                    inode_key: fm
                        .vnode
                        .get_inode_key()
                        .map_err(|_| "could not get inode key")?,
                    offset: vaddr - mapping.base + fm.file_offset,
                };
                let paddr = PAGE_CACHE.lock().get_page(&key)?;
                let permissions = PagingOptions::PRESENT
                    | PagingOptions::CACHEABLE
                    | PagingOptions::USER_ACCESSIBLE
                    | mapping.prot;
                Arch::virtual_map(
                    self.get_page_table() as u64,
                    vaddr as u64,
                    paddr as u64,
                    permissions,
                );
            }
            MapBacking::Device { paddr, memory_info } => {
                // TODO validate length of mapping against device memory size
                let page_offset = vaddr - mapping.base;
                let permissions = PagingOptions::PRESENT
                    | PagingOptions::USER_ACCESSIBLE
                    | mapping.prot
                    | *memory_info;
                Arch::virtual_map(
                    self.get_page_table() as u64,
                    vaddr as u64,
                    (*paddr + page_offset) as u64,
                    permissions,
                );
            }
            MapBacking::Anonymous => {
                return Err("shared anonymous mappings not implemented");
            }
        }
        Ok(())
    }

    fn handle_mapping_private(
        &self,
        _cause: PageFaultConditions, // TODO use cause for COW
        vaddr: usize,
        mapping: &Mapping,
    ) -> Result<(), &'static str> {
        assert!(vaddr.is_multiple_of(Arch::PAGE_SIZE));
        match &mapping.backing {
            MapBacking::File(fm) => {
                if let Some(file_length) = fm.file_length
                    && self.handle_mapping_private_partial(vaddr, mapping, file_length)?
                {
                    return Ok(());
                }
                // TODO use page cache here
                let paddr = physical_memory::frame_alloc();
                let permissions = PagingOptions::PRESENT
                    | PagingOptions::CACHEABLE
                    | PagingOptions::USER_ACCESSIBLE
                    | mapping.prot;
                Arch::virtual_map(
                    self.get_page_table() as u64,
                    vaddr as u64,
                    paddr as u64,
                    permissions,
                );
                fm.vnode
                    .read_page(paddr, vaddr - mapping.base + fm.file_offset)
                    .map_err(|_| "could not read from file")?;
            }
            MapBacking::Device {
                paddr: _,
                memory_info: _,
            } => return Err("Private device mappings are not supported"),
            MapBacking::Anonymous => {
                let paddr = physical_memory::frame_alloc();
                let permissions = PagingOptions::PRESENT
                    | PagingOptions::CACHEABLE
                    | PagingOptions::USER_ACCESSIBLE
                    | mapping.prot;
                Arch::virtual_map(
                    self.get_page_table() as u64,
                    vaddr as u64,
                    paddr as u64,
                    permissions,
                );
                unsafe {
                    core::ptr::write_bytes(
                        (paddr + *physical_memory::HHDM_OFFSET.get().unwrap()) as *mut u8,
                        0,
                        Arch::PAGE_SIZE,
                    );
                }
            }
        }
        Ok(())
    }

    // this method returns either an error, true if the faulting address was in a partial file page,
    // or false otherwise
    fn handle_mapping_private_partial(
        &self,
        vaddr: usize,
        mapping: &Mapping,
        file_length: usize,
    ) -> Result<bool, &'static str> {
        // if the page of the vaddr is fully contained in the file, use the normal faulting path
        if vaddr - mapping.base + Arch::PAGE_SIZE <= file_length {
            return Ok(false);
        }

        let MapBacking::File(fm) = &mapping.backing else {
            return Err("partial file mapping has non-file backing");
        };

        let private_paddr = physical_memory::frame_alloc();
        let page_offset = vaddr - mapping.base;
        let file_bytes = file_length.saturating_sub(page_offset);

        if file_bytes > 0 {
            let file_key = PageKey {
                inode_key: fm
                    .vnode
                    .get_inode_key()
                    .map_err(|_| "could not get inode key")?,
                offset: page_offset + fm.file_offset,
            };
            let cached_paddr = PAGE_CACHE.lock().find_page(&file_key);
            if let Some(file_paddr) = cached_paddr {
                unsafe {
                    physical_memory::copy(file_paddr, private_paddr, file_bytes);
                }
            } else {
                fm.vnode
                    .read_page(private_paddr, file_key.offset)
                    .map_err(|_| "could not read from file")?;
            }
        }

        let hhdm = *physical_memory::HHDM_OFFSET
            .get()
            .ok_or("could not get HHDM offset")?;
        unsafe {
            core::ptr::write_bytes(
                (private_paddr + hhdm + file_bytes) as *mut u8,
                0,
                Arch::PAGE_SIZE - file_bytes,
            );
        }

        let permissions = PagingOptions::PRESENT
            | PagingOptions::CACHEABLE
            | PagingOptions::USER_ACCESSIBLE
            | mapping.prot;
        Arch::virtual_map(
            self.get_page_table() as u64,
            vaddr as u64,
            private_paddr as u64,
            permissions,
        );
        Ok(true)
    }
}

impl VirtualMemory {
    // TODO: MJ said there might be a better way to changing mappings
    // in the future.
    fn invlpg(&self, vaddr: usize) {
        Arch::virtual_unmap_no_dealloc(self.page_table as u64, vaddr as u64);
        Arch::shootdown_tlbs(self.page_table as u64, vaddr, Arch::PAGE_SIZE);
    }
}
