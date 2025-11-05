use std::env;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use bae::cue_flac::CueFlacProcessor;
use rayon::prelude::*;
use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_FLAC};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() != 3 {
        eprintln!("Usage: {} <cue_file> <flac_file>", args[0]);
        eprintln!("Example: {} album.cue album.flac", args[0]);
        std::process::exit(1);
    }

    let cue_path = PathBuf::from(&args[1]);
    let flac_path = PathBuf::from(&args[2]);

    if let Err(e) = validate_inputs(&cue_path, &flac_path) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }

    if let Err(e) = split_cue_flac(&cue_path, &flac_path) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn validate_inputs(cue_path: &Path, flac_path: &Path) -> Result<(), String> {
    if !cue_path.exists() {
        return Err(format!("CUE file not found: {}", cue_path.display()));
    }
    if !flac_path.exists() {
        return Err(format!("FLAC file not found: {}", flac_path.display()));
    }
    Ok(())
}

fn split_cue_flac(cue_path: &Path, flac_path: &Path) -> Result<(), String> {
    println!("Parsing CUE sheet: {}", cue_path.display());
    let cue_sheet = CueFlacProcessor::parse_cue_sheet(cue_path)
        .map_err(|e| format!("Failed to parse CUE sheet: {}", e))?;

    println!("Found {} tracks", cue_sheet.tracks.len());

    if cue_sheet.tracks.is_empty() {
        return Err("CUE sheet contains no tracks".to_string());
    }

    let output_dir = flac_path
        .parent()
        .ok_or_else(|| "FLAC file has no parent directory".to_string())?;

    // Process tracks in parallel
    let results: Result<Vec<_>, String> = cue_sheet
        .tracks
        .par_iter()
        .map(|track| {
            let track_start_ms = track.start_time_ms;
            let track_end_ms = track.end_time_ms;

            let output_filename = format!("{:02}.flac", track.number);
            let output_path = output_dir.join(&output_filename);

            decode_and_encode_track(flac_path, &output_path, track_start_ms, track_end_ms)?;

            Ok((track.number, track.title.clone(), output_filename))
        })
        .collect();

    let tracks_processed = results?;

    // Print results in order
    println!();
    for (number, title, filename) in tracks_processed {
        println!("✓ Track {}: {} -> {}", number, title, filename);
    }

    println!("\n✓ Successfully split {} tracks", cue_sheet.tracks.len());
    Ok(())
}

fn decode_and_encode_track(
    source_path: &Path,
    output_path: &Path,
    start_ms: u64,
    end_ms: Option<u64>,
) -> Result<(), String> {
    // Open the source FLAC file with Symphonia (like XLD uses libFLAC)
    let file = File::open(source_path).map_err(|e| format!("Failed to open source file: {}", e))?;

    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    hint.with_extension("flac");

    let format_opts = FormatOptions::default();
    let metadata_opts = MetadataOptions::default();

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &format_opts, &metadata_opts)
        .map_err(|e| format!("Failed to probe file: {}", e))?;

    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec == CODEC_TYPE_FLAC)
        .ok_or_else(|| "No FLAC track found".to_string())?;

    let track_id = track.id;
    let codec_params = track.codec_params.clone();

    let sample_rate = codec_params
        .sample_rate
        .ok_or_else(|| "No sample rate found".to_string())?;
    let channels = codec_params
        .channels
        .ok_or_else(|| "No channel info found".to_string())?;
    let bits_per_sample = codec_params
        .bits_per_sample
        .ok_or_else(|| "No bits per sample found".to_string())?;

    println!(
        "  Sample rate: {} Hz, Channels: {}, Bits: {}",
        sample_rate,
        channels.count(),
        bits_per_sample
    );

    // Create decoder
    let mut decoder = symphonia::default::get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|e| format!("Failed to create decoder: {}", e))?;

    // Seek to start position (like XLD's FLAC__stream_decoder_seek_absolute)
    let start_time = Time::from(start_ms as f64 / 1000.0);
    format
        .seek(
            SeekMode::Accurate,
            SeekTo::Time {
                time: start_time,
                track_id: Some(track_id),
            },
        )
        .map_err(|e| format!("Failed to seek to start: {}", e))?;

    // Calculate end sample
    let end_sample = end_ms.map(|ms| (ms * sample_rate as u64) / 1000);

    // Collect decoded samples (interleaved for all channels)
    let num_channels = channels.count();
    let mut all_samples: Vec<i32> = Vec::new();
    let mut current_sample = (start_ms * sample_rate as u64) / 1000;

    println!("  Decoding audio...");

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(e) => return Err(format!("Failed to read packet: {}", e)),
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = decoder
            .decode(&packet)
            .map_err(|e| format!("Failed to decode packet: {}", e))?;

        // Extract samples from the decoded audio buffer (interleave channels)
        let num_frames = decoded.frames();

        for frame_idx in 0..num_frames {
            if let Some(end) = end_sample {
                if current_sample >= end {
                    break;
                }
            }

            // Interleave channels
            for ch_idx in 0..num_channels {
                let sample = match &decoded {
                    AudioBufferRef::S16(buf) => buf.chan(ch_idx)[frame_idx] as i32,
                    AudioBufferRef::S32(buf) => {
                        // S32 samples from Symphonia are in full 32-bit range
                        // Scale down to the target bits_per_sample range
                        let s32_sample = buf.chan(ch_idx)[frame_idx];
                        s32_sample >> (32 - bits_per_sample)
                    }
                    _ => return Err("Unsupported sample format".to_string()),
                };
                all_samples.push(sample);
            }

            current_sample += 1;
        }

        if let Some(end) = end_sample {
            if current_sample >= end {
                break;
            }
        }
    }

    println!(
        "  Decoded {} samples ({} frames)",
        all_samples.len(),
        all_samples.len() / num_channels
    );

    // Encode to FLAC using flacenc
    println!("  Encoding to FLAC...");
    encode_flac(
        output_path,
        &all_samples,
        sample_rate,
        num_channels as u32,
        bits_per_sample,
    )?;

    Ok(())
}

fn encode_flac(
    output_path: &Path,
    samples: &[i32],
    sample_rate: u32,
    channels: u32,
    bits_per_sample: u32,
) -> Result<(), String> {
    use flacenc::bitsink::ByteSink;
    use flacenc::component::BitRepr;
    use flacenc::config;
    use flacenc::error::Verify;
    use flacenc::source::MemSource;

    // Convert samples to the format flacenc expects (interleaved i32)
    let source = MemSource::from_samples(
        samples,
        channels as usize,
        bits_per_sample as usize,
        sample_rate as usize,
    );

    // Create and verify encoder config
    let config = config::Encoder::default();
    let config = config
        .into_verified()
        .map_err(|(_, e)| format!("Failed to verify encoder config: {:?}", e))?;

    // Encode with default block size (4096)
    let flac_stream = flacenc::encode_with_fixed_block_size(&config, source, 4096)
        .map_err(|e| format!("Failed to encode FLAC: {:?}", e))?;

    // Write stream to a ByteSink
    let mut sink = ByteSink::new();
    flac_stream
        .write(&mut sink)
        .map_err(|e| format!("Failed to write stream to sink: {:?}", e))?;

    // Write to file
    let output_file =
        File::create(output_path).map_err(|e| format!("Failed to create output file: {}", e))?;
    let mut writer = BufWriter::new(output_file);

    use std::io::Write;
    writer
        .write_all(sink.as_slice())
        .map_err(|e| format!("Failed to write FLAC file: {}", e))?;

    Ok(())
}
