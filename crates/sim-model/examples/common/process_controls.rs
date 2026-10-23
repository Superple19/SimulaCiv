// Standalone harnesses share this module but each uses only a subset of its helpers.
#![allow(dead_code)]

#[derive(Clone, Copy, Debug, Default)]
pub struct ApplyStatus {
    pub priority_set: bool,
    pub affinity_set: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct ProcessSettings {
    pub priority_class: u32,
    pub process_mask: usize,
    pub system_mask: usize,
}

#[cfg(windows)]
mod windows {
    use super::{ApplyStatus, ProcessSettings};
    use std::ffi::c_void;

    const HIGH_PRIORITY_CLASS: u32 = 0x0000_0080;
    const AFFINITY_MASK: usize = 0x555;

    #[repr(C)]
    struct ProcessMemoryCountersEx {
        size: u32,
        page_fault_count: u32,
        values: [usize; 9],
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
        fn SetPriorityClass(process: *mut c_void, priority_class: u32) -> i32;
        fn SetProcessAffinityMask(process: *mut c_void, mask: usize) -> i32;
        fn GetPriorityClass(process: *mut c_void) -> u32;
        fn GetProcessAffinityMask(
            process: *mut c_void,
            process_mask: *mut usize,
            system_mask: *mut usize,
        ) -> i32;
    }

    #[link(name = "Psapi")]
    unsafe extern "system" {
        fn GetProcessMemoryInfo(
            process: *mut c_void,
            counters: *mut ProcessMemoryCountersEx,
            size: u32,
        ) -> i32;
    }

    pub fn apply_high_priority_affinity() -> ApplyStatus {
        let process = unsafe { GetCurrentProcess() };
        ApplyStatus {
            priority_set: unsafe { SetPriorityClass(process, HIGH_PRIORITY_CLASS) } != 0,
            affinity_set: unsafe { SetProcessAffinityMask(process, AFFINITY_MASK) } != 0,
        }
    }

    pub fn current_settings() -> Option<ProcessSettings> {
        let process = unsafe { GetCurrentProcess() };
        let mut process_mask = 0_usize;
        let mut system_mask = 0_usize;
        if unsafe { GetProcessAffinityMask(process, &mut process_mask, &mut system_mask) } == 0 {
            return None;
        }
        let priority_class = unsafe { GetPriorityClass(process) };
        (priority_class != 0).then_some(ProcessSettings {
            priority_class,
            process_mask,
            system_mask,
        })
    }

    pub fn current_working_set_bytes() -> Option<usize> {
        let mut counters = ProcessMemoryCountersEx {
            size: std::mem::size_of::<ProcessMemoryCountersEx>() as u32,
            page_fault_count: 0,
            values: [0; 9],
        };
        let succeeded =
            unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.size) };
        (succeeded != 0).then_some(counters.values[1])
    }
}

pub fn apply_high_priority_affinity() -> ApplyStatus {
    #[cfg(windows)]
    {
        windows::apply_high_priority_affinity()
    }
    #[cfg(not(windows))]
    {
        ApplyStatus::default()
    }
}

pub fn current_settings() -> Option<ProcessSettings> {
    #[cfg(windows)]
    {
        windows::current_settings()
    }
    #[cfg(not(windows))]
    {
        None
    }
}

pub fn current_working_set_bytes() -> Option<usize> {
    #[cfg(windows)]
    {
        windows::current_working_set_bytes()
    }
    #[cfg(not(windows))]
    {
        None
    }
}
