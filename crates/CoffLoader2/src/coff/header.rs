use super::CoffHeader;

pub fn parse(data: &[u8]) -> Result<CoffHeader, String> {
    if data.len() < 20 {
        return Err("File is too small for a COFF header".into());
    }

    Ok(CoffHeader {
        machine: u16::from_le_bytes(data[0..2].try_into().unwrap()),
        number_of_sections: u16::from_le_bytes(data[2..4].try_into().unwrap()),
        timestamp: u32::from_le_bytes(data[4..8].try_into().unwrap()),
        pointer_to_symbol_table: u32::from_le_bytes(data[8..12].try_into().unwrap()),
        number_of_symbols: u32::from_le_bytes(data[12..16].try_into().unwrap()),
        optional_header_size: u16::from_le_bytes(data[16..18].try_into().unwrap()),
        characteristics: u16::from_le_bytes(data[18..20].try_into().unwrap()),
    })
}