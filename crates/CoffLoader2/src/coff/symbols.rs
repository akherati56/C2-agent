use super::{CoffHeader, CoffSymbol};
use super::strings::{checked_range, parse_symbol_name};

pub fn parse(
    data: &[u8],
    header: &CoffHeader,
    string_table: Option<&[u8]>,
) -> Result<Vec<CoffSymbol>, String> {
    let mut symbols = Vec::new();

    if header.number_of_symbols == 0 {
        return Ok(symbols);
    }
    if header.pointer_to_symbol_table == 0 {
        return Err("Symbol table pointer is zero".into());
    }

    let start = header.pointer_to_symbol_table as usize;
    let mut index = 0usize;

    while index < header.number_of_symbols as usize {
        let offset = start
            .checked_add(index.checked_mul(18).ok_or("Symbol offset overflow")?)
            .ok_or("Symbol offset overflow")?;

        let entry = checked_range(data, offset, 18)?;

        let name = parse_symbol_name(&entry[0..8], string_table)?;
        let value = u32::from_le_bytes(entry[8..12].try_into().unwrap());
        let section_number = i16::from_le_bytes(entry[12..14].try_into().unwrap());
        let symbol_type = u16::from_le_bytes(entry[14..16].try_into().unwrap());
        let storage_class = entry[16];
        let auxiliary_count = entry[17];

        let remaining = header.number_of_symbols as usize - index - 1;
        if auxiliary_count as usize > remaining {
            return Err(format!(
                "Invalid auxiliary count at symbol index {}",
                index
            ));
        }

        symbols.push(CoffSymbol {
            table_index: index as u32,
            name,
            value,
            section_number,
            symbol_type,
            storage_class,
            auxiliary_count,
        });

        index += 1 + auxiliary_count as usize;
    }

    Ok(symbols)
}