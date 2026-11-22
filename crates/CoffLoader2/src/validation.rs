use crate::coff::CoffObject;

pub fn validate(object: &CoffObject, data_size: usize) -> Result<(), String> {
    let header = &object.header;

    if header.number_of_sections as usize != object.sections.len() {
        return Err("Section count mismatch".into());
    }

    if header.number_of_symbols != 0 && header.pointer_to_symbol_table == 0 {
        return Err("Invalid symbol-table pointer".into());
    }

    for section in &object.sections {
        if section.raw_data_size > 0 {
            let start = section.raw_data_pointer as usize;
            let end = start
                .checked_add(section.raw_data_size as usize)
                .ok_or("Section data range overflow")?;
            if end > data_size {
                return Err(format!(
                    "Section '{}' raw data exceeds file bounds",
                    section.name
                ));
            }
        }

        if section.number_of_relocations > 0
            && section.number_of_relocations != 0xFFFF
        {
            let size = (section.number_of_relocations as usize)
                .checked_mul(10)
                .ok_or("Relocation table size overflow")?;
            let start = section.relocation_pointer as usize;
            let end = start
                .checked_add(size)
                .ok_or("Relocation table range overflow")?;
            if start == 0 || end > data_size {
                return Err(format!(
                    "Section '{}' relocation table exceeds file bounds",
                    section.name
                ));
            }
        }
    }

    for relocation in &object.relocations {
        if !object.symbols.iter().any(|symbol| {
            symbol.table_index == relocation.symbol_table_index
        }) {
            return Err(format!(
                "Invalid relocation symbol reference: {}",
                relocation.symbol_table_index
            ));
        }
    }

    Ok(())
}