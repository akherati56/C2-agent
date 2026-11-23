use crate::coff;
use crate::imports;
use crate::validation;
use crate::loader;

use std::fs::File;
use std::process;
use std::ffi::CString;
use memmap2::Mmap;
use imports::dependencies::analyze_dependencies;

/// Toggle verbose COFF-loader diagnostics.
///
/// `true`  -> full parse/validate/relocation dump (develop)
/// `false` -> silent operation, only the payload's own output appears
///
/// When integrating into a C2 agent, set this from your tasking
/// (e.g. `let develop_mode = task_args.develop;`) and pass it into
/// `loader::load_and_execute`.
//const DEVELOP_MODE: bool = false;
const DEVELOP_MODE: bool = true;

pub fn run(args: CString) {
    let develop_mode = DEVELOP_MODE;

    // Hardcoded payload path — no CLI arguments.
    let file_path = "listprocess-raw.o";

    let file = File::open(file_path).unwrap_or_else(|e| {
        eprintln!("Failed to open file: {}", e);
        process::exit(1);
    });

    let metadata = file.metadata().unwrap_or_else(|e| {
        eprintln!("Failed to read file metadata: {}", e);
        process::exit(1);
    });

    if metadata.len() == 0 {
        eprintln!("Input file is empty");
        process::exit(1);
    }

    let mmap = unsafe {
        Mmap::map(&file).unwrap_or_else(|e| {
            eprintln!("Failed to map file: {}", e);
            process::exit(1);
        })
    };

    if develop_mode {
        println!("Input: {}", file_path);
        println!("Size: {} bytes", mmap.len());
    }

    // 1. Parse
    let object = coff::parse(&mmap).unwrap_or_else(|e| {
        eprintln!("COFF parsing failed: {}", e);
        process::exit(1);
    });

    // 2. Validate
    if let Err(e) = validation::validate(&object, mmap.len()) {
        eprintln!("Validation failed: {}", e);
        process::exit(1);
    }

    // 3. Develop-mode diagnostics
    if develop_mode {
        println!("\nCOFF parsing and validation successful.");

        println!("\n========== HEADER ==========");
        println!("{:#?}", object.header);

        println!("\n========== SECTIONS ==========");
        println!("{:#?}", object.sections);

        println!("\n========== SYMBOLS ==========");
        println!("{:#?}", object.symbols);

        println!("\n========== RELOCATIONS ==========");
        println!("{:#?}", object.relocations);

        let report = analyze_dependencies(object.symbols.iter().map(|s| {
            (s.name.as_str(), s.section_number, s.storage_class)
        }));

        println!("\n========== EXTERNAL DEPENDENCIES ==========");
        for d in &report.dependencies {
            println!("Symbol:   {}", d.symbol);
            println!("DLL:      {}", d.dll.as_deref().unwrap_or("Unknown"));
            println!("Function: {}", d.function);
            println!();
        }
        println!("Total external dependencies: {}", report.dependencies.len());

        println!("\n========== LOADING & EXECUTING ==========");
    }

    // 4. Load & execute
    if let Err(e) = loader::load_and_execute(&object, &mmap, develop_mode, &args) {
        eprintln!("Execution failed: {}", e);
        process::exit(1);
    }
}
