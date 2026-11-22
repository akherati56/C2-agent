use super::CoffHeader;

pub fn get_string_table<'a>(
    data: &'a [u8],
    header: &CoffHeader,
) -> Result<Option<&'a [u8]>, String> {
    if header.number_of_symbols == 0 {
        return Ok(None);
    }
    if header.pointer_to_symbol_table == 0 {
        return Err("Symbol table pointer is zero".into());
    }

    let start = header.pointer_to_symbol_table as usize;

    let symbol_size = (header.number_of_symbols as usize)
        .checked_mul(18)
        .ok_or("Symbol table size overflow")?;

    let string_start = start
        .checked_add(symbol_size)
        .ok_or("String table offset overflow")?;

    let size_bytes = checked_range(data, string_start, 4)?;
    let size = u32::from_le_bytes(size_bytes.try_into().unwrap()) as usize;

    if size < 4 {
        return Err("Invalid string table size".into());
    }

    checked_range(data, string_start, size).map(Some)
}

pub fn read_string(table: &[u8], offset: usize) -> Result<String, String> {
    if offset < 4 || offset >= table.len() {
        return Err("Invalid string table offset".into());
    }
    let remaining = &table[offset..];
    let end = remaining
        .iter()
        .position(|&b| b == 0)
        .ok_or("Unterminated string")?;
    Ok(String::from_utf8_lossy(&remaining[..end]).into_owned())
}

pub fn parse_symbol_name(
    name: &[u8],
    table: Option<&[u8]>,
) -> Result<String, String> {
    if name.len() != 8 {
        return Err("Invalid COFF symbol name length".into());
    }
    if name[0..4] == [0, 0, 0, 0] {
        let offset = u32::from_le_bytes(name[4..8].try_into().unwrap()) as usize;
        return read_string(
            table.ok_or("Missing string table for long symbol name")?,
            offset,
        );
    }
    let end = name.iter().position(|&b| b == 0).unwrap_or(8);
    Ok(String::from_utf8_lossy(&name[..end]).into_owned())
}

pub fn parse_section_name(
    name: &[u8],
    table: Option<&[u8]>,
) -> Result<String, String> {
    let end = name.iter().position(|&b| b == 0).unwrap_or(8);
    let value = String::from_utf8_lossy(&name[..end]);

    if let Some(offset_text) = value.strip_prefix('/') {
        if !offset_text.is_empty() && offset_text.bytes().all(|b| b.is_ascii_digit()) {
            let offset = offset_text
                .parse::<usize>()
                .map_err(|_| "Invalid section-name offset")?;
            return read_string(
                table.ok_or("Missing string table for long section name")?,
                offset,
            );
        }
    }
    Ok(value.into_owned())
}

pub fn checked_range(data: &[u8], offset: usize, size: usize) -> Result<&[u8], String> {
    let end = offset.checked_add(size).ok_or("Offset overflow")?;
    data.get(offset..end).ok_or_else(|| {
        format!("Out-of-bounds access: offset=0x{:X}, size={}", offset, size)
    })
}