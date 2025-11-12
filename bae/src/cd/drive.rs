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
        // TODO: Implement actual drive detection using libcdio
        // For now, return empty list - will be implemented with FFI bindings
        Ok(vec![])
    }

    /// Read TOC from the disc in this drive
    pub fn read_toc(&self) -> Result<CdToc, CdDriveError> {
        // TODO: Use libcdio-paranoia to read TOC directly
        // For now, use discid crate to read DiscID and basic track info
        // Note: discid::read() doesn't provide direct access to offsets,
        // so we'll need libcdio for full TOC reading
        
        let device_str = self
            .device_path
            .to_str()
            .ok_or_else(|| CdDriveError::Access("Invalid device path".to_string()))?;

        let disc = DiscId::read(Some(device_str))
            .map_err(|e| CdDriveError::DiscId(format!("Failed to read disc: {}", e)))?;

        let first_track = disc.first_track_num() as u8;
        let last_track = disc.last_track_num() as u8;
        
        // TODO: Get actual offsets from libcdio
        // For now, create placeholder offsets (will be replaced with libcdio implementation)
        let num_tracks = (last_track - first_track + 1) as usize;
        let mut track_offsets = Vec::with_capacity(num_tracks);
        for i in 0..num_tracks {
            // Placeholder: assume 75 sectors per second, ~3 minutes per track
            track_offsets.push((150 + i * 13500) as u32); // 150 (lead-in) + track offset
        }
        
        // Placeholder leadout (will be replaced with actual value from libcdio)
        let leadout_track = if !track_offsets.is_empty() {
            (track_offsets.last().unwrap() + 13500) as u8 // Last track + ~3 minutes
        } else {
            150u8
        };

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

