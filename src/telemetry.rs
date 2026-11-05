//! Telemetry inspection subcommands for GoldSrc.rs `.stitch` binary containers.

use clap::Subcommand;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

#[derive(Subcommand, Debug)]
pub enum TelemetryCommands {
    /// Inspect a `.stitch` container file and print header, chunks, and tick summary
    Inspect {
        /// Path to the `.stitch` container file
        path: PathBuf,
        /// Verbose inspection printing individual chunk metrics
        #[arg(short, long)]
        verbose: bool,
    },
}

/// Raw representation of a 64-byte `TickMetricsPacket` as persisted in `.stitch`.
#[repr(C, align(64))]
#[derive(Debug, Clone, Copy)]
pub struct RawTickPacket {
    pub timestamp_ns: u64,
    pub frame_delta_ns: u64,
    pub measured_fps: f32,
    pub frame_jitter_ms: f32,
    pub cpu_usage: f32,
    pub process_rss_bytes: u64,
    pub sequence: u64,
    pub _padding: [u8; 16],
}

pub fn execute_telemetry(cmd: TelemetryCommands) -> Result<(), String> {
    match cmd {
        TelemetryCommands::Inspect { path, verbose } => inspect_stitch_file(&path, verbose),
    }
}

fn inspect_stitch_file(path: &PathBuf, verbose: bool) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("File does not exist: {}", path.display()));
    }

    let mut file = File::open(path).map_err(|e| format!("Failed to open file: {e}"))?;
    let file_len = file
        .metadata()
        .map_err(|e| format!("Failed to get metadata: {e}"))?
        .len();

    if file_len < 32 {
        return Err("File is too small to be a valid .stitch container (minimum 32 bytes for header+footer)".to_string());
    }

    // 1. Read and parse 16-byte ContainerHeader
    let mut header_bytes = [0u8; 16];
    file.read_exact(&mut header_bytes)
        .map_err(|e| format!("Failed to read header: {e}"))?;

    if &header_bytes[0..4] != b"STDC" {
        return Err(format!(
            "Invalid magic header: expected 'STDC', found {:?}",
            &header_bytes[0..4]
        ));
    }

    let version = u16::from_le_bytes(header_bytes[4..6].try_into().unwrap());
    let flags = u16::from_le_bytes(header_bytes[6..8].try_into().unwrap());
    let schema_hash = u64::from_le_bytes(header_bytes[8..16].try_into().unwrap());

    // 2. Read 16-byte ContainerFooter from end of file
    file.seek(SeekFrom::End(-16))
        .map_err(|e| format!("Failed to seek footer: {e}"))?;
    let mut footer_bytes = [0u8; 16];
    file.read_exact(&mut footer_bytes)
        .map_err(|e| format!("Failed to read footer: {e}"))?;

    let directory_offset = u64::from_le_bytes(footer_bytes[0..8].try_into().unwrap());
    let chunk_count = u32::from_le_bytes(footer_bytes[8..12].try_into().unwrap());
    let directory_crc = u32::from_le_bytes(footer_bytes[12..16].try_into().unwrap());

    println!("============================================================");
    println!(
        " GoldSrc.rs Telemetry Container Inspector: {}",
        path.display()
    );
    println!("============================================================");
    println!(" File Size:         {} bytes", file_len);
    println!(" Container Version: v{}", version);
    println!(" Flags:             0x{:04X}", flags);
    println!(" Schema Hash:       0x{:016X}", schema_hash);
    println!(" Chunk Count:       {}", chunk_count);
    println!(" Directory Offset:  0x{:08X}", directory_offset);
    println!(" Directory CRC32:   0x{:08X}", directory_crc);
    println!("------------------------------------------------------------");

    // 3. Read chunk descriptors (24 bytes each)
    file.seek(SeekFrom::Start(directory_offset))
        .map_err(|e| format!("Failed to seek directory: {e}"))?;

    let dir_len = (chunk_count as usize) * 24;
    let mut dir_bytes = vec![0u8; dir_len];
    file.read_exact(&mut dir_bytes)
        .map_err(|e| format!("Failed to read chunk directory: {e}"))?;

    let mut total_ticks: u64 = 0;
    let mut min_fps = f32::MAX;
    let mut max_fps = 0.0f32;
    let mut sum_fps = 0.0f64;
    let mut first_ts: Option<u64> = None;
    let mut last_ts: Option<u64> = None;

    for i in 0..chunk_count as usize {
        let offset = i * 24;
        let first_key = u64::from_le_bytes(dir_bytes[offset..offset + 8].try_into().unwrap());
        let file_offset =
            u64::from_le_bytes(dir_bytes[offset + 8..offset + 16].try_into().unwrap());
        let compressed_len =
            u32::from_le_bytes(dir_bytes[offset + 16..offset + 20].try_into().unwrap());
        let row_count = u32::from_le_bytes(dir_bytes[offset + 20..offset + 24].try_into().unwrap());

        total_ticks += row_count as u64;

        if verbose {
            println!(
                " Chunk #{:<3}: first_ts={} ns, offset=0x{:08X}, len={} bytes, rows={}",
                i, first_key, file_offset, compressed_len, row_count
            );
        }

        // Read first and last packet of chunk for range inspection
        let mut tick_bytes = [0u8; 64];
        let mut cur_file = file.try_clone().map_err(|e| format!("Clone file: {e}"))?;
        cur_file
            .seek(SeekFrom::Start(file_offset))
            .map_err(|e| format!("Seek chunk: {e}"))?;

        for _ in 0..row_count {
            if cur_file.read_exact(&mut tick_bytes).is_ok() {
                let packet: RawTickPacket =
                    unsafe { core::ptr::read_unaligned(tick_bytes.as_ptr() as *const _) };
                if first_ts.is_none() {
                    first_ts = Some(packet.timestamp_ns);
                }
                last_ts = Some(packet.timestamp_ns);

                if packet.measured_fps > 0.0 {
                    if packet.measured_fps < min_fps {
                        min_fps = packet.measured_fps;
                    }
                    if packet.measured_fps > max_fps {
                        max_fps = packet.measured_fps;
                    }
                    sum_fps += packet.measured_fps as f64;
                }
            }
        }
    }

    let avg_fps = if total_ticks > 0 && sum_fps > 0.0 {
        sum_fps / (total_ticks as f64)
    } else {
        0.0
    };

    let duration_ms = match (first_ts, last_ts) {
        (Some(start), Some(end)) if end >= start => (end - start) as f64 / 1_000_000.0,
        _ => 0.0,
    };

    println!(" Summary Analytics:");
    println!("   Total Ticks Logged:  {}", total_ticks);
    println!(
        "   Session Duration:    {:.2} ms ({:.2} s)",
        duration_ms,
        duration_ms / 1000.0
    );
    if min_fps < f32::MAX {
        println!("   Measured FPS (Min):  {:.1} fps", min_fps);
        println!("   Measured FPS (Max):  {:.1} fps", max_fps);
        println!("   Measured FPS (Avg):  {:.1} fps", avg_fps);
    }
    println!("============================================================");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inspect_missing_file_returns_error() {
        let path = PathBuf::from("non_existent_file.stitch");
        let res = inspect_stitch_file(&path, false);
        assert!(res.is_err());
    }

    #[test]
    fn test_inspect_corrupt_small_file_returns_error() {
        let temp_dir = std::env::temp_dir().join(format!("grs_test_{}", std::process::id()));
        std::fs::create_dir_all(&temp_dir).unwrap();
        let path = temp_dir.join("corrupt.stitch");
        std::fs::write(&path, b"TOO_SMALL").unwrap();

        let res = inspect_stitch_file(&path, false);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("too small"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
