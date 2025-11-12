//! FFI bindings for libcdio-paranoia
//!
//! Safe wrappers around libcdio-sys for CD drive access and audio extraction
//!
//! Requires libcdio system library to be installed:
//! - macOS: `brew install libcdio`
//! - Linux: `apt-get install libcdio-dev` or `dnf install libcdio-devel`
//! - Windows: Install libcdio from source or use vcpkg

use libc;
use libcdio_sys;
use std::ffi::{CStr, CString};
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LibcdioError {
    #[error("libcdio error: {0}")]
    Libcdio(String),
    #[error("Invalid device path")]
    InvalidPath,
    #[error("No disc in drive")]
    NoDisc,
    #[error("Read error: {0}")]
    Read(String),
}

/// Safe wrapper for libcdio drive
pub struct LibcdioDrive {
    device: *mut libcdio_sys::CdIo_t,
    device_path: PathBuf,
}

unsafe impl Send for LibcdioDrive {}

impl LibcdioDrive {
    /// Open a CD drive by device path
    pub fn open(device_path: &PathBuf) -> Result<Self, LibcdioError> {
        let path_str = device_path.to_str().ok_or(LibcdioError::InvalidPath)?;
        let c_path = CString::new(path_str).map_err(|_| LibcdioError::InvalidPath)?;

        unsafe {
            let device =
                libcdio_sys::cdio_open(c_path.as_ptr(), libcdio_sys::driver_id_t_DRIVER_DEVICE);
            if device.is_null() {
                return Err(LibcdioError::Libcdio(format!(
                    "Failed to open device: {}",
                    path_str
                )));
            }

            Ok(Self {
                device,
                device_path: device_path.clone(),
            })
        }
    }

    /// Get the device path
    pub fn device_path(&self) -> &PathBuf {
        &self.device_path
    }

    /// Check if a disc is present
    pub fn has_disc(&self) -> bool {
        unsafe {
            libcdio_sys::cdio_get_discmode(self.device)
                != libcdio_sys::discmode_t_CDIO_DISC_MODE_NO_INFO
        }
    }

    /// Get number of tracks
    pub fn num_tracks(&self) -> Result<u8, LibcdioError> {
        unsafe {
            let num = libcdio_sys::cdio_get_num_tracks(self.device) as i32;
            if num < 0 {
                return Err(LibcdioError::Libcdio(
                    "Failed to get track count".to_string(),
                ));
            }
            Ok(num as u8)
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

    /// Get track format (audio vs data)
    pub fn track_format(&self, track_num: u8) -> Result<bool, LibcdioError> {
        unsafe {
            let format =
                libcdio_sys::cdio_get_track_format(self.device, track_num as libcdio_sys::track_t);
            Ok(format == libcdio_sys::track_format_t_TRACK_FORMAT_AUDIO)
        }
    }

    /// Read audio sectors from CD
    /// Returns raw PCM audio data (16-bit stereo, 44100 Hz)
    /// Each sector is 2352 bytes (CD audio sector size)
    pub fn read_audio_sectors(
        &self,
        start_lba: u32,
        num_sectors: u32,
    ) -> Result<Vec<u8>, LibcdioError> {
        unsafe {
            let sector_size = libcdio_sys::CDIO_CD_FRAMESIZE_RAW as usize;
            let total_size = (num_sectors as usize) * sector_size;
            let mut buffer = vec![0u8; total_size];

            for i in 0..num_sectors {
                let lba = (start_lba + i) as libcdio_sys::lba_t;
                let result = libcdio_sys::cdio_read_audio_sector(
                    self.device,
                    buffer.as_mut_ptr().add((i as usize) * sector_size) as *mut libc::c_void,
                    lba as libcdio_sys::lba_t,
                );

                if result != 0 {
                    return Err(LibcdioError::Read(format!(
                        "Failed to read sector at LBA {}",
                        lba
                    )));
                }
            }

            Ok(buffer)
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
                    if let Ok(drive) = LibcdioDrive::open(&path) {
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
            // macOS uses /dev/disk* for CD drives
            use std::fs;
            if let Ok(entries) = fs::read_dir("/dev") {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if name.starts_with("disk") && name.len() > 4 {
                            // Try to open it
                            if let Ok(drive) = LibcdioDrive::open(&path) {
                                if !drives.contains(&path) {
                                    drives.push(path);
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(drives)
    }
}
