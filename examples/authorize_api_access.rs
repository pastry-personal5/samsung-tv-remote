//! Standalone inspection/access helper. Approval of this helper does not prove
//! routine access for a different executable; prefer the app's explicit mode.
use samsung_tv_remote::infrastructure::macos::api_access;
use std::{io, path::Path};

fn main() -> io::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 || !matches!(args[0].to_str(), Some("--inspect" | "--request-access")) {
        return Err(io::Error::other(
            "usage: authorize_api_access --inspect|--request-access <signed-candidate.app>",
        ));
    }
    let inspect = args[0] == "--inspect";
    let report = api_access::run(inspect, Path::new(&args[1]))?;
    println!("{report}");
    if !inspect && (report["gui"] != "available" || report["mcp"] != "available") {
        return Err(io::Error::other(
            "access incomplete; existing values preserved",
        ));
    }
    Ok(())
}
