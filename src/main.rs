use samsung_tv_remote::presentation::iced::app;

fn main() -> iced::Result {
    if let Some(status) =
        samsung_tv_remote::server::entry::dispatch(&std::env::args_os().skip(1).collect::<Vec<_>>())
    {
        std::process::exit(status);
    }
    let Ok(executable) = std::env::current_exe() else {
        eprintln!("Unable to locate the signed application executable.");
        std::process::exit(1);
    };
    app::run(executable)
}
