use samsung_tv_remote::application::control_service::{CandidateId, Command, Epoch, OperationId};
use samsung_tv_remote::domain::DeviceId;
use samsung_tv_remote::server::control_http::request::{
    parse_mutation, parse_stop, MutationRoute, RequestError, MAX_BODY_BYTES,
};
use samsung_tv_remote::server::control_http::wire::{self, Counter};
use serde_json::{json, Value};

fn envelope() -> Value {
    json!({"server_epoch": wire::epoch(Epoch::from_u128(7)), "operation_key": "a2222222-2222-4222-8222-222222222222"})
}
fn body(extra: Value) -> Vec<u8> {
    let mut value = envelope();
    value
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    serde_json::to_vec(&value).unwrap()
}

#[test]
fn idle_connect_flag_is_optional_strict_and_round_trips() {
    let device = wire::device(DeviceId::new(1));
    let base = json!({"device_id":device,"selection_generation":"0"});
    let epoch = Epoch::from_u128(7);
    let explicit = parse_mutation(MutationRoute::Connect, &body(base.clone()), epoch).unwrap();
    assert!(matches!(
        explicit.command,
        Command::Connect {
            only_if_idle: false,
            ..
        }
    ));
    let mut automatic = base;
    automatic["only_if_idle"] = json!(true);
    let request = parse_mutation(MutationRoute::Connect, &body(automatic.clone()), epoch).unwrap();
    assert!(matches!(
        request.command,
        Command::Connect {
            only_if_idle: true,
            ..
        }
    ));
    let (_, _, encoded) = samsung_tv_remote::control_client::projection::encode(&request).unwrap();
    assert_eq!(
        parse_mutation(
            MutationRoute::Connect,
            &serde_json::to_vec(&encoded).unwrap(),
            epoch
        )
        .unwrap(),
        request
    );
    for invalid in [Value::Null, json!("true"), json!(1)] {
        automatic["only_if_idle"] = invalid;
        assert_eq!(
            parse_mutation(MutationRoute::Connect, &body(automatic.clone()), epoch),
            Err(RequestError::InvalidRequest)
        );
    }
    automatic["only_if_idle"] = json!(true);
    for route in [
        MutationRoute::Wake,
        MutationRoute::CancelWake(&wire::operation(OperationId(1))),
    ] {
        assert_eq!(
            parse_mutation(route, &body(automatic.clone()), epoch),
            Err(RequestError::InvalidRequest)
        );
    }
}

#[test]
fn identifiers_are_canonical_kind_checked_and_lossless() {
    let epoch = Epoch::from_u128(u128::MAX);
    assert_eq!(wire::parse_epoch(&wire::epoch(epoch)), Ok(epoch));
    let device = DeviceId::generate();
    assert_eq!(wire::parse_device(&wire::device(device)), Ok(device));
    for counter in [1, (1 << 53) + 1, u64::MAX] {
        let op = OperationId(counter);
        assert_eq!(
            wire::parse_operation(&wire::operation(op), epoch, epoch),
            Ok(op)
        );
        for index in [0, 31] {
            let candidate = CandidateId(op, index);
            let encoded = wire::candidate(candidate);
            assert_eq!(wire::parse_candidate(&encoded, epoch, epoch), Ok(candidate));
            assert!(wire::parse_operation(&encoded, epoch, epoch).is_err());
        }
        assert!(wire::parse_candidate(&wire::operation(op), epoch, epoch).is_err());
        assert_eq!(
            wire::parse_operation(&wire::operation(op), epoch, Epoch::from_u128(8)),
            Err(wire::WireError::StaleEpoch)
        );
    }
    for bad in [
        wire::operation(OperationId(0)),
        wire::candidate(CandidateId(OperationId(1), 32)),
        "53545201-0100-0001-0000-000000000001".into(),
        "53545201-0100-0000-0000-00000000000A".into(),
        "53545201010000000000000000000001".into(),
    ] {
        assert!(wire::parse_operation(&bad, epoch, epoch).is_err());
    }
}

#[test]
fn counters_reject_rounding_overflow_and_noncanonical_input() {
    for n in [0, 1, (1 << 53) + 1, u64::MAX] {
        let encoded = serde_json::to_string(&Counter(n)).unwrap();
        assert_eq!(encoded, format!("\"{n}\""));
        assert_eq!(
            serde_json::from_str::<Counter>(&encoded).unwrap(),
            Counter(n)
        );
    }
    for bad in [
        "0",
        "9007199254740993",
        "1.5",
        "NaN",
        "\"18446744073709551616\"",
        "\"-1\"",
        "\"01\"",
        "\"+1\"",
        "\" 1\"",
        "\"1e2\"",
        "null",
    ] {
        assert!(serde_json::from_str::<Counter>(bad).is_err(), "{bad}");
    }
}

#[test]
fn every_mutation_route_preserves_guards_and_fits_the_body_budget() {
    let device = wire::device(DeviceId::new(1));
    let operation = wire::operation(OperationId(u64::MAX));
    let candidate = wire::candidate(CandidateId(OperationId(u64::MAX), 31));
    let n = u64::MAX.to_string();
    let pin = "ab".repeat(32);
    let host = [
        "a".repeat(63),
        "b".repeat(63),
        "c".repeat(63),
        "d".repeat(61),
    ]
    .join(".");
    let selected = json!({"device_id":device,"selection_generation":n});
    let cases = vec![
        (MutationRoute::Discover, json!({})),
        (
            MutationRoute::Probe,
            json!({"host":host,"candidate_id":candidate}),
        ),
        (MutationRoute::Probe, json!({"host":"tv.local"})),
        (
            MutationRoute::Pair,
            json!({"attempt_id":operation,"observation_id":operation,"host":host,"fingerprint":pin,"confirm_fingerprint":pin,"label":"\u{0001}".repeat(256),"selection_generation":n,"device_id":device,"recovery_transaction_id":device}),
        ),
        (
            MutationRoute::Select,
            json!({"device_id":device,"expected_generation":n}),
        ),
        (MutationRoute::Connect, selected.clone()),
        (MutationRoute::Wake, selected.clone()),
        (MutationRoute::CancelWake(&operation), selected),
        (
            MutationRoute::Remote,
            json!({"device_id":device,"selection_generation":n,"action":"VolumeUp"}),
        ),
        (
            MutationRoute::SetWake(&device),
            json!({"device_id":device,"selection_generation":n,"expected_config_revision":n,"wired_mac":"02:00:00:00:00:01","wifi_mac":"02:00:00:00:00:02","active_interface":"WiFi"}),
        ),
        (
            MutationRoute::Forget(&device),
            json!({"device_id":device,"expected_config_revision":n,"confirm_device_id":device}),
        ),
        (
            MutationRoute::RecoverPairing(&device),
            json!({"device_id":device,"transaction_id":device,"confirm_device_id":device}),
        ),
        (
            MutationRoute::RecoverForget(&device),
            json!({"device_id":device,"transaction_id":device,"confirm_device_id":device}),
        ),
    ];
    for (route, extra) in cases {
        let encoded = body(extra);
        assert!(encoded.len() < MAX_BODY_BYTES);
        let request = parse_mutation(route, &encoded, Epoch::from_u128(7)).unwrap();
        assert_eq!(request.epoch, Epoch::from_u128(7));
        let (method, _path, native) =
            samsung_tv_remote::control_client::projection::encode(&request).unwrap();
        assert_eq!(
            method,
            if matches!(request.command, Command::SetWake { .. }) {
                "PUT"
            } else {
                "POST"
            }
        );
        let decoded = parse_mutation(
            route,
            &serde_json::to_vec(&native).unwrap(),
            Epoch::from_u128(7),
        )
        .unwrap();
        assert_eq!(decoded, request);

        match request.command {
            Command::Select { generation, .. }
            | Command::Connect { generation, .. }
            | Command::Wake { generation, .. }
            | Command::CancelWake { generation, .. }
            | Command::SetWake { generation, .. }
            | Command::Pair { generation, .. } => assert_eq!(generation, u64::MAX),
            Command::Remote(r) => assert_eq!(r.selection_generation(), u64::MAX),
            _ => (),
        }
    }
    assert!(parse_stop(&body(json!({})), Epoch::from_u128(7)).is_ok());
}

#[test]
fn invalid_envelopes_and_confirmations_cannot_reach_admission() {
    let epoch = Epoch::from_u128(7);
    let device = wire::device(DeviceId::new(1));
    let other = wire::device(DeviceId::new(2));
    assert_eq!(
        parse_mutation(
            MutationRoute::Discover,
            &body(json!({"client_id":1})),
            epoch
        ),
        Err(RequestError::InvalidRequest)
    );
    let duplicate = br#"{"server_epoch":"00000000-0000-0000-0000-000000000007","operation_key":"a2222222-2222-4222-8222-222222222222","operation_key":"a2222222-2222-4222-8222-222222222222"}"#;
    assert_eq!(
        parse_stop(duplicate, epoch),
        Err(RequestError::InvalidRequest)
    );
    assert_eq!(
        parse_stop(&body(json!({})), Epoch::from_u128(8)),
        Err(RequestError::StaleEpoch)
    );
    assert_eq!(
        parse_stop(&vec![b' '; MAX_BODY_BYTES + 1], epoch),
        Err(RequestError::TooLarge)
    );
    assert_eq!(
        parse_mutation(
            MutationRoute::Forget(&device),
            &body(
                json!({"device_id":other,"expected_config_revision":"0","confirm_device_id":other})
            ),
            epoch
        ),
        Err(RequestError::InvalidTarget)
    );
    assert_eq!(
        parse_mutation(
            MutationRoute::RecoverPairing(&device),
            &body(json!({"device_id":device,"transaction_id":device,"confirm_device_id":other})),
            epoch
        ),
        Err(RequestError::InvalidConfirmation)
    );
    assert!(parse_mutation(
        MutationRoute::CancelWake(&wire::operation(OperationId(1))),
        &body(json!({"device_id":device})),
        epoch
    )
    .is_err());
    assert!(parse_mutation(
        MutationRoute::SetWake(&device),
        &body(json!({"device_id":device,"expected_config_revision":"0"})),
        epoch
    )
    .is_err());
}
