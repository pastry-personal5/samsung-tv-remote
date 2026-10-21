//! Isolated signed-process experiment. It has no TV or credential access.
use std::env;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::{self, Command, Stdio};

fn main() {
    let args: Vec<String> = env::args().collect();
    let result = match args.as_slice() {
        [_, mode, port, state_dir] if mode == "--serve" => serve(port, Path::new(state_dir)),
        [_, mode, port, executable, state_dir] if mode == "--launch" => {
            launch(port, executable, Path::new(state_dir))
        }
        [_, mode, port, executable, state_dir] if mode == "--resume" => {
            resume(port, executable, Path::new(state_dir))
        }
        _ => Err(
            "usage: probe --serve PORT STATE_DIR | --launch/--resume PORT SERVER_PATH STATE_DIR"
                .to_owned(),
        ),
    };
    if let Err(error) = result {
        eprintln!("{error}");
        process::exit(1);
    }
}

fn serve(port: &str, state_dir: &Path) -> Result<(), String> {
    let port: u16 = port.parse().map_err(|_| "invalid port")?;
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|error| error.to_string())?;
    for stream in listener.incoming() {
        let mut stream = stream.map_err(|error| error.to_string())?;
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .map_err(|error| error.to_string())?;
        let mut input = [0; 1024];
        let size = stream.read(&mut input).map_err(|error| error.to_string())?;
        if input[..size].starts_with(b"POST /stop ") {
            mark_stopped(state_dir)?;
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\nm6-stopped",
                )
                .map_err(|error| error.to_string())?;
            break;
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\nm6-ready")
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn launch(port: &str, executable: &str, state_dir: &Path) -> Result<(), String> {
    if state_dir.join("stopped").exists() {
        return Err("user-stopped marker prevents launch".to_owned());
    }
    let child = Command::new(executable)
        .args([
            "--serve",
            port,
            state_dir.to_str().ok_or("invalid state path")?,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .current_dir("/")
        .env_clear()
        .spawn()
        .map_err(|error| error.to_string())?;
    println!("{}", child.id());
    Ok(())
}

fn mark_stopped(state_dir: &Path) -> Result<(), String> {
    let temporary = state_dir.join(format!("stopped-{}.tmp", process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(b"stopped")?;
        file.sync_all()?;
        fs::rename(&temporary, state_dir.join("stopped"))?;
        File::open(state_dir)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|error: std::io::Error| error.to_string())
}

fn resume(port: &str, executable: &str, state_dir: &Path) -> Result<(), String> {
    fs::remove_file(state_dir.join("stopped")).map_err(|error| error.to_string())?;
    File::open(state_dir)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| error.to_string())?;
    launch(port, executable, state_dir)
}
