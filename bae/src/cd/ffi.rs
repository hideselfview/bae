//! FFI bindings for libcdio-paranoia
//!
//! Safe wrappers around libcdio-sys for CD drive access and audio extraction
//!
//! Requires libcdio system library to be installed:
//! - macOS: `brew install libcdio`
//! - Linux: `apt-get install libcdio-dev` or `dnf install libcdio-devel`
//! - Windows: Install libcdio from source or use vcpkg

use libcdio_sys;
use std::ffi::{CStr, CString};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LibcdioError {
    #[error("libcdio error: {0}")]
    Libcdio(String),
    #[error("Invalid device path")]
    InvalidPath,
}

/// Safe wrapper for libcdio drive
pub struct LibcdioDrive {
    device: *mut libcdio_sys::CdIo_t,
}

unsafe impl Send for LibcdioDrive {}

impl LibcdioDrive {
    /// Open a CD drive by device path
    pub fn open(device_path: &Path) -> Result<Self, LibcdioError> {
        let path_str = device_path.to_str().ok_or(LibcdioError::InvalidPath)?;
        let c_path = CString::new(path_str).map_err(|_| LibcdioError::InvalidPath)?;

        unsafe {
            let device =
                libcdio_sys::cdio_open(c_path.as_ptr(), libcdio_sys::driver_id_t_DRIVER_DEVICE);
            if device.is_null() {
                // On macOS, permission errors are common without Full Disk Access
                #[cfg(target_os = "macos")]
                {
                    return Err(LibcdioError::Libcdio(format!(
                        "Failed to open device: {} (may need Full Disk Access permission in System Settings)",
                        path_str
                    )));
                }
                #[cfg(not(target_os = "macos"))]
                {
                    return Err(LibcdioError::Libcdio(format!(
                        "Failed to open device: {}",
                        path_str
                    )));
                }
            }

            Ok(Self { device })
        }
    }

    /// Check if a disc is present
    pub fn has_disc(&self) -> bool {
        unsafe {
            libcdio_sys::cdio_get_discmode(self.device)
                != libcdio_sys::discmode_t_CDIO_DISC_MODE_NO_INFO
        }
    }

    /// Get first track number (usually 1)
    pub fn first_track_num(&self) -> Result<u8, LibcdioError> {
        unsafe {
            let first = libcdio_sys::cdio_get_first_track_num(self.device) as i32;
            if first < 0 {
                return Err(LibcdioError::Libcdio(
                    "Failed to get first track".to_string(),
                ));
            }
            Ok(first as u8)
        }
    }

    /// Get last track number
    pub fn last_track_num(&self) -> Result<u8, LibcdioError> {
        unsafe {
            let last = libcdio_sys::cdio_get_last_track_num(self.device) as i32;
            if last < 0 {
                return Err(LibcdioError::Libcdio(
                    "Failed to get last track".to_string(),
                ));
            }
            Ok(last as u8)
        }
    }

    /// Get track start LBA (Logical Block Address) for a track
    pub fn track_start_lba(&self, track_num: u8) -> Result<u32, LibcdioError> {
        unsafe {
            let lba =
                libcdio_sys::cdio_get_track_lba(self.device, track_num as libcdio_sys::track_t);
            if lba < 0 {
                return Err(LibcdioError::Libcdio(format!(
                    "Failed to get LBA for track {}",
                    track_num
                )));
            }
            Ok(lba as u32)
        }
    }

    /// Get leadout LBA
    pub fn leadout_lba(&self) -> Result<u32, LibcdioError> {
        unsafe {
            // Leadout track is track 0xAA (170 decimal)
            let lba = libcdio_sys::cdio_get_track_lba(
                self.device,
                libcdio_sys::cdio_track_enums_CDIO_CDROM_LEADOUT_TRACK as libcdio_sys::track_t,
            );
            if lba < 0 {
                return Err(LibcdioError::Libcdio(
                    "Failed to get leadout LBA".to_string(),
                ));
            }
            Ok(lba as u32)
        }
    }

    /// Get the raw device pointer (for advanced operations)
    pub fn device_ptr(&self) -> *mut libcdio_sys::CdIo_t {
        self.device
    }
}

impl Drop for LibcdioDrive {
    fn drop(&mut self) {
        unsafe {
            if !self.device.is_null() {
                libcdio_sys::cdio_destroy(self.device);
            }
        }
    }
}

/// Detect available CD drives
pub fn detect_drives() -> Result<Vec<PathBuf>, LibcdioError> {
    unsafe {
        let mut drives = Vec::new();

        // Try to get default device first
        let mut driver_id = libcdio_sys::driver_id_t_DRIVER_DEVICE;
        let default_device = libcdio_sys::cdio_get_default_device_driver(&mut driver_id);
        if !default_device.is_null() {
            let path = CStr::from_ptr(default_device).to_string_lossy().to_string();
            drives.push(PathBuf::from(path));
        }

        // On Unix systems, common CD device paths
        #[cfg(unix)]
        {
            let common_paths = [
                "/dev/cdrom",
                "/dev/sr0",
                "/dev/sr1",
                "/dev/cdrom0",
                "/dev/cdrom1",
            ];

            for path_str in &common_paths {
                let path = PathBuf::from(*path_str);
                if path.exists() {
                    // Try to open it to verify it's a CD drive
                    if LibcdioDrive::open(&path).is_ok() {
                        // Accept drive even if no disc is present
                        if !drives.contains(&path) {
                            drives.push(path);
                        }
                    }
                }
            }
        }

        #[cfg(target_os = "macos")]
        {
            // macOS uses /dev/rdisk* (raw disk) for CD drives
            // Prefer rdisk* over disk* as raw devices are required for CD access
            // Filter out partition devices (rdisk8s1, rdisk8s2, etc.) - only try base devices
            use std::fs;
            if let Ok(entries) = fs::read_dir("/dev") {
                let mut rdisk_paths = Vec::new();
                let mut disk_paths = Vec::new();

                for entry in entries.flatten() {
                    let path = entry.path();
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        // Prefer rdisk* devices (raw disk), but exclude partitions (rdisk8s1, etc.)
                        if name.starts_with("rdisk") && name.len() > 5 && !name.contains('s') {
                            rdisk_paths.push(path);
                        } else if name.starts_with("disk") && name.len() > 4 && !name.contains('s')
                        {
                            disk_paths.push(path);
                        }
                    }
                }

                // Try rdisk* first (raw devices work better for CD access)
                for path in rdisk_paths {
                    // Silently try to open - permission errors are expected without Full Disk Access
                    if LibcdioDrive::open(&path).is_ok() {
                        if !drives.contains(&path) {
                            drives.push(path);
                        }
                    }
                }

                // Fall back to disk* if no rdisk* devices worked
                if drives.is_empty() {
                    for path in disk_paths {
                        if LibcdioDrive::open(&path).is_ok() {
                            if !drives.contains(&path) {
                                drives.push(path);
                            }
                        }
                    }
                }
            }
        }

        Ok(drives)
    }
}
