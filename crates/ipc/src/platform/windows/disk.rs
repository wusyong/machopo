//! What is free on a volume, and how much memory the machine has.

use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

/// The bytes free on the volume `path` is on, for this user (Chromium's
/// `base::SysInfo::AmountOfFreeDiskSpace`): what `GetDiskFreeSpaceExW` says
/// is available to the caller, its quota counted.
pub fn available_disk_space(path: &Path) -> std::io::Result<u64> {
    let mut name: Vec<u16> = path.as_os_str().encode_wide().collect();
    // One with a 0 in it would be another, shorter path.
    if name.contains(&0) {
        return Err(std::io::ErrorKind::InvalidInput.into());
    }
    name.push(0);
    let mut available = 0u64;
    // SAFETY: `name` ends in its only 0 and lives until the call returns;
    // `available` is a local the call writes, which lives as long. The
    // other two outputs are null, which the call takes as not asked for.
    let done = unsafe {
        GetDiskFreeSpaceExW(
            name.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if done == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(available)
}

/// The machine's physical memory, in bytes (Chromium's
/// `base::SysInfo::AmountOfPhysicalMemory`): what `GlobalMemoryStatusEx`
/// says there is. `None` if it says nothing.
pub fn amount_of_physical_memory() -> Option<u64> {
    let mut status = windows_sys::Win32::System::SystemInformation::MEMORYSTATUSEX {
        dwLength: u32::try_from(std::mem::size_of::<
            windows_sys::Win32::System::SystemInformation::MEMORYSTATUSEX,
        >())
        .ok()?,
        dwMemoryLoad: 0,
        ullTotalPhys: 0,
        ullAvailPhys: 0,
        ullTotalPageFile: 0,
        ullAvailPageFile: 0,
        ullTotalVirtual: 0,
        ullAvailVirtual: 0,
        ullAvailExtendedVirtual: 0,
    };
    // SAFETY: `status` is a local the call writes, which lives until it
    // returns; its `dwLength` is its size, as the call asks.
    let done =
        unsafe { windows_sys::Win32::System::SystemInformation::GlobalMemoryStatusEx(&mut status) };
    (done != 0 && status.ullTotalPhys > 0).then_some(status.ullTotalPhys)
}
