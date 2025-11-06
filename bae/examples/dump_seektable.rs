use std::collections::HashMap;

fn main() {
    let track_id = std::env::args()
        .nth(1)
        .expect("Usage: dump_seektable <track_id>");

    let db_path =
        "/var/folders/_1/btkmr9qd7js2n9nndq7bbr_h0000gn/T/bae_test_streaming_seek/test.db";

    let conn = rusqlite::Connection::open(db_path).unwrap();

    let mut stmt = conn
        .prepare("SELECT flac_seektable FROM audio_formats WHERE track_id = ?1")
        .unwrap();

    let seektable_bytes: Vec<u8> = stmt.query_row([&track_id], |row| row.get(0)).unwrap();

    let seektable: HashMap<u64, u64> = bincode::deserialize(&seektable_bytes).unwrap();

    println!("Seektable has {} seekpoints", seektable.len());

    if let (Some(min_sample), Some(max_sample)) = (seektable.keys().min(), seektable.keys().max()) {
        let sample_rate = 44100u64;
        let duration_samples = max_sample - min_sample;
        let avg_samples_per_seekpoint = if seektable.len() > 1 {
            duration_samples / (seektable.len() as u64 - 1)
        } else {
            0
        };

        println!(
            "Range: sample {} to {} (~{:.1}s)",
            min_sample,
            max_sample,
            duration_samples as f64 / sample_rate as f64
        );
        println!(
            "Density: ~{:.1}s between seekpoints (avg {} samples)",
            avg_samples_per_seekpoint as f64 / sample_rate as f64,
            avg_samples_per_seekpoint
        );
    }
}
