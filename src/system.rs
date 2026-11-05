//! Platform layer: memory statistics and actual memory reduction.
//!
//! Windows: `GlobalMemoryStatusEx` for stats, `EmptyWorkingSet` + documented
//! `NtSetSystemInformation` memory-list commands for reduction.
//! Linux: `/proc/meminfo` for stats, `/proc/sys/vm/drop_caches` for reduction.

use platform::{clean_platform, is_elevated_platform, poll_stats_platform};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MemoryStats {
    pub total_phys: u64,
    pub avail_phys: u64,
    pub total_commit: u64,
    pub avail_commit: u64,
    pub load_percent: u32, // 0..=100, physical memory
}

impl MemoryStats {
    pub fn used_phys(&self) -> u64 {
        self.total_phys.saturating_sub(self.avail_phys)
    }
    pub fn used_commit(&self) -> u64 {
        self.total_commit.saturating_sub(self.avail_commit)
    }
}

#[derive(Clone, Debug, Default)]
pub struct CleanReport {
    pub processes_checked: u32,
    pub processes_reduced: u32,
    pub system_ok: Vec<String>,
    pub system_fail: Vec<String>,
}

/// Which memory layers to clean.
#[derive(Clone, Copy, Debug)]
pub struct CleanOptions {
    /// Reduce the working set of every accessible process.
    pub working_sets: bool,
    /// Reduce working sets of all processes at once (needs admin on Windows).
    pub system_working_sets: bool,
    /// Purge the standby list / trim page cache (needs admin on Windows, root on Linux).
    pub standby: bool,
}

impl Default for CleanOptions {
    fn default() -> Self {
        Self {
            working_sets: true,
            system_working_sets: false,
            standby: false,
        }
    }
}

pub fn poll_stats() -> MemoryStats {
    poll_stats_platform()
}

pub fn is_elevated() -> bool {
    is_elevated_platform()
}

pub fn clean(options: &CleanOptions, report: &mut CleanReport) {
    clean_platform(options, report);
}

#[cfg(windows)]
mod platform {
    use super::*;

    use std::ffi::c_void;

    pub type HANDLE = *mut c_void;

    const INVALID_HANDLE_VALUE: HANDLE = usize::MAX as *mut c_void;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const PROCESS_SET_QUOTA: u32 = 0x0100;
    const TH32CS_SNAPPROCESS: u32 = 0x2;
    const SYSTEM_MEMORY_LIST_INFORMATION: u32 = 80;
    const MEMORY_EMPTY_WORKING_SETS: u32 = 3;
    const MEMORY_PURGE_STANDBY_LIST: u32 = 4;
    const TOKEN_QUERY: u32 = 0x0008;
    const TOKEN_ELEVATION: i32 = 20;

    #[repr(C)]
    struct MEMORYSTATUSEX {
        dw_length: u32,
        dw_memory_load: u32,
        ull_total_phys: u64,
        ull_avail_phys: u64,
        ull_total_page_file: u64,
        ull_avail_page_file: u64,
        ull_total_virtual: u64,
        ull_avail_virtual: u64,
        ull_avail_extended_virtual: u64,
    }

    #[repr(C)]
    struct PROCESSENTRY32W {
        dw_size: u32,
        cnt_usage: u32,
        th32_process_id: u32,
        th32_default_heap_id: usize,
        th32_module_id: u32,
        cnt_threads: u32,
        th32_parent_process_id: u32,
        pc_pri_class_base: i32,
        dw_flags: u32,
        sz_exe_file: [u16; 260],
    }

    #[repr(C)]
    struct TOKEN_ELEVATION {
        level: u32,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalMemoryStatusEx(buf: *mut MEMORYSTATUSEX) -> i32;
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> HANDLE;
        fn Process32FirstW(snap: HANDLE, entry: *mut PROCESSENTRY32W) -> i32;
        fn Process32NextW(snap: HANDLE, entry: *mut PROCESSENTRY32W) -> i32;
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> HANDLE;
        fn CloseHandle(handle: HANDLE) -> i32;
        fn K32EmptyWorkingSet(process: HANDLE) -> i32;
        fn GetCurrentProcess() -> HANDLE;
        fn LoadLibraryA(name: *const i8) -> HANDLE;
        fn GetProcAddress(module: HANDLE, name: *const i8) -> *mut c_void;
    }

    #[link(name = "advapi32")]
    extern "system" {
        fn OpenProcessToken(process: HANDLE, access: u32, token: *mut HANDLE) -> i32;
        fn GetTokenInformation(
            token: HANDLE,
            class: i32,
            info: *mut c_void,
            len: u32,
            len_out: *mut u32,
        ) -> i32;
    }

    type NtSetSystemInformationFn =
        unsafe extern "system" fn(class: u32, info: *mut c_void, length: u32) -> i32;

    pub fn poll_stats_platform() -> MemoryStats {
        let mut status = MEMORYSTATUSEX {
            dw_length: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
            ..unsafe { std::mem::zeroed() }
        };
        let ok = unsafe { GlobalMemoryStatusEx(&mut status) } != 0;
        if !ok {
            return MemoryStats::default();
        }
        let total = status.ull_total_phys;
        let avail = status.ull_avail_phys;
        let total_commit = status.ull_total_page_file;
        let avail_commit = status.ull_avail_page_file;
        let used = total.saturating_sub(avail);
        MemoryStats {
            total_phys: total,
            avail_phys: avail,
            total_commit,
            avail_commit,
            load_percent: percent(used, total),
        }
    }

    fn percent(used: u64, total: u64) -> u32 {
        if total == 0 {
            0
        } else {
            ((used as f64 / total as f64) * 100.0).round() as u32
        }
    }

    pub fn is_elevated_platform() -> bool {
        unsafe {
            let mut token: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return false;
            }
            let mut elevation = TOKEN_ELEVATION { level: 0 };
            let mut len_out: u32 = 0;
            let ok = GetTokenInformation(
                token,
                TOKEN_ELEVATION,
                &mut elevation as *mut TOKEN_ELEVATION as *mut c_void,
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut len_out,
            );
            CloseHandle(token);
            ok != 0 && elevation.level == 2
        }
    }

    /// Looks up `NtSetSystemInformation` at runtime so we never depend on
    /// import libraries for ntdll.
    unsafe fn nt_set_system_information() -> Option<NtSetSystemInformationFn> {
        let module = LoadLibraryA(b"ntdll.dll\0".as_ptr() as *const i8);
        if module.is_null() {
            return None;
        }
        let addr = GetProcAddress(
            module,
            b"NtSetSystemInformation\0".as_ptr() as *const i8,
        );
        if addr.is_null() {
            return None;
        }
        Some(std::mem::transmute::<*mut c_void, NtSetSystemInformationFn>(addr))
    }

    unsafe fn set_memory_list_command(
        command: u32,
        report: &mut CleanReport,
        name: &str,
    ) -> bool {
        if let Some(f) = nt_set_system_information() {
            let ok = f(SYSTEM_MEMORY_LIST_INFORMATION, &command as *const u32 as *mut c_void, 4) == 0;
            if ok {
                report.system_ok.push(name.to_owned());
            } else {
                report.system_fail.push(format!(
                    "{name}: rejected (needs elevated access)"
                ));
            }
            ok
        } else {
            report.system_fail.push(format!("{name}: not available"));
            false
        }
    }

    pub fn clean_platform(options: &CleanOptions, report: &mut CleanReport) {
        if options.standby {
            unsafe {
                set_memory_list_command(MEMORY_PURGE_STANDBY_LIST, report, "Standby list purged");
            }
        }
        if options.system_working_sets {
            unsafe {
                set_memory_list_command(
                    MEMORY_EMPTY_WORKING_SETS,
                    report,
                    "System working sets emptied",
                );
            }
        }
        if options.working_sets {
            unsafe {
                reduce_all_working_sets(report);
            }
        }
    }

    unsafe fn reduce_all_working_sets(report: &mut CleanReport) {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            report
                .system_fail
                .push("Working sets: could not list processes".to_owned());
            return;
        }

        let mut entry = PROCESSENTRY32W {
            dw_size: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..std::mem::zeroed()
        };
        if Process32FirstW(snap, &mut entry) == 0 {
            CloseHandle(snap);
            report
                .system_fail
                .push("Working sets: could not read processes".to_owned());
            return;
        }

        loop {
            let pid = entry.th32_process_id;
            report.processes_checked += 1;
            if pid != 0 {
                if let Some(_name) = utf8_lossy(&entry.sz_exe_file) {
                    let process = OpenProcess(
                        PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_QUOTA,
                        0,
                        pid,
                    );
                    if !process.is_null() {
                        if K32EmptyWorkingSet(process) != 0 {
                            report.processes_reduced += 1;
                        }
                        CloseHandle(process);
                    }
                }
            }
            if Process32NextW(snap, &mut entry) == 0 {
                break;
            }
        }
        CloseHandle(snap);
    }

    fn utf8_lossy(buf: &[u16]) -> Option<String> {
        if buf.is_empty() {
            return None;
        }
        let mut chars = Vec::new();
        for &unit in buf {
            if unit == 0 {
                break;
            }
            chars.push(unit);
        }
        if chars.is_empty() {
            None
        } else {
            Some(String::from_utf16_lossy(&chars))
        }
    }
}

#[cfg(unix)]
mod platform {
    use super::*;

    pub fn poll_stats_platform() -> MemoryStats {
        let Ok(info) = std::fs::read_to_string("/proc/meminfo") else {
            return MemoryStats::default();
        };

        let mut meminfo: std::collections::HashMap<&str, u64> =
            std::collections::HashMap::new();
        for line in info.lines() {
            if let Some((key, rest)) = line.split_once(':') {
                let value: u64 = rest
                    .split_whitespace()
                    .next()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
                // /proc/meminfo values are KiB.
                meminfo.insert(key.trim(), value.saturating_mul(1024));
            }
        }

        let total = *meminfo.get("MemTotal").unwrap_or(&0);
        let avail = *meminfo.get("MemAvailable").unwrap_or(&0);
        let swap_total = *meminfo.get("SwapTotal").unwrap_or(&0);
        let swap_free = *meminfo.get("SwapFree").unwrap_or(&0);
        let used = total.saturating_sub(avail);
        MemoryStats {
            total_phys: total,
            avail_phys: avail,
            total_commit: swap_total,
            avail_commit: swap_free,
            load_percent: percent(used, total),
        }
    }

    fn percent(used: u64, total: u64) -> u32 {
        if total == 0 {
            0
        } else {
            ((used as f64 / total as f64) * 100.0).round() as u32
        }
    }

    pub fn is_elevated_platform() -> bool {
        // Unix: effective uid 0.
        #[cfg(target_os = "linux")]
        {
            libc_geteuid()
        }
        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }

    #[cfg(target_os = "linux")]
    fn libc_geteuid() -> bool {
        // geteuid returns uid_t = u32.
        unsafe extern "C" {
            fn geteuid() -> u32;
        }
        unsafe { geteuid() == 0 }
    }

    pub fn clean_platform(options: &CleanOptions, report: &mut CleanReport) {
        if options.working_sets {
            // On Linux the closest equivalent is trimming the page cache,
            // which the kernel performs as part of drop_caches.
            report.processes_checked = 0;
        }
        if options.standby || options.working_sets {
            match drop_caches() {
                Ok(()) => report
                    .system_ok
                    .push("Page cache, dentries and inodes dropped".to_owned()),
                Err(e) => report
                    .system_fail
                    .push(format!("drop_caches failed: {e} (root required?)")),
            }
        }
        if options.system_working_sets {
            report
                .system_fail
                .push("System working sets: Windows-only layer, skipped".to_owned());
        }
    }

    fn drop_caches() -> Result<(), String> {
        use std::io::Write;
        let mut sync = std::process::Command::new("sync")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        if sync.is_err() {
            sync = Err(String::from("sync not found"));
        }
        let _ = sync;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .open("/proc/sys/vm/drop_caches")
            .map_err(|e| e.to_string())?;
        file.write_all(b"3\n").map_err(|e| e.to_string())
    }
}
