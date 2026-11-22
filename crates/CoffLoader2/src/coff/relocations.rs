use std::collections::HashMap;

use super::{CoffRelocation, CoffSection, CoffSymbol};
use super::strings::checked_range;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelocType {
    Absolute,
    Addr64,
    Addr32,
    Addr32Nb,
    Rel32,
    Rel32_1,
    Rel32_2,
    Rel32_3,
    Rel32_4,
    Rel32_5,
    Section,
    SecRel,
    Other(u16),
}

impl From<u16> for RelocType {
    fn from(v: u16) -> Self {
        match v {
            0x0000 => Self::Absolute,
            0x0001 => Self::Addr64,
            0x0002 => Self::Addr32,
            0x0003 => Self::Addr32Nb,
            0x0004 => Self::Rel32,
            0x0005 => Self::Rel32_1,
            0x0006 => Self::Rel32_2,
            0x0007 => Self::Rel32_3,
            0x0008 => Self::Rel32_4,
            0x0009 => Self::Rel32_5,
            0x000A => Self::Section,
            0x000B => Self::SecRel,
            other => Self::Other(other),
        }
    }
}

pub fn parse(
    data: &[u8],
    sections: &[CoffSection],
    symbols: &[CoffSymbol],
) -> Result<Vec<CoffRelocation>, String> {
    let mut relocations = Vec::new();

    let symbol_map: HashMap<u32, &CoffSymbol> = symbols
        .iter()
        .map(|s| (s.table_index, s))
        .collect();

    for section in sections {
        let count = section.number_of_relocations as usize;
        if count == 0 {
            continue;
        }
        if section.number_of_relocations == 0xFFFF {
            return Err(format!(
                "Extended relocation count not supported: {}",
                section.name
            ));
        }
        if section.relocation_pointer == 0 {
            return Err(format!(
                "Missing relocation pointer: {}",
                section.name
            ));
        }

        let start = section.relocation_pointer as usize;
        for index in 0..count {
            let offset = start
                .checked_add(index.checked_mul(10).ok_or("Relocation offset overflow")?)
                .ok_or("Relocation offset overflow")?;

            let entry = checked_range(data, offset, 10)?;

            let virtual_address = u32::from_le_bytes(entry[0..4].try_into().unwrap());
            let symbol_table_index = u32::from_le_bytes(entry[4..8].try_into().unwrap());
            let relocation_type = u16::from_le_bytes(entry[8..10].try_into().unwrap());

            let symbol = symbol_map.get(&symbol_table_index).ok_or_else(|| {
                format!(
                    "Relocation references missing or auxiliary symbol index {}",
                    symbol_table_index
                )
            })?;

            relocations.push(CoffRelocation {
                section_name: section.name.clone(),
                virtual_address,
                symbol_table_index,
                symbol_name: symbol.name.clone(),
                relocation_type,
            });
        }
    }
    Ok(relocations)
}