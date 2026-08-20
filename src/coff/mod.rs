// src/coff/mod.rs

pub mod types;
pub mod reloc;
// mod exec;

use std::fmt;
pub use types::*;
pub use reloc::*;

// ---------- ساختارهای خروجی ----------
#[derive(Debug)]
pub struct CoffFile {
    pub header: CoffFileHeader,
    pub sections: Vec<Section>,
    pub symbols: Vec<Symbol>,
    pub string_table: Vec<u8>,
}

#[derive(Debug)]
pub struct Section {
    pub header: SectionHeader,
    pub data: Vec<u8>,
    pub relocations: Vec<Relocation>,
}

// ---------- خطا ----------
#[derive(Debug)]
pub enum CoffError {
    TooSmall,
    InvalidOffset,
    UnsupportedMachine(u16),
}

impl fmt::Display for CoffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoffError::TooSmall => write!(f, "file too small to be a COFF"),
            CoffError::InvalidOffset => write!(f, "offset out of bounds"),
            CoffError::UnsupportedMachine(m) => write!(f, "unsupported machine: 0x{m:04X}"),
        }
    }
}

impl std::error::Error for CoffError {}

// ---------- توابع کمکی خواندن ----------
fn read_u16(data: &[u8], off: usize) -> Result<u16, CoffError> {
    let b: [u8; 2] = data.get(off..off + 2).ok_or(CoffError::InvalidOffset)?.try_into().unwrap();
    Ok(u16::from_le_bytes(b))
}

fn read_u32(data: &[u8], off: usize) -> Result<u32, CoffError> {
    let b: [u8; 4] = data.get(off..off + 4).ok_or(CoffError::InvalidOffset)?.try_into().unwrap();
    Ok(u32::from_le_bytes(b))
}

fn read_range(data: &[u8], start: usize, len: usize) -> Result<&[u8], CoffError> {
    let end = start.checked_add(len).ok_or(CoffError::InvalidOffset)?;
    data.get(start..end).ok_or(CoffError::InvalidOffset)
}

fn read_arr8(data: &[u8], off: usize) -> Result<[u8; 8], CoffError> {
    let b: [u8; 8] = data.get(off..off + 8).ok_or(CoffError::InvalidOffset)?.try_into().unwrap();
    Ok(b)
}

// ---------- پارس اجزا ----------
fn parse_header(data: &[u8]) -> Result<CoffFileHeader, CoffError> {
    if data.len() < 20 {
        return Err(CoffError::TooSmall);
    }
    Ok(CoffFileHeader {
        machine: read_u16(data, 0)?,
        number_of_sections: read_u16(data, 2)?,
        time_date_stamp: read_u32(data, 4)?,
        ptr_to_symbol_table: read_u32(data, 8)?,
        number_of_symbols: read_u32(data, 12)?,
        size_of_optional_header: read_u16(data, 16)?,
        characteristics: read_u16(data, 18)?,
    })
}

fn parse_section_header(data: &[u8], off: usize) -> Result<SectionHeader, CoffError> {
    Ok(SectionHeader {
        name: read_arr8(data, off)?,
        virtual_size: read_u32(data, off + 8)?,
        virtual_address: read_u32(data, off + 12)?,
        size_of_raw_data: read_u32(data, off + 16)?,
        ptr_to_raw_data: read_u32(data, off + 20)?,
        ptr_to_relocations: read_u32(data, off + 24)?,
        ptr_to_line_numbers: read_u32(data, off + 28)?,
        number_of_relocations: read_u16(data, off + 32)?,
        number_of_line_numbers: read_u16(data, off + 34)?,
        characteristics: read_u32(data, off + 36)?,
    })
}

fn parse_relocation(data: &[u8], off: usize) -> Result<Relocation, CoffError> {
    Ok(Relocation {
        virtual_address: read_u32(data, off)?,
        symbol_table_index: read_u32(data, off + 4)?,
        relocation_type: read_u16(data, off + 8)?,
    })
}

fn parse_symbol_entry(data: &[u8], off: usize) -> Result<SymbolEntry, CoffError> {
    Ok(SymbolEntry {
        name: read_arr8(data, off)?,
        value: read_u32(data, off + 8)?,
        section_number: read_u16(data, off + 12)? as i16,
        symbol_type: read_u16(data, off + 14)?,
        storage_class: data.get(off + 16).copied().ok_or(CoffError::InvalidOffset)?,
        number_of_aux_symbols: data.get(off + 17).copied().ok_or(CoffError::InvalidOffset)?,
    })
}

// ---------- رزولوشن نام نماد ----------
fn resolve_symbol_name(entry: &SymbolEntry, string_table: &[u8]) -> String {
    // اگه ۴ بایت اول صفر باشه، ۴ بایت بعدی آفست داخل string table هست
    if entry.name[0..4] == [0u8; 4] {
        let off = u32::from_le_bytes(entry.name[4..8].try_into().unwrap()) as usize;
        let mut end = off;
        while end < string_table.len() && string_table[end] != 0 {
            end += 1;
        }
        return String::from_utf8_lossy(&string_table[off..end]).into_owned();
    }
    // وگرنه اسم کوتاه (null-terminated داخل ۸ بایت)
    let mut end = 0;
    while end < 8 && entry.name[end] != 0 {
        end += 1;
    }
    String::from_utf8_lossy(&entry.name[..end]).into_owned()
}

// ---------- تابع اصلی پارس ----------
impl CoffFile {
    pub fn parse(data: &[u8]) -> Result<Self, CoffError> {
        let header = parse_header(data)?;

        if header.machine != IMAGE_FILE_MACHINE_AMD64 {
            return Err(CoffError::UnsupportedMachine(header.machine));
        }

        // --- سکشن‌ها ---
        let mut sections = Vec::new();
        let section_table_start = 20usize + header.size_of_optional_header as usize;

        for i in 0..header.number_of_sections as usize {
            let hdr_off = section_table_start + i * 40;
            let sh = parse_section_header(data, hdr_off)?;

            // داده خام سکشن
            let raw = read_range(data, sh.ptr_to_raw_data as usize, sh.size_of_raw_data as usize)?;

            // رکوردهای relocation
            let mut relocations = Vec::new();
            let reloc_base = sh.ptr_to_relocations as usize;
            for j in 0..sh.number_of_relocations as usize {
                relocations.push(parse_relocation(data, reloc_base + j * 10)?);
            }

            sections.push(Section {
                header: sh,
                data: raw.to_vec(),
                relocations,
            });
        }

        // --- نمادها (با احتساب aux symbols) ---
        // --- نمادها با احتساب Auxiliary Symbols ---

        let mut symbols = Vec::new();

        let sym_base = header.ptr_to_symbol_table as usize;
        let sym_count = header.number_of_symbols as usize;

        let mut i = 0usize;

        while i < sym_count {
            let entry = parse_symbol_entry(data, sym_base + i * 18)?;

            symbols.push(Symbol {
                entry,
                name: String::new(),
                is_aux: false,
            });

            let aux_count = entry.number_of_aux_symbols as usize;

            for aux_index in 0..aux_count {
                let aux_pos = i + 1 + aux_index;

                if aux_pos >= sym_count {
                    return Err(CoffError::InvalidOffset);
                }

                let aux_entry =
                    parse_symbol_entry(data, sym_base + aux_pos * 18)?;

                symbols.push(Symbol {
                    entry: aux_entry,
                    name: String::new(),
                    is_aux: true,
                });
            }

            i += 1 + aux_count;
        }


        // --- string table (بعد از جدول نمادها) ---
        let str_base = sym_base
            .checked_add(sym_count * 18)
            .ok_or(CoffError::InvalidOffset)?;

        let string_table = if str_base + 4 <= data.len() {
            let size = read_u32(data, str_base)? as usize;

            if size >= 4 && str_base + size <= data.len() {
                data[str_base..str_base + size].to_vec()
            } else {
                return Err(CoffError::InvalidOffset);
            }
        } else {
            Vec::new()
        };

        // --- رزولوشن اسم نمادها ---
        for sym in &mut symbols {
            sym.name = resolve_symbol_name(&sym.entry, &string_table);
        }

        Ok(CoffFile {
            header,
            sections,
            symbols,
            string_table,
        })
    }
}
pub fn section_name(section: &Section) -> String {
    let end = section
        .header
        .name
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(section.header.name.len());

    String::from_utf8_lossy(&section.header.name[..end]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bof() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("whoami.x64.o");

        let bytes = std::fs::read(&path)
            .expect("failed to read whoami.x64.o");

        let f = CoffFile::parse(&bytes)
            .expect("failed to parse COFF");

        println!();
        println!("========== COFF TEST ==========");
        println!("file: {}", path.display());
        println!("size: {} bytes", bytes.len());
        println!("machine: 0x{:04X}", f.header.machine);
        println!("sections: {}", f.sections.len());
        println!("symbols: {}", f.symbols.len());

        println!();
        println!("---------- Sections ----------");

        for (index, section) in f.sections.iter().enumerate() {
            println!(
                "[{}] {:<12} raw_size={:<6} relocations={:<4} characteristics=0x{:08X}",
                index,
                section_name(section),
                section.header.size_of_raw_data,
                section.header.number_of_relocations,
                section.header.characteristics
            );
        }

        println!();
        println!("---------- Symbols ----------");

        for (index, symbol) in f.symbols.iter().enumerate() {
            println!(
                "[{}] {:<40} section={:<4} value=0x{:X} aux={}",
                index,
                symbol.name,
                symbol.entry.section_number,
                symbol.entry.value,
                symbol.is_aux
            );
        }

        println!();
        println!("========== COFF PARSE OK ==========");

        // این قسمت صرفاً برای تأیید بصری است که فایل موردنظر
        // واقعاً در اختیار loader قرار گرفته است.
        println!("BOF loaded successfully: whoami.x64.o");
        println!("Execution stage: not performed");
    }
}

fn read_cstring(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).to_string()
}

#[test]
fn apply_all_relocations_to_bof() {
    use super::*;
    use reloc::*;

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("whoami.x64.o");

    let bytes =
        std::fs::read(path)
            .expect("read bof");

    let coff =
        CoffFile::parse(&bytes)
            .expect("parse");


    println!();
    println!("========== RELOCATION TEST ==========");


    // Fake virtual layout
    let section_bases =
        compute_section_virtual_addresses(
            &coff.sections
        );


    // Dummy external resolver
    let dummy_resolver =
        |name: &str| -> Option<usize> {

            println!(
                "  resolving external: {}",
                name
            );

            // آدرس فرضی نزدیک به layout سکشن‌ها
            Some(0x0010_0000usize)
        };


    let mut total_relocations = 0usize;


    for (section_index, section) in
        coff.sections.iter().enumerate()
    {
        let name =
            section_name(section);

        if section.relocations.is_empty() {
            continue;
        }


        println!();
        println!(
            "Section [{}] {}",
            section_index,
            name
        );

        println!(
            "  base: 0x{:X}",
            section_bases[section_index]
        );

        println!(
            "  relocations: {}",
            section.relocations.len()
        );


        let mut section_data =
            section.data.clone();


        let result =
            apply_relocations(
                &mut section_data,
                &section.relocations,
                &coff.symbols,
                &dummy_resolver,
                section_bases[section_index],
                &section_bases,
            );


        match result {
            Ok(()) => {
                println!(
                    "  result: OK"
                );
            }

            Err(error) => {
                panic!(
                    "Relocation failed for section '{}': {:?}",
                    name,
                    error
                );
            }
        }


        total_relocations +=
            section.relocations.len();
    }


    println!();
    println!(
        "Total relocations processed: {}",
        total_relocations
    );
}
