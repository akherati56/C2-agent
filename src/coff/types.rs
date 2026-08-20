// src/coff/types.rs

// ---------- Machine ----------

pub const IMAGE_FILE_MACHINE_AMD64: u16 = 0x8664;
pub const IMAGE_FILE_MACHINE_I386: u16 = 0x014C;
pub const IMAGE_FILE_MACHINE_ARM64: u16 = 0xAA64;


// ---------- Section Characteristics ----------

pub const IMAGE_SCN_CNT_CODE: u32 = 0x0000_0020;
pub const IMAGE_SCN_CNT_INITIALIZED_DATA: u32 = 0x0000_0040;
pub const IMAGE_SCN_CNT_UNINITIALIZED_DATA: u32 = 0x0000_0080;

pub const IMAGE_SCN_MEM_EXECUTE: u32 = 0x2000_0000;
pub const IMAGE_SCN_MEM_READ: u32 = 0x4000_0000;
pub const IMAGE_SCN_MEM_WRITE: u32 = 0x8000_0000;


// ---------- AMD64 Relocations ----------

pub const IMAGE_REL_AMD64_ABSOLUTE: u16 = 0x0000;
pub const IMAGE_REL_AMD64_ADDR64: u16 = 0x0001;
pub const IMAGE_REL_AMD64_ADDR32: u16 = 0x0002;
pub const IMAGE_REL_AMD64_ADDR32NB: u16 = 0x0003;
pub const IMAGE_REL_AMD64_REL32: u16 = 0x0004;
pub const IMAGE_REL_AMD64_REL32_1: u16 = 0x0005;
pub const IMAGE_REL_AMD64_REL32_2: u16 = 0x0006;
pub const IMAGE_REL_AMD64_REL32_3: u16 = 0x0007;
pub const IMAGE_REL_AMD64_REL32_4: u16 = 0x0008;
pub const IMAGE_REL_AMD64_REL32_5: u16 = 0x0009;
pub const IMAGE_REL_AMD64_SECTION: u16 = 0x000A;
pub const IMAGE_REL_AMD64_SECREL: u16 = 0x000B;


// ---------- Symbol Storage Classes ----------

pub const IMAGE_SYM_CLASS_EXTERNAL: u8 = 2;
pub const IMAGE_SYM_CLASS_STATIC: u8 = 3;


// ---------- Special Section Numbers ----------

pub const IMAGE_SYM_UNDEFINED: i16 = 0;
pub const IMAGE_SYM_ABSOLUTE: i16 = -1;
pub const IMAGE_SYM_DEBUG: i16 = -2;


// ---------- COFF File Header ----------

#[derive(Debug, Clone, Copy)]
pub struct CoffFileHeader {
    pub machine: u16,
    pub number_of_sections: u16,
    pub time_date_stamp: u32,
    pub ptr_to_symbol_table: u32,
    pub number_of_symbols: u32,
    pub size_of_optional_header: u16,
    pub characteristics: u16,
}


// ---------- COFF Section Header ----------

#[derive(Debug, Clone, Copy)]
pub struct SectionHeader {
    pub name: [u8; 8],
    pub virtual_size: u32,
    pub virtual_address: u32,
    pub size_of_raw_data: u32,
    pub ptr_to_raw_data: u32,
    pub ptr_to_relocations: u32,
    pub ptr_to_line_numbers: u32,
    pub number_of_relocations: u16,
    pub number_of_line_numbers: u16,
    pub characteristics: u32,
}


// ---------- Relocation ----------

#[derive(Debug, Clone, Copy)]
pub struct Relocation {
    pub virtual_address: u32,
    pub symbol_table_index: u32,
    pub relocation_type: u16,
}


// ---------- Symbol Entry ----------

#[derive(Debug, Clone, Copy)]
pub struct SymbolEntry {
    pub name: [u8; 8],
    pub value: u32,
    pub section_number: i16,
    pub symbol_type: u16,
    pub storage_class: u8,
    pub number_of_aux_symbols: u8,
}


// ---------- Symbol ----------

#[derive(Debug, Clone)]
pub struct Symbol {
    pub name: String,
    pub entry: SymbolEntry,
    pub is_aux: bool,
}