use std::sync::Arc;

use samsung_tv_remote::application::tv_control_coordinator::AppServices;
use samsung_tv_remote::application::tv_setup_service::TvSetupService;
use samsung_tv_remote::infrastructure::macos::keychain::KeychainCredentialStore;
use samsung_tv_remote::infrastructure::preferences::{
    default_app_data_dir, LocalCertificateTrustStore, LocalDeviceRepository, LocalRecoveryJournal,
};
use samsung_tv_remote::infrastructure::samsung::session::SamsungGateway;
use samsung_tv_remote::infrastructure::ssdp_discovery::SsdpDiscovery;
use samsung_tv_remote::infrastructure::wake_udp::UdpWakeTransport;
use samsung_tv_remote::presentation::iced::app;

fn main() -> iced::Result {
    if let Some(status) =
        samsung_tv_remote::server::entry::dispatch(&std::env::args_os().skip(1).collect::<Vec<_>>())
    {
        std::process::exit(status);
    }
    let setup = default_app_data_dir().map(|directory| {
        Arc::new(
            TvSetupService::new(
                LocalDeviceRepository::new(&directory),
                KeychainCredentialStore,
                LocalCertificateTrustStore::new(&directory),
            )
            .with_journal(Arc::new(LocalRecoveryJournal::new(&directory))),
        ) as Arc<dyn samsung_tv_remote::application::tv_setup_service::TvSetupPort>
    });
    app::run(AppServices::new(
        setup,
        Arc::new(SsdpDiscovery),
        Arc::new(SamsungGateway),
        Arc::new(UdpWakeTransport),
    ))
}
