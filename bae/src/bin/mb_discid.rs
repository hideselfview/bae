use std::env;
use std::path::PathBuf;
use tracing::{error, info};

use bae::import::{calculate_mb_discid_from_cue_flac, calculate_mb_discid_from_log};

fn main() {
    // Use RUST_LOG env var if set, otherwise default to info level for detailed output
    let log_filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    tracing_subscriber::fmt().with_env_filter(log_filter).init();

    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage(&args[0]);
        std::process::exit(1);
    }

    let mut log_path: Option<PathBuf> = None;
    let mut cue_path: Option<PathBuf> = None;
    let mut flac_path: Option<PathBuf> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--log" => {
                if i + 1 >= args.len() {
                    error!("--log requires a file path");
                    print_usage(&args[0]);
                    std::process::exit(1);
                }
                log_path = Some(PathBuf::from(&args[i + 1]));
                i += 2;
            }
            "--cue" => {
                if i + 1 >= args.len() {
                    error!("--cue requires a file path");
                    print_usage(&args[0]);
                    std::process::exit(1);
                }
                cue_path = Some(PathBuf::from(&args[i + 1]));
                i += 2;
            }
            "--flac" => {
                if i + 1 >= args.len() {
                    error!("--flac requires a file path");
                    print_usage(&args[0]);
                    std::process::exit(1);
                }
                flac_path = Some(PathBuf::from(&args[i + 1]));
                i += 2;
            }
            _ => {
                error!("Unknown argument: {}", args[i]);
                print_usage(&args[0]);
                std::process::exit(1);
            }
        }
    }

    // Validate inputs
    if let Some(ref log) = log_path {
        if !log.exists() {
            error!("LOG file not found: {}", log.display());
            std::process::exit(1);
        }
        if cue_path.is_some() || flac_path.is_some() {
            error!("Cannot use --log with --cue/--flac. Use either --log alone or --cue + --flac");
            std::process::exit(1);
        }
    } else if cue_path.is_some() || flac_path.is_some() {
        if cue_path.is_none() || flac_path.is_none() {
            error!("Both --cue and --flac are required when using CUE/FLAC mode");
            print_usage(&args[0]);
            std::process::exit(1);
        }
        let cue = cue_path.as_ref().unwrap();
        let flac = flac_path.as_ref().unwrap();
        if !cue.exists() {
            error!("CUE file not found: {}", cue.display());
            std::process::exit(1);
        }
        if !flac.exists() {
            error!("FLAC file not found: {}", flac.display());
            std::process::exit(1);
        }
    } else {
        error!("No input files specified");
        print_usage(&args[0]);
        std::process::exit(1);
    }

    // Calculate DiscID
    let discid = if let Some(log) = log_path {
        info!("Calculating MusicBrainz DiscID from LOG file: {:?}", log);
        match calculate_mb_discid_from_log(&log) {
            Ok(id) => id,
            Err(e) => {
                error!("Failed to calculate DiscID from LOG: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        let cue = cue_path.unwrap();
        let flac = flac_path.unwrap();
        info!(
            "Calculating MusicBrainz DiscID from CUE: {:?} and FLAC: {:?}",
            cue, flac
        );
        match calculate_mb_discid_from_cue_flac(&cue, &flac) {
            Ok(id) => id,
            Err(e) => {
                error!("Failed to calculate DiscID from CUE/FLAC: {}", e);
                std::process::exit(1);
            }
        }
    };

    // Print result
    println!("{}", discid);
}

fn print_usage(program_name: &str) {
    eprintln!("Usage:");
    eprintln!("  {} --log <log_file>", program_name);
    eprintln!("  {} --cue <cue_file> --flac <flac_file>", program_name);
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  {} --log album.log", program_name);
    eprintln!("  {} --cue album.cue --flac album.flac", program_name);
}
