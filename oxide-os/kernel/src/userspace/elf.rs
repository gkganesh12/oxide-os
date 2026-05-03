use crate::println;

const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];

#[repr(C)]
#[derive(Debug)]
pub struct ElfHeader {
    pub e_ident: [u8; 16],
    pub e_type: u16,
    pub e_machine: u16,
    pub e_version: u32,
    pub e_entry: u64,
    pub e_phoff: u64,
    pub e_shoff: u64,
    pub e_flags: u32,
    pub e_ehsize: u16,
    pub e_phentsize: u16,
    pub e_phnum: u16,
    pub e_shentsize: u16,
    pub e_shnum: u16,
    pub e_shstrndx: u16,
}

#[repr(C)]
#[derive(Debug)]
pub struct ProgramHeader {
    pub p_type: u32,
    pub p_flags: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

pub const PT_LOAD: u32 = 1;

/// Validate an ELF binary. Returns the entry point address.
pub fn validate(binary: &[u8]) -> Result<u64, &'static str> {
    if binary.len() < core::mem::size_of::<ElfHeader>() {
        return Err("binary too small");
    }
    let header = unsafe { &*(binary.as_ptr() as *const ElfHeader) };
    if header.e_ident[..4] != ELF_MAGIC { return Err("not a valid ELF"); }
    if header.e_ident[4] != 2 { return Err("not 64-bit"); }
    if header.e_machine != 0x3E { return Err("not x86_64"); }

    println!("[elf] Valid ELF: entry={:#X}, {} program headers", header.e_entry, header.e_phnum);
    Ok(header.e_entry)
}

/// Count loadable segments.
pub fn count_load_segments(binary: &[u8]) -> usize {
    let header = unsafe { &*(binary.as_ptr() as *const ElfHeader) };
    let mut count = 0;
    for i in 0..header.e_phnum {
        let offset = header.e_phoff as usize + (i as usize * header.e_phentsize as usize);
        if offset + core::mem::size_of::<ProgramHeader>() > binary.len() { break; }
        let ph = unsafe { &*(binary.as_ptr().add(offset) as *const ProgramHeader) };
        if ph.p_type == PT_LOAD { count += 1; }
    }
    count
}
