// src/coff/reloc.rs

use super::types::*;

#[derive(Debug)]
pub enum RelocError {
    UnsupportedReloc(u16),
    InvalidOffset,
    SymbolNotFound(String),
    SectionNotFound,
    RelocationOverflow,
}

impl std::fmt::Display for RelocError {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        match self {
            RelocError::UnsupportedReloc(t) => {
                write!(
                    f,
                    "unsupported relocation type: 0x{t:04X}"
                )
            }

            RelocError::InvalidOffset => {
                write!(f, "offset out of bounds")
            }

            RelocError::SymbolNotFound(s) => {
                write!(f, "symbol '{s}' not found")
            }

            RelocError::SectionNotFound => {
                write!(f, "section not found")
            }

            RelocError::RelocationOverflow => {
                write!(f, "relocation value overflow")
            }
        }
    }
}

impl std::error::Error for RelocError {}


// ---------- Symbol Resolver ----------

pub type SymbolResolver =
dyn Fn(&str) -> Option<usize>;


// ---------- Write helpers ----------

fn write_u32(
    data: &mut [u8],
    offset: usize,
    value: u32,
) -> Result<(), RelocError> {
    if offset.checked_add(4).ok_or(RelocError::InvalidOffset)? > data.len() {
        return Err(RelocError::InvalidOffset);
    }

    data[offset..offset + 4]
        .copy_from_slice(&value.to_le_bytes());

    Ok(())
}

fn write_u64(
    data: &mut [u8],
    offset: usize,
    value: u64,
) -> Result<(), RelocError> {
    if offset.checked_add(8).ok_or(RelocError::InvalidOffset)? > data.len() {
        return Err(RelocError::InvalidOffset);
    }

    data[offset..offset + 8]
        .copy_from_slice(&value.to_le_bytes());

    Ok(())
}


// ---------- Apply one relocation ----------

fn apply_one(
    section_data: &mut [u8],
    offset: usize,
    relocation_type: u16,
    symbol_addr: usize,
    section_base: usize,
    image_base: usize,
) -> Result<(), RelocError> {

    match relocation_type {

        IMAGE_REL_AMD64_ABSOLUTE => {
            Ok(())
        }


        IMAGE_REL_AMD64_ADDR64 => {
            write_u64(
                section_data,
                offset,
                symbol_addr as u64,
            )
        }


        IMAGE_REL_AMD64_ADDR32 => {
            if symbol_addr > u32::MAX as usize {
                return Err(
                    RelocError::RelocationOverflow
                );
            }

            write_u32(
                section_data,
                offset,
                symbol_addr as u32,
            )
        }


        IMAGE_REL_AMD64_REL32
        | IMAGE_REL_AMD64_REL32_1
        | IMAGE_REL_AMD64_REL32_2
        | IMAGE_REL_AMD64_REL32_3
        | IMAGE_REL_AMD64_REL32_4
        | IMAGE_REL_AMD64_REL32_5 => {

            if offset.checked_add(4)
                .ok_or(RelocError::InvalidOffset)?
                > section_data.len()
            {
                return Err(
                    RelocError::InvalidOffset
                );
            }

            let adjustment =
                match relocation_type {
                    IMAGE_REL_AMD64_REL32 => 0,
                    IMAGE_REL_AMD64_REL32_1 => 1,
                    IMAGE_REL_AMD64_REL32_2 => 2,
                    IMAGE_REL_AMD64_REL32_3 => 3,
                    IMAGE_REL_AMD64_REL32_4 => 4,
                    IMAGE_REL_AMD64_REL32_5 => 5,
                    _ => unreachable!(),
                };

            let next_instruction =
                section_base
                    .checked_add(offset)
                    .and_then(|v| v.checked_add(4))
                    .and_then(|v| v.checked_add(adjustment))
                    .ok_or(RelocError::RelocationOverflow)?;

            let delta =
                (symbol_addr as i64)
                    - (next_instruction as i64);

            let delta =
                i32::try_from(delta)
                    .map_err(|_| {
                        RelocError::RelocationOverflow
                    })?;

            write_u32(
                section_data,
                offset,
                delta as u32,
            )
        }


        // IMAGE_REL_AMD64_ADDR32NB
        //
        // Writes a 32-bit RVA:
        //     SymbolVA - ImageBase
        //
        IMAGE_REL_AMD64_ADDR32NB => {

            let rva =
                symbol_addr
                    .checked_sub(image_base)
                    .ok_or(
                        RelocError::RelocationOverflow
                    )?;

            if rva > u32::MAX as usize {
                return Err(
                    RelocError::RelocationOverflow
                );
            }

            write_u32(
                section_data,
                offset,
                rva as u32,
            )
        }


        IMAGE_REL_AMD64_SECTION => {

            if symbol_addr > u32::MAX as usize {
                return Err(
                    RelocError::RelocationOverflow
                );
            }

            write_u32(
                section_data,
                offset,
                symbol_addr as u32,
            )
        }


        IMAGE_REL_AMD64_SECREL => {

            if symbol_addr < section_base {
                return Err(
                    RelocError::RelocationOverflow
                );
            }

            let offset_in_section =
                symbol_addr - section_base;

            if offset_in_section > u32::MAX as usize {
                return Err(
                    RelocError::RelocationOverflow
                );
            }

            write_u32(
                section_data,
                offset,
                offset_in_section as u32,
            )
        }


        _ => {
            Err(
                RelocError::UnsupportedReloc(
                    relocation_type
                )
            )
        }
    }
}


// ---------- Apply relocations ----------

pub fn apply_relocations(
    section_data: &mut [u8],
    relocations: &[Relocation],
    symbols: &[super::Symbol],
    resolve_external: &SymbolResolver,
    section_base: usize,
    section_bases: &[usize],
) -> Result<(), RelocError> {

    let image_base =
        *section_bases
            .iter()
            .min()
            .unwrap_or(&0);


    for reloc in relocations {

        let symbol_index =
            reloc.symbol_table_index as usize;

        let sym =
            symbols
                .get(symbol_index)
                .ok_or_else(|| {
                    RelocError::SymbolNotFound(
                        format!(
                            "idx={}",
                            symbol_index
                        )
                    )
                })?;

        if sym.is_aux {
            return Err(
                RelocError::SymbolNotFound(
                    format!(
                        "idx={} (auxiliary record)",
                        symbol_index
                    )
                )
            );
        }


        let symbol_addr =
            match sym.entry.section_number {

                IMAGE_SYM_UNDEFINED => {
                    resolve_external(&sym.name)
                        .ok_or_else(|| {
                            RelocError::SymbolNotFound(
                                sym.name.clone()
                            )
                        })?
                }


                IMAGE_SYM_ABSOLUTE => {
                    sym.entry.value as usize
                }


                n if n > 0 => {

                    let section_index =
                        (n - 1) as usize;

                    let base =
                        section_bases
                            .get(section_index)
                            .ok_or(
                                RelocError::SectionNotFound
                            )?;

                    base
                        .checked_add(
                            sym.entry.value as usize
                        )
                        .ok_or(
                            RelocError::RelocationOverflow
                        )?
                }


                _ => {
                    return Err(
                        RelocError::InvalidOffset
                    );
                }
            };


        let offset =
            reloc.virtual_address as usize;


        println!(
            "  reloc: offset=0x{:X}, type=0x{:04X}, symbol={}, symbol_addr=0x{:X}, section_base=0x{:X}",
            reloc.virtual_address,
            reloc.relocation_type,
            sym.name,
            symbol_addr,
            section_base
        );


        apply_one(
            section_data,
            offset,
            reloc.relocation_type,
            symbol_addr,
            section_base,
            image_base,
        )?;
    }

    Ok(())
}


// ---------- Section virtual layout ----------

pub fn compute_section_virtual_addresses(
    sections: &[super::Section],
) -> Vec<usize> {

    let mut current = 0usize;

    let mut addresses =
        Vec::with_capacity(
            sections.len()
        );


    for section in sections {

        addresses.push(current);

        let size =
            section
                .header
                .virtual_size
                .max(
                    section
                        .header
                        .size_of_raw_data
                ) as usize;


        let aligned =
            (size + 0xFFF)
                & !0xFFF;


        current += aligned;
    }


    addresses
}