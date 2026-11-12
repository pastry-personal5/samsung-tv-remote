//! Disposable signed-platform fixture. No production profile or Keychain access.
use samsung_tv_remote::{
    infrastructure::macos::signed_peer::SignedPeer,
    server::bootstrap::{PeerVerifier, Role},
};
use std::{
    io,
    process::{Command, Stdio},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    time::timeout,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let verifier = SignedPeer::for_current_executable()
        .map_err(|_| io::Error::other("signed identity required"))?;
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.len() == 2 && arguments[0] == "--child" {
        let mut stream = UnixStream::connect(&arguments[1]).await?;
        let verified = verifier.verify(&stream, Role::Gui).is_ok();
        stream.write_u8(u8::from(verified)).await?;
        // Keep the authenticated process alive until the parent checks it.
        let _ = timeout(Duration::from_secs(5), stream.read_u8()).await;
        return if verified {
            Ok(())
        } else {
            Err(io::Error::other("parent rejected").into())
        };
    }
    if !arguments.is_empty() {
        return Err(io::Error::other("usage: signed_peer_probe [--child socket]").into());
    }
    let directory = tempfile::tempdir_in("/private/tmp")?;
    let socket = directory.path().join("peer.sock");
    let listener = UnixListener::bind(&socket)?;
    let mut child = Command::new(std::env::current_exe()?)
        .arg("--child")
        .arg(&socket)
        .spawn()?;
    let (mut stream, _) = timeout(Duration::from_secs(10), listener.accept()).await??;
    let accepted = verifier.verify(&stream, Role::Gui).is_ok();
    let mutual = timeout(Duration::from_secs(5), stream.read_u8()).await?? == 1;
    stream.write_u8(1).await?;
    let successful = child.wait()?.success();
    if !accepted || !mutual || !successful {
        return Err(io::Error::other("same signed executable rejected").into());
    }
    println!("same-executable: accepted mutually");
    // nc connects from a distinct signed executable owned by the same user.
    let mut wrong = Command::new("/usr/bin/nc")
        .arg("-U")
        .arg(&socket)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let result = timeout(Duration::from_secs(5), listener.accept()).await;
    let rejected = match result {
        Ok(Ok((stream, _))) => verifier.verify(&stream, Role::Gui).is_err(),
        _ => false,
    };
    let _ = wrong.kill();
    let _ = wrong.wait();
    if !rejected {
        return Err(io::Error::other("wrong-executable rejection not proven").into());
    }
    println!("different same-user executable: rejected");
    println!("requirement: {}", verifier.requirement());
    Ok(())
}
