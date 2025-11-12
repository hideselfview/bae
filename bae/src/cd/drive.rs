//! CD drive detection and TOC reading

use discid::DiscId;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CdDriveError {
    #[error("No CD drive found")]
    NoDrive,
    #[error("No disc in drive")]
    NoDisc,
    #[error("DiscID error: {0}")]
    DiscId(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Drive access error: {0}")]
    Access(String),
}

/// Represents a CD drive
#[derive(Debug, Clone)]
pub struct CdDrive {
    pub device_path: PathBuf,
    pub name: String,
}

/// Table of Contents (TOC) information from a CD
#[derive(Debug, Clone)]
pub struct CdToc {
    pub disc_id: String,
    pub first_track: u8,
    pub last_track: u8,
    pub leadout_track: u8,
    pub track_offsets: Vec<u32>, // Sector offsets for each track
}

impl CdDrive {
    /// Detect available CD drives
    pub fn detect_drives() -> Result<Vec<CdDrive>, CdDriveError> {
        use crate::cd::ffi::detect_drives;

        let device_paths = detect_drives()
            .map_err(|e| CdDriveError::Access(format!("Failed to detect drives: {}", e)))?;

        let mut drives = Vec::new();
        for path in device_paths {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Unknown")
                .to_string();
            drives.push(CdDrive {
                device_path: path,
                name,
            });
        }

        Ok(drives)
    }

    /// Read TOC from the disc in this drive
    pub fn read_toc(&self) -> Result<CdToc, CdDriveError> {
        use crate::cd::ffi::LibcdioDrive;

        // Open drive with libcdio
        let drive = LibcdioDrive::open(&self.device_path)
            .map_err(|e| CdDriveError::Access(format!("Failed to open drive: {}", e)))?;

        // Check if disc is present
        if !drive.has_disc() {
            return Err(CdDriveError::NoDisc);
        }

        // Get track information
        let first_track = drive
            .first_track_num()
            .map_err(|e| CdDriveError::DiscId(format!("Failed to get first track: {}", e)))?;
        let last_track = drive
            .last_track_num()
            .map_err(|e| CdDriveError::DiscId(format!("Failed to get last track: {}", e)))?;

        // Get track offsets (LBAs)
        let mut track_offsets = Vec::new();
        for track_num in first_track..=last_track {
            let lba = drive.track_start_lba(track_num).map_err(|e| {
                CdDriveError::DiscId(format!("Failed to get LBA for track {}: {}", track_num, e))
            })?;
            // Convert LBA to sector offset (add 150 for lead-in)
            track_offsets.push(lba + 150);
        }

        // Get leadout LBA
        let leadout_lba = drive
            .leadout_lba()
            .map_err(|e| CdDriveError::DiscId(format!("Failed to get leadout: {}", e)))?;
        let leadout_track = (leadout_lba + 150) as u8;

        // Calculate DiscID using discid crate (for MusicBrainz lookup)
        let device_str = self
            .device_path
            .to_str()
            .ok_or_else(|| CdDriveError::Access("Invalid device path".to_string()))?;

        let disc = DiscId::read(Some(device_str))
            .map_err(|e| CdDriveError::DiscId(format!("Failed to read disc: {}", e)))?;

        Ok(CdToc {
            disc_id: disc.id(),
            first_track,
            last_track,
            leadout_track,
            track_offsets,
        })
    }

    /// Check if a disc is present in the drive
    pub fn has_disc(&self) -> Result<bool, CdDriveError> {
        // TODO: Implement disc detection using libcdio
        // For now, try to read TOC - if it fails, assume no disc
        match self.read_toc() {
            Ok(_) => Ok(true),
            Err(CdDriveError::NoDisc) => Ok(false),
            Err(e) => Err(e),
        }
    }
}
