//! COFF loader: allocates section memory, resolves symbols, applies
//! relocations, and invokes the entry point (`go`).
//!
//! Faithful port of the C reference (`LoadCOFF` / `ResolveSymbols`).
//!
//! All progress output is gated by `develop_mode`. When `false`, the
//! loader is silent and only the payload's own output (printf, etc.)
//! is visible.

use std::collections::HashMap;
use std::ffi::CString;
use std::os::raw::{c_char, c_void};
use std::ptr;

use crate::coff::relocations::RelocType;
use crate::coff::{CoffObject, CoffSection};

// ---------- Windows FFI ----------

type HMODULE = *mut c_void;
type FARPROC = *mut c_void;

const MEM_COMMIT: u32 = 0x0000_1000;
const MEM_RESERVE: u32 = 0x0000_2000;
const MEM_RELEASE: u32 = 0x0000_8000;
const MEM_TOP_DOWN: u32 = 0x0010_0000;
const PAGE_EXECUTE_READWRITE: u32 = 0x40;
const PAGE_READWRITE: u32 = 0x04;

const IMAGE_SCN_CNT_CODE: u32 = 0x0000_0020;
const TOKEN_IMP: &str = "__imp_";
const PAGE_SIZE: usize = 0x1000;

#[link(name = "kernel32")]
#[allow(non_snake_case)]
unsafe extern "system" {
    fn VirtualAlloc(
        lpAddress: *mut c_void,
        dwSize: usize,
        flAllocationType: u32,
        flProtect: u32,
    ) -> *mut c_void;

    fn VirtualFree(
        lpAddress: *mut c_void,
        dwSize: usize,
        dwFreeType: u32,
    ) -> i32;

    fn LoadLibraryA(lpLibFileName: *const c_char) -> HMODULE;
    fn GetProcAddress(hModule: HMODULE, lpProcName: *const c_char) -> FARPROC;
}

// ---------- Internal state ----------

struct SectionMem {
    section_index: usize,
    base: *mut u8,
    size: usize,
}

struct SymbolMem {
    name: String,
    section_number: i16,
    value: u32,
    storage_class: u8,
    in_memory_address: u64,
    got_address: Option<u64>,
}

struct GotMem {
    base: *mut u8,
    size: usize,
}

// ---------- Entry point ----------

pub fn load_and_execute(
    object: &CoffObject,
    raw: &[u8],
    develop_mode: bool,
    args: &std::ffi::CString,
) -> Result<(), String>{
    // ---- Step 1: allocate & copy sections ----
    let mut sections: Vec<SectionMem> = Vec::new();

    for (i, s) in object.sections.iter().enumerate() {
        if s.raw_data_size == 0 {
            continue;
        }

        let base = alloc_section(s)?;

        if develop_mode {
            println!(
                "[loader] section[{}] '{}' -> base={:p} size=0x{:X}",
                i, s.name, base, s.raw_data_size
            );
        }

        sections.push(SectionMem {
            section_index: i,
            base,
            size: s.raw_data_size as usize,
        });

        // copy raw data
        if s.raw_data_pointer != 0 && s.raw_data_size > 0 {
            let start = s.raw_data_pointer as usize;
            let end = start + s.raw_data_size as usize;
            let src = raw
                .get(start..end)
                .ok_or_else(|| format!("Raw data out of bounds for '{}'", s.name))?;
            unsafe {
                ptr::copy_nonoverlapping(src.as_ptr(), base, s.raw_data_size as usize);
            }
        }
    }

    // ---- Step 2: resolve symbols ----
    let mut syms: Vec<SymbolMem> = object
        .symbols
        .iter()
        .map(|s| SymbolMem {
            name: s.name.clone(),
            section_number: s.section_number,
            value: s.value,
            storage_class: s.storage_class,
            in_memory_address: 0,
            got_address: None,
        })
        .collect();

    // GOT: preallocated, same as C loader (2048 bytes).
    let got = alloc_got(2048)?;
    let mut got_index: usize = 0;

    if develop_mode {
        println!(
            "[loader] GOT allocated at {:p} ({} bytes)",
            got.base, got.size
        );
    }

    // let mut launch_go: Option<extern "C" fn()> = None;
    let mut launch_go: Option<unsafe extern "C" fn(*const c_char, i32) -> i32> = None;

    for i in 0..syms.len() {
        // skip IMAGE_SYM_ABSOLUTE / DEBUG (>0xff)
        if syms[i].section_number as i32 > 0xff {
            syms[i].in_memory_address = 0;
            continue;
        }

        // skip IMAGE_SYM_UNDEFINED + storage_class 0 (matches C)
        if syms[i].section_number == 0 && syms[i].storage_class == 0 {
            syms[i].in_memory_address = 0;
            continue;
        }

        if syms[i].name.contains(TOKEN_IMP) {
            let (dll, func) = split_import_name(&syms[i].name);
            let dll_c = CString::new(dll.clone())
                .map_err(|_| format!("Bad DLL name: {}", dll))?;
            let func_c = CString::new(func.clone())
                .map_err(|_| format!("Bad function name: {}", func))?;

            let lib = unsafe { LoadLibraryA(dll_c.as_ptr()) };
            if !lib.is_null() {
                let proc = unsafe { GetProcAddress(lib, func_c.as_ptr()) };
                let addr = proc as u64;
                syms[i].in_memory_address = addr;

                if got_index * 8 + 8 <= got.size {
                    unsafe {
                        ptr::write_unaligned(got.base.add(got_index * 8) as *mut u64, addr);
                    }
                    syms[i].got_address =
                        Some(got.base as u64 + (got_index as u64) * 8);
                    got_index += 1;
                }
            }

            if develop_mode {
                if syms[i].in_memory_address != 0 {
                    println!(
                        "[loader] import {}!{} -> {:X}",
                        dll, func, syms[i].in_memory_address
                    );
                } else {
                    println!("[loader] import {}!{} -> UNRESOLVED", dll, func);
                }
            }
        } else {
            // defined symbol: section_number - 1
            let sec_idx = (syms[i].section_number - 1) as usize;
            if let Some(sec) = sections.iter().find(|s| s.section_index == sec_idx) {
                syms[i].in_memory_address = sec.base as u64 + syms[i].value as u64;

                if develop_mode {
                    println!(
                        "[loader] defined {} -> {:X}",
                        syms[i].name, syms[i].in_memory_address
                    );
                }

                if syms[i].name == "go" {
                    launch_go = Some(unsafe {
                        std::mem::transmute::<
                            u64,
                            unsafe extern "C" fn(*const c_char, i32) -> i32,
                        >(syms[i].in_memory_address)
                    });
                }
            }
        }
    }

    // ---- Step 3: apply relocations ----
    let mut section_index_by_name: HashMap<&str, usize> = HashMap::new();
    for (i, s) in object.sections.iter().enumerate() {
        section_index_by_name.insert(s.name.as_str(), i);
    }

    for reloc in &object.relocations {
        if develop_mode {
            println!(
                "[loader] reloc '{}' @ +0x{:X} type=0x{:X} sym='{}'",
                reloc.section_name,
                reloc.virtual_address,
                reloc.relocation_type,
                reloc.symbol_name
            );
        }

        apply_relocation_inner(
            &section_index_by_name,
            &sections,
            &syms,
            reloc,
        )?;
    }




    // ---- Step 4: invoke ----
    let result = match launch_go {
        Some(f) => {
            if develop_mode {
                println!("[loader] invoking entry point at {:p}", f as *const ());
            }
            unsafe { f(args.as_ptr(), args.as_bytes().len() as i32) };
            Ok(())
        }
        None => Err("Entry symbol `go` not found".to_string()),
    };

    // ---- Step 5: free (matches C behavior; only safe because single-threaded) ----
    for s in &sections {
        unsafe {
            VirtualFree(s.base as *mut c_void, 0, MEM_RELEASE);
        }
    }
    unsafe {
        VirtualFree(got.base as *mut c_void, 0, MEM_RELEASE);
    }

    result
}

// ---------- helpers ----------

fn alloc_section(s: &CoffSection) -> Result<*mut u8, String> {
    let sz = s.raw_data_size as usize;
    // Match C formula exactly: size + (PAGE - size % PAGE)
    let aligned = sz + (PAGE_SIZE - sz % PAGE_SIZE);
    let prot = if s.characteristics & IMAGE_SCN_CNT_CODE != 0 {
        PAGE_EXECUTE_READWRITE
    } else {
        PAGE_READWRITE
    };
    let p = unsafe {
        VirtualAlloc(
            ptr::null_mut(),
            aligned,
            MEM_COMMIT | MEM_TOP_DOWN,
            prot,
        )
    };
    if p.is_null() {
        return Err(format!("VirtualAlloc failed for section '{}'", s.name));
    }
    Ok(p as *mut u8)
}

fn alloc_got(size: usize) -> Result<GotMem, String> {
    let p = unsafe {
        VirtualAlloc(
            ptr::null_mut(),
            size,
            MEM_COMMIT | MEM_RESERVE | MEM_TOP_DOWN,
            PAGE_READWRITE,
        )
    };
    if p.is_null() {
        return Err("VirtualAlloc failed for GOT".into());
    }
    Ok(GotMem {
        base: p as *mut u8,
        size,
    })
}

fn split_import_name(name: &str) -> (String, String) {
    let after = name.strip_prefix(TOKEN_IMP).unwrap_or(name);
    if let Some((dll, func)) = after.split_once('$') {
        if !dll.is_empty() && !func.is_empty() {
            return (dll.to_string(), func.to_string());
        }
    }
    ("kernel32".to_string(), after.to_string())
}

fn apply_relocation_inner(
    section_index_by_name: &HashMap<&str, usize>,
    sections: &[SectionMem],
    syms: &[SymbolMem],
    reloc: &crate::coff::CoffRelocation,
) -> Result<(), String> {
    let sec_idx = *section_index_by_name
        .get(reloc.section_name.as_str())
        .ok_or_else(|| {
            format!("relocation references unknown section '{}'", reloc.section_name)
        })?;

    let sec_mem = sections
        .iter()
        .find(|s| s.section_index == sec_idx)
        .ok_or_else(|| {
            format!(
                "section '{}' has no allocated memory (raw_data_size=0?)",
                reloc.section_name
            )
        })?;

    let sym = syms
        .iter()
        .find(|s| s.name == reloc.symbol_name)
        .ok_or_else(|| {
            format!("relocation references unknown symbol '{}'", reloc.symbol_name)
        })?;

    let where_ptr = unsafe { sec_mem.base.add(reloc.virtual_address as usize) };

    match RelocType::from(reloc.relocation_type) {
        RelocType::Addr64 => {
            // C reads a 32-bit offset here, then writes a 64-bit value.
            let offset64 = unsafe { ptr::read_unaligned(where_ptr as *const i32) } as i64;
            let what64 = sym.in_memory_address.wrapping_add(offset64 as u64);
            unsafe {
                ptr::write_unaligned(where_ptr as *mut u64, what64);
            }
        }
        RelocType::Addr32Nb => {
            let offset32 = unsafe { ptr::read_unaligned(where_ptr as *const i32) };
            let what32 = (offset32 as i64)
                .wrapping_add(sym.in_memory_address as i64)
                .wrapping_sub(where_ptr as i64 + 4);
            unsafe {
                ptr::write_unaligned(where_ptr as *mut u32, what32 as u32);
            }
        }
        RelocType::Rel32 => {
            let offset32 = unsafe { ptr::read_unaligned(where_ptr as *const i32) };
            let what32 = if let Some(got_addr) = sym.got_address {
                (got_addr as i64).wrapping_sub(where_ptr as i64 + 4) as i32
            } else {
                (offset32 as i64)
                    .wrapping_add(sym.in_memory_address as i64)
                    .wrapping_sub(where_ptr as i64 + 4) as i32
            };
            unsafe {
                ptr::write_unaligned(where_ptr as *mut u32, what32 as u32);
            }
        }
        RelocType::Rel32_4 => {
            let offset32 = unsafe { ptr::read_unaligned(where_ptr as *const i32) };
            let what32 = (offset32 as i64)
                .wrapping_add(sym.in_memory_address as i64)
                .wrapping_sub(where_ptr as i64 + 4 + 4);
            unsafe {
                ptr::write_unaligned(where_ptr as *mut u32, what32 as u32);
            }
        }
        other => {
            return Err(format!("unsupported relocation type: {:?}", other));
        }
    }
    Ok(())
}