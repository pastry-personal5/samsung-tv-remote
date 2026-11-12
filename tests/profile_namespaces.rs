//! Namespace boundaries use temporary private directories and no Keychain,
//! Samsung, UDP, DNS or production profile access.
use samsung_tv_remote::{
    infrastructure::private_files::{FileError, Namespace, RuntimeDirectory},
    server::{
        bootstrap::socket::Location,
        development::DevelopmentProfile,
        lifecycle::{
            endpoint::Manifest,
            stop::{FileStopStore, NamespacedStopStore, StopMarker, StopStore},
        },
    },
};
use std::sync::Arc;

#[test]
fn endpoint_and_stop_metadata_cannot_cross_profile_namespaces() {
    let base = tempfile::tempdir().unwrap();
    let base = base.path().canonicalize().unwrap();
    for namespace in [Namespace::Development, Namespace::Control] {
        let directory = Arc::new(RuntimeDirectory::open(&base.join(namespace.name())).unwrap());
        let owner = directory.lock().unwrap();
        let (manifest, listener) =
            Manifest::bind_for(&directory, &owner, &"ab".repeat(32), namespace).unwrap();
        assert_eq!(manifest.profile, namespace.name());
        assert_eq!(
            Manifest::read_for(&directory, namespace).unwrap(),
            Some(manifest.clone())
        );
        let other = if namespace == Namespace::Control {
            Namespace::Development
        } else {
            Namespace::Control
        };
        assert!(matches!(
            Manifest::read_for(&directory, other),
            Err(FileError::UnsafePath)
        ));
        drop(listener);
        let (rebound, listener) =
            Manifest::bind_for(&directory, &owner, &"cd".repeat(32), namespace).unwrap();
        assert_eq!(rebound.port, manifest.port);
        drop(listener);
        let store = NamespacedStopStore::new(directory.clone(), namespace);
        let marker = StopMarker {
            version: 1,
            profile: namespace.name().into(),
            server_epoch: uuid::Uuid::new_v4().to_string(),
            operation_key: uuid::Uuid::new_v4().to_string(),
            identity: "gui".into(),
        };
        store.write(&marker).unwrap();
        assert_eq!(store.read().unwrap(), Some(marker.clone()));
        let wrong = NamespacedStopStore::new(directory.clone(), other);
        assert!(matches!(wrong.read(), Err(FileError::UnsafePath)));
        assert!(matches!(wrong.clear(), Err(FileError::UnsafePath)));
        assert!(matches!(wrong.write(&marker), Err(FileError::UnsafePath)));
        assert_eq!(store.read().unwrap(), Some(marker));
        if namespace == Namespace::Control {
            assert!(FileStopStore(directory).read().is_err());
        }
        store.clear().unwrap();
    }
}
#[test]
fn bootstrap_paths_and_development_initialization_cannot_adopt_control_identity() {
    let base = tempfile::tempdir().unwrap();
    let base = base.path().canonicalize().unwrap();
    let fake = Location::for_namespace(&base, Namespace::Development).unwrap();
    let control = Location::for_namespace(&base, Namespace::Control).unwrap();
    assert_ne!(fake.profile, control.profile);
    assert_ne!(fake.path, control.path);
    assert!(fake.path.to_str().unwrap().contains("/str-m8-"));
    assert!(control.path.to_str().unwrap().contains("/str-control-v1-"));
    let fake_root = RuntimeDirectory::open(&base.join(Namespace::Development.name())).unwrap();
    fake_root
        .replace("namespace", Namespace::Control.name().as_bytes())
        .unwrap();
    assert!(matches!(
        DevelopmentProfile::open(&base),
        Err(FileError::UnsafePath)
    ));
    for location in [fake, control] {
        std::fs::remove_dir(location.path.parent().unwrap()).unwrap();
    }
}
