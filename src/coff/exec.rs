// src/coff/exec.rs

use std::ffi::c_void;
use std::sync::Mutex;

use crate::coff::Section;


// ---------- Windows FFI ----------

#[link(name = "kernel32")]
unsafe extern "system" {
    fn VirtualAlloc(
        lp_address: *mut c_void,
        size: usize,
        allocation_type: u32,
        protect: u32,
    ) -> *mut c_void;

    fn VirtualFree(
        lp_address: *mut c_void,
        size: usize,
        free_type: u32,
    ) -> i32;

    fn VirtualProtect(
        lp_address: *mut c_void,
        size: usize,
        new_protect: u32,
        old_protect: *mut u32,
    ) -> i32;

    fn GetModuleHandleA(
        name: *const u8,
    ) -> *mut c_void;

    fn GetProcAddress(
        module: *mut c_void,
        name: *const u8,
    ) -> *mut c_void;
}


// ---------- Memory constants ----------

const MEM_COMMIT: u32 = 0x1000;
const MEM_RESERVE: u32 = 0x2000;
const MEM_RELEASE: u32 = 0x8000;

const PAGE_READWRITE: u32 = 0x04;
const PAGE_READONLY: u32 = 0x02;
const PAGE_EXECUTE_READ: u32 = 0x20;


// ---------- Section characteristics ----------

const IMAGE_SCN_MEM_EXECUTE: u32 = 0x2000_0000;
const IMAGE_SCN_MEM_READ: u32 = 0x4000_0000;
const IMAGE_SCN_MEM_WRITE: u32 = 0x8000_0000;


// ---------- Load sections ----------

pub fn load_sections(
    sections: &[Section],
) -> Result<Vec<u64>, String> {

    let mut bases =
        Vec::with_capacity(sections.len());

    for section in sections {

        let size =
            section
                .header
                .virtual_size
                .max(section.header.size_of_raw_data)
                .max(1) as usize;


        let base = unsafe {
            VirtualAlloc(
                std::ptr::null_mut(),
                size,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            )
        };


        if base.is_null() {
            return Err(format!(
                "VirtualAlloc failed for section {}",
                crate::coff::section_name(section)
            ));
        }


        if !section.data.is_empty() {

            unsafe {
                std::ptr::copy_nonoverlapping(
                    section.data.as_ptr(),
                    base as *mut u8,
                    section.data.len(),
                );
            }
        }


        bases.push(base as u64);
    }


    Ok(bases)
}


// ---------- Apply section protection ----------

pub fn apply_section_protection(
    sections: &[Section],
    bases: &[u64],
) -> Result<(), String> {

    if sections.len() != bases.len() {
        return Err(
            "sections/bases length mismatch".to_string()
        );
    }


    for (section, &base) in
        sections.iter().zip(bases.iter())
    {

        let size =
            section
                .header
                .virtual_size
                .max(section.header.size_of_raw_data)
                .max(1) as usize;


        let chars =
            section.header.characteristics;


        let protect =
            if chars & IMAGE_SCN_MEM_EXECUTE != 0 {
                PAGE_EXECUTE_READ
            } else if chars & IMAGE_SCN_MEM_WRITE != 0 {
                PAGE_READWRITE
            } else {
                PAGE_READONLY
            };


        let mut old_protect =
            0u32;


        let ok = unsafe {
            VirtualProtect(
                base as *mut c_void,
                size,
                protect,
                &mut old_protect,
            )
        };


        if ok == 0 {
            return Err(format!(
                "VirtualProtect failed for section {}",
                crate::coff::section_name(section)
            ));
        }
    }


    Ok(())
}


// ---------- BeaconOutput shim ----------

type OutputSink =
Box<dyn Fn(&str) + Send + Sync>;


static OUTPUT_SINK:
Mutex<Option<OutputSink>> =
    Mutex::new(None);


pub fn set_output_sink(
    f: OutputSink,
) {
    *OUTPUT_SINK
        .lock()
        .unwrap() = Some(f);
}


pub unsafe extern "system" fn beacon_output_shim(
    _kind: i32,
    data: *const u8,
    len: i32,
) {

    if data.is_null() || len <= 0 {
        return;
    }


    let bytes =
        std::slice::from_raw_parts(
            data,
            len as usize,
        );


    let text =
        String::from_utf8_lossy(bytes);


    if let Some(sink) =
        OUTPUT_SINK.lock().unwrap().as_ref()
    {
        sink(&text);
    }
}