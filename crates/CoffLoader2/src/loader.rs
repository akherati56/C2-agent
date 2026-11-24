//! COFF loader: allocates section memory, resolves symbols, applies
//! relocations, and invokes the entry point (`go`).
//!
//! Faithful port of the C reference (`LoadCOFF` / `ResolveSymbols`),
//! with corrections for MSVC `__imp_?<DLL>$<decorated>` import symbols
//! and for the `ADDR32NB` / `ADDR64` relocation semantics.

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

    fn VirtualFree(lpAddress: *mut c_void, dwSize: usize, dwFreeType: u32) -> i32;

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
) -> Result<(), String> {
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

        // Copy raw data (skip for .bss where raw_data_pointer == 0;
        // VirtualAlloc already zero-filled the page).
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

    // GOT: preallocated, same as C loader (2048 bytes = 256 slots).
    let got = alloc_got(2048)?;
    let mut got_index: usize = 0;

    if develop_mode {
        println!(
            "[loader] GOT allocated at {:p} ({} bytes)",
            got.base, got.size
        );
    }

    // Cached DLL handles so we don't reload the same library per import.
    let mut dll_cache: HashMap<String, HMODULE> = HashMap::new();

    let mut launch_go: Option<unsafe extern "C" fn(*const c_char, i32) -> i32> = None;

    for i in 0..syms.len() {
        // Skip IMAGE_SYM_ABSOLUTE / DEBUG / special (>0xff).
        if syms[i].section_number as i32 > 0xff {
            syms[i].in_memory_address = 0;
            continue;
        }

        // Skip truly-null entries.
        if syms[i].section_number == 0 && syms[i].storage_class == 0 {
            syms[i].in_memory_address = 0;
            continue;
        }

        if syms[i].name.contains(TOKEN_IMP) {
            // ---- Imported symbol (__imp_?DLL$FUNC@@...) ----
            let (dll, func) = split_import_name(&syms[i].name);

            if dll.is_empty() || func.is_empty() {
                if develop_mode {
                    println!(
                        "[loader] import {} -> UNRESOLVED (bad symbol format)",
                        syms[i].name
                    );
                }
                continue;
            }

            let dll_c = CString::new(dll.clone())
                .map_err(|_| format!("Bad DLL name (contains NUL): {}", dll))?;
            let func_c = CString::new(func.clone())
                .map_err(|_| format!("Bad function name (contains NUL): {}", func))?;

            // Load DLL (cached).
            let lib = if let Some(&h) = dll_cache.get(&dll) {
                h
            } else {
                let h = unsafe { LoadLibraryA(dll_c.as_ptr()) };
                if h.is_null() {
                    // Fallback: try ucrtbase if msvcrt failed, and vice-versa.
                    let alt = match dll.as_str() {
                        "msvcrt.dll" => Some("ucrtbase.dll"),
                        "ucrtbase.dll" => Some("msvcrt.dll"),
                        _ => None,
                    };
                    if let Some(alt_name) = alt {
                        if let Ok(alt_c) = CString::new(alt_name) {
                            let alt_h = unsafe { LoadLibraryA(alt_c.as_ptr()) };
                            if !alt_h.is_null() {
                                if develop_mode {
                                    println!(
                                        "[loader] {} missing, fell back to {}",
                                        dll, alt_name
                                    );
                                }
                                dll_cache.insert(dll.clone(), alt_h);
                                // Continue with alt_h for this symbol; also
                                // remember under the alt name for the future.
                                dll_cache.insert(alt_name.to_string(), alt_h);
                                alt_h
                            } else {
                                null_module()
                            }
                        } else {
                            null_module()
                        }
                    } else {
                        null_module()
                    }
                } else {
                    dll_cache.insert(dll.clone(), h);
                    h
                }
            };

            if lib.is_null() {
                if develop_mode {
                    let err = std::io::Error::last_os_error();
                    println!(
                        "[loader] import {}!{} -> UNRESOLVED (LoadLibraryA failed: {})",
                        dll, func, err
                    );
                }
                continue;
            }

            let proc = unsafe { GetProcAddress(lib, func_c.as_ptr()) };
            if proc.is_null() {
                if develop_mode {
                    let err = std::io::Error::last_os_error();
                    println!(
                        "[loader] import {}!{} -> UNRESOLVED (GetProcAddress failed: {})",
                        dll, func, err
                    );
                }
                continue;
            }

            let addr = proc as u64;
            syms[i].in_memory_address = addr;

            // Allocate a GOT slot (one per imported symbol), same as C loader.
            if (got_index + 1) * 8 <= got.size {
                unsafe {
                    ptr::write_unaligned(got.base.add(got_index * 8) as *mut u64, addr);
                }
                syms[i].got_address = Some(got.base as u64 + (got_index as u64) * 8);
                got_index += 1;
            }

            if develop_mode {
                println!("[loader] import {}!{} -> {:X}", dll, func, addr);
            }
        } else {
            // ---- Defined symbol: section_number is 1-based ----
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
            } else if develop_mode {
                println!(
                    "[loader] defined {} -> SKIPPED (section {} not mapped)",
                    syms[i].name, sec_idx
                );
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

        if let Err(e) = apply_relocation_inner(&section_index_by_name, &sections, &syms, reloc) {
            if develop_mode {
                println!("[loader] relocation failed: {}", e);
            }
            return Err(e);
        }
    }

    // ---- Step 4: invoke ----
    let result = match launch_go {
        Some(f) => {
            if develop_mode {
                println!("[loader] invoking entry point at {:p}", f as *const ());
            }
            unsafe {
                f(args.as_ptr(), args.as_bytes().len() as i32);
            }
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

fn null_module() -> HMODULE {
    ptr::null_mut()
}

fn alloc_section(s: &CoffSection) -> Result<*mut u8, String> {
    let sz = s.raw_data_size as usize;
    // Match C formula: size + (PAGE - size % PAGE). Note: rounds up to the
    // next page (and adds an extra page when size is already page-aligned);
    // harmless over-allocation.
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
        return Err(format!(
            "VirtualAlloc failed for section '{}' ({} bytes)",
            s.name, aligned
        ));
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

/// Parse an MSVC `__imp_` symbol into `(dll_name, export_name)`.
///
/// Formats handled:
///   `__imp_?KERNEL32$VirtualAlloc@@YAPEAXPEAX_KKK@Z`
///       -> ("kernel32.dll", "VirtualAlloc")
///   `__imp_?MSVCRT$printf@@YAHPEBDZZ`
///       -> ("msvcrt.dll",   "printf")
///   `__imp_?SHLWAPI$StrStrIW@@YAPEA_WPEB_W0@Z`
///       -> ("shlwapi.dll",  "StrStrIW")
///   `__imp_?KERNEL32$GetProcAddress@@YAP6A_JXZPEAUHINSTANCE__@@PEBD@Z`
///       -> ("kernel32.dll", "GetProcAddress")
///
/// Anything unrecognised -> ("", "") so the caller marks it UNRESOLVED.
fn split_import_name(name: &str) -> (String, String) {
    let after = match name.strip_prefix(TOKEN_IMP) {
        Some(a) => a,
        None => return (String::new(), String::new()),
    };

    // Strip the leading '?' that prefixes the DLL name.
    let after = after.strip_prefix('?').unwrap_or(after);

    // Split DLL from decorated function name.
    let (dll, decorated) = match after.split_once('$') {
        Some((d, f)) => (d, f),
        None => return (String::new(), String::new()),
    };

    if dll.is_empty() || decorated.is_empty() {
        return (String::new(), String::new());
    }

    // Real export name = decorated up to the first '@'.
    let func = match decorated.split('@').next() {
        Some(f) if !f.is_empty() => f.to_string(),
        _ => return (String::new(), String::new()),
    };

    // Normalise DLL name (lowercase + ".dll").
    let mut dll_name = dll.to_ascii_lowercase();
    if !dll_name.ends_with(".dll") {
        dll_name.push_str(".dll");
    }

    (dll_name, func)
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
            format!(
                "relocation references unknown section '{}'",
                reloc.section_name
            )
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
            format!(
                "relocation references unknown symbol '{}'",
                reloc.symbol_name
            )
        })?;

    // Bounds check: every write below is 4 or 8 bytes wide.
    let va = reloc.virtual_address as usize;
    let rtype = RelocType::from(reloc.relocation_type);

    let need: usize = match rtype {
        // 8-byte writes
        RelocType::Addr64 => 8,
        // 4-byte writes
        RelocType::Addr32
        | RelocType::Addr32Nb
        | RelocType::Rel32
        | RelocType::Rel32_1
        | RelocType::Rel32_2
        | RelocType::Rel32_3
        | RelocType::Rel32_4
        | RelocType::Rel32_5
        | RelocType::Section
        | RelocType::SecRel => 4,
        // no write
        RelocType::Absolute => 0,
        other => {
            return Err(format!(
                "unsupported relocation type 0x{:X} ({:?}) for symbol '{}'",
                reloc.relocation_type, other, reloc.symbol_name
            ));
        }
    };

    if va > sec_mem.size || need > sec_mem.size - va {
        return Err(format!(
            "relocation for '{}' at +0x{:X} ({} bytes) exceeds section '{}' (size 0x{:X})",
            reloc.symbol_name, reloc.virtual_address, need, reloc.section_name, sec_mem.size
        ));
    }

    let where_ptr = unsafe { sec_mem.base.add(va) };

    match rtype {
        // ---------- absolute ----------
        RelocType::Addr64 => {
            // ADDR64: S + A.  The addend is the 8-byte value already present
            // at the target slot (compiler initialises it, often to 0).
            let addend = unsafe { ptr::read_unaligned(where_ptr as *const i64) };
            let what = (addend as u64).wrapping_add(sym.in_memory_address);
            unsafe {
                ptr::write_unaligned(where_ptr as *mut u64, what);
            }
        }
        RelocType::Addr32 => {
            // ADDR32: S + A, 4-byte addend.
            let addend = unsafe { ptr::read_unaligned(where_ptr as *const i32) };
            let what = (addend as u64).wrapping_add(sym.in_memory_address) as u32;
            unsafe {
                ptr::write_unaligned(where_ptr as *mut u32, what);
            }
        }
        RelocType::Addr32Nb => {
            // ADDR32NB: S + A - ImageBase.
            //
            // For an in-memory COFF we define ImageBase = 0 (the sections
            // are not laid out as a contiguous PE image), so this degenerates
            // to S + A.  The .pdata entries written this way are never seen
            // by the Windows unwinder (we never call RtlAddFunctionTable),
            // so the truncated 32-bit absolute is harmless and consistent
            // with what the Cobalt-Strike-style COFF loaders do.
            let addend = unsafe { ptr::read_unaligned(where_ptr as *const i32) };
            let what = (addend as u64).wrapping_add(sym.in_memory_address) as u32;
            unsafe {
                ptr::write_unaligned(where_ptr as *mut u32, what);
            }
        }

        // ---------- REL32 family ----------
        // REL32_N => S + A - (P + 4 + N).
        // If the symbol lives in the GOT (import), we target the GOT slot
        // instead of the raw resolved address: MSVC compiles imported calls
        // to `call [rip+disp]`, where disp points at the __imp_ slot.
        RelocType::Rel32
        | RelocType::Rel32_1
        | RelocType::Rel32_2
        | RelocType::Rel32_3
        | RelocType::Rel32_4
        | RelocType::Rel32_5 => {
            let n: i64 = match rtype {
                RelocType::Rel32 => 0,
                RelocType::Rel32_1 => 1,
                RelocType::Rel32_2 => 2,
                RelocType::Rel32_3 => 3,
                RelocType::Rel32_4 => 4,
                RelocType::Rel32_5 => 5,
                _ => unreachable!(),
            };

            let addend = unsafe { ptr::read_unaligned(where_ptr as *const i32) };

            let target = sym.got_address.unwrap_or(sym.in_memory_address);
            let what = (addend as i64)
                .wrapping_add(target as i64)
                .wrapping_sub(where_ptr as i64 + 4 + n);

            unsafe {
                ptr::write_unaligned(where_ptr as *mut u32, what as u32);
            }
        }

        // ---------- section-relative ----------
        RelocType::Section => {
            // SECTION: S + A - SectionBase(S)
            // SectionBase(S) is the base of the section containing the symbol.
            let sym_sec_base = syms
                .iter()
                .find(|s| s.name == reloc.symbol_name)
                .and_then(|s| {
                    if s.section_number > 0 {
                        sections
                            .iter()
                            .find(|sm| sm.section_index == (s.section_number - 1) as usize)
                            .map(|sm| sm.base as i64)
                    } else {
                        None
                    }
                })
                .unwrap_or(0);

            let addend = unsafe { ptr::read_unaligned(where_ptr as *const i32) };
            let what = (addend as i64)
                .wrapping_add(sym.in_memory_address as i64)
                .wrapping_sub(sym_sec_base);
            unsafe {
                ptr::write_unaligned(where_ptr as *mut u32, what as u32);
            }
        }
        RelocType::SecRel => {
            // SECREL: S + A - SectionBase(S)
            // Semantically identical to SECTION for our purposes; kept
            // separate for clarity.
            let sym_sec_base = syms
                .iter()
                .find(|s| s.name == reloc.symbol_name)
                .and_then(|s| {
                    if s.section_number > 0 {
                        sections
                            .iter()
                            .find(|sm| sm.section_index == (s.section_number - 1) as usize)
                            .map(|sm| sm.base as i64)
                    } else {
                        None
                    }
                })
                .unwrap_or(0);

            let addend = unsafe { ptr::read_unaligned(where_ptr as *const i32) };
            let what = (addend as i64)
                .wrapping_add(sym.in_memory_address as i64)
                .wrapping_sub(sym_sec_base);
            unsafe {
                ptr::write_unaligned(where_ptr as *mut u32, what as u32);
            }
        }

        // ---------- no-op ----------
        RelocType::Absolute => { /* padding; nothing to do */ }

        other => {
            return Err(format!(
                "unsupported relocation type 0x{:X} ({:?}) for symbol '{}'",
                reloc.relocation_type, other, reloc.symbol_name
            ));
        }
    }
    Ok(())
}