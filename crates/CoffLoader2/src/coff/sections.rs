use super::{CoffHeader, CoffSection};
use super::strings::{checked_range, parse_section_name};

pub fn parse(
    data: &[u8],
    header: &CoffHeader,
    string_table: Option<&[u8]>,
) -> Result<Vec<CoffSection>, String> {
    let mut sections = Vec::new();

    let start = 20usize
        .checked_add(header.optional_header_size as usize)
        .ok_or("Section table offset overflow")?;

    for index in 0..header.number_of_sections as usize {
        let offset = start
            .checked_add(index.checked_mul(40).ok_or("Offset overflow")?)
            .ok_or("Offset overflow")?;

        let entry = checked_range(data, offset, 40)?;

        sections.push(CoffSection {
            name: parse_section_name(&entry[0..8], string_table)?,
            virtual_size: u32::from_le_bytes(entry[8..12].try_into().unwrap()),
            virtual_address: u32::from_le_bytes(entry[12..16].try_into().unwrap()),
            raw_data_size: u32::from_le_bytes(entry[16..20].try_into().unwrap()),
            raw_data_pointer: u32::from_le_bytes(entry[20..24].try_into().unwrap()),
            relocation_pointer: u32::from_le_bytes(entry[24..28].try_into().unwrap()),
            line_number_pointer: u32::from_le_bytes(entry[28..32].try_into().unwrap()),
            number_of_relocations: u16::from_le_bytes(entry[32..34].try_into().unwrap()),
            number_of_line_numbers: u16::from_le_bytes(entry[34..36].try_into().unwrap()),
            characteristics: u32::from_le_bytes(entry[36..40].try_into().unwrap()),
        });
    }

    Ok(sections)
}