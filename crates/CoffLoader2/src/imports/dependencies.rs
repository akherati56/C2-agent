use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalDependency {
    pub symbol: String,
    pub dll: Option<String>,
    pub function: String,
    pub is_import_symbol: bool,
}

#[derive(Debug, Default)]
pub struct DependencyReport {
    pub dependencies: Vec<ExternalDependency>,
}

const IMAGE_SYM_CLASS_EXTERNAL: u8 = 2;

/// Analyze undefined external COFF symbols.
pub fn analyze_dependencies<'a, I>(symbols: I) -> DependencyReport
where
    I: IntoIterator<Item = (&'a str, i16, u8)>,
{
    let mut report = DependencyReport::default();
    let mut seen = HashSet::new();

    for (name, section_number, storage_class) in symbols {
        if section_number != 0 || storage_class != IMAGE_SYM_CLASS_EXTERNAL {
            continue;
        }
        if !seen.insert(name.to_owned()) {
            continue;
        }
        report.dependencies.push(parse_external_symbol(name));
    }
    report
}

fn parse_external_symbol(name: &str) -> ExternalDependency {
    let Some(import_name) = name.strip_prefix("__imp_") else {
        return ExternalDependency {
            symbol: name.to_owned(),
            dll: None,
            function: name.to_owned(),
            is_import_symbol: false,
        };
    };

    if let Some((dll, function)) = import_name.split_once('$') {
        if !dll.is_empty() && !function.is_empty() {
            return ExternalDependency {
                symbol: name.to_owned(),
                dll: Some(dll.to_owned()),
                function: function.to_owned(),
                is_import_symbol: true,
            };
        }
    }

    ExternalDependency {
        symbol: name.to_owned(),
        dll: Some("kernel32".to_owned()),
        function: import_name.to_owned(),
        is_import_symbol: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_explicit_dll() {
        let result =
            parse_external_symbol("__imp_KERNEL32$CreateToolhelp32Snapshot");
        assert_eq!(result.dll.as_deref(), Some("KERNEL32"));
        assert_eq!(result.function, "CreateToolhelp32Snapshot");
    }

    #[test]
    fn defaults_to_kernel32() {
        let result = parse_external_symbol("__imp_CloseHandle");
        assert_eq!(result.dll.as_deref(), Some("kernel32"));
        assert_eq!(result.function, "CloseHandle");
    }

    #[test]
    fn ignores_defined_symbols() {
        let symbols = [
            ("__imp_CloseHandle", 0, 2),
            ("DefinedFunction", 1, 2),
            ("StaticSymbol", 0, 3),
        ];
        let report = analyze_dependencies(symbols);
        assert_eq!(report.dependencies.len(), 1);
    }

    #[test]
    fn removes_duplicates() {
        let symbols = [
            ("__imp_CloseHandle", 0, 2),
            ("__imp_CloseHandle", 0, 2),
        ];
        let report = analyze_dependencies(symbols);
        assert_eq!(report.dependencies.len(), 1);
    }
}