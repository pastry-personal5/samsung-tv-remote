//! One-shot LaunchAgent experiment. It has no network or credential access.
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let home = PathBuf::from(env::var_os("HOME").ok_or("HOME is unavailable")?);
    let directory = home.join("Library/Application Support/M6LaunchAgentProbe");
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&directory)?;
    let file = directory.join("starts.log");
    let mut output = OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .open(file)?;
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    writeln!(output, "{timestamp} pid={}", process::id())?;
    output.sync_all()?;
    File::open(directory)?.sync_all()?;
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("M6 LaunchAgent probe failed: {error}");
        process::exit(1);
    }
}
