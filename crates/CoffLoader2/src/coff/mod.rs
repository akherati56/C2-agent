pub mod header;
pub mod sections;
pub mod symbols;
pub mod relocations;
pub mod strings;

#[derive(Debug)]
pub struct CoffHeader {
    pub machine: u16,
    pub number_of_sections: u16,
    pub timestamp: u32,
    pub pointer_to_symbol_table: u32,
    pub number_of_symbols: u32,
    pub optional_header_size: u16,
    pub characteristics: u16,
}

#[derive(Debug)]
pub struct CoffSection {
    pub name: String,
    pub virtual_size: u32,
    pub virtual_address: u32,
    pub raw_data_size: u32,
    pub raw_data_pointer: u32,
    pub relocation_pointer: u32,
    pub line_number_pointer: u32,
    pub number_of_relocations: u16,
    pub number_of_line_numbers: u16,
    pub characteristics: u32,
}

#[derive(Debug)]
pub struct CoffSymbol {
    pub table_index: u32,
    pub name: String,
    pub value: u32,
    pub section_number: i16,
    pub symbol_type: u16,
    pub storage_class: u8,
    pub auxiliary_count: u8,
}

#[allow(dead_code)]
impl CoffSymbol {
    pub fn is_undefined_external(&self) -> bool {
        self.section_number == 0 && self.storage_class == 2
    }

    pub fn is_weak_external(&self) -> bool {
        self.section_number == 0 && self.storage_class == 105
    }

    pub fn is_defined(&self) -> bool {
        self.section_number > 0
    }
}

#[derive(Debug)]
pub struct CoffRelocation {
    pub section_name: String,
    pub virtual_address: u32,
    pub symbol_table_index: u32,
    pub symbol_name: String,
    pub relocation_type: u16,
}

#[derive(Debug)]
pub struct CoffObject {
    pub header: CoffHeader,
    pub sections: Vec<CoffSection>,
    pub symbols: Vec<CoffSymbol>,
    pub relocations: Vec<CoffRelocation>,
}

pub fn parse(data: &[u8]) -> Result<CoffObject, String> {
    let header = header::parse(data)?;
    let string_table = strings::get_string_table(data, &header)?;
    let sections = sections::parse(data, &header, string_table.as_deref())?;
    let symbols = symbols::parse(data, &header, string_table.as_deref())?;
    let relocations = relocations::parse(data, &sections, &symbols)?;

    Ok(CoffObject {
        header,
        sections,
        symbols,
        relocations,
    })
}