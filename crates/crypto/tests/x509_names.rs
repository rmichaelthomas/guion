use guion_crypto::{x509_dns_names, x509_validity};

fn der(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/tests/fixtures/x509/{name}.der",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|error| panic!("{path}: {error}"))
}

#[test]
fn san_dns_names_are_returned_in_order_and_ignore_other_kinds_and_the_cn() {
    assert_eq!(
        x509_dns_names(&der("leaf-san")).unwrap(),
        vec!["a.example.test".to_string(), "b.example.test".to_string()]
    );
}

#[test]
fn the_subject_cn_is_used_only_without_a_san_extension() {
    assert_eq!(
        x509_dns_names(&der("leaf-cn-only")).unwrap(),
        vec!["cn-only.example.test".to_string()]
    );
    // A SAN with no dNSName entry does not fall back to the CN.
    assert_eq!(
        x509_dns_names(&der("leaf-san-ip-only")).unwrap(),
        Vec::<String>::new()
    );
    assert_eq!(
        x509_dns_names(&der("leaf-no-names")).unwrap(),
        Vec::<String>::new()
    );
}

#[test]
fn the_real_nist_leaf_names_the_beacon_host() {
    let names = x509_dns_names(&der("nist-leaf")).unwrap();
    assert!(
        names.iter().any(|name| name == "engine.beacon.nist.gov"),
        "{names:?}"
    );
}

#[test]
fn validity_is_notbefore_and_notafter_in_unix_seconds() {
    // 2024-01-02T03:05:05Z .. 2025-06-07T08:09:10Z
    assert_eq!(
        x509_validity(&der("leaf-dated")).unwrap(),
        [1704164645, 1749283750]
    );
}

#[test]
fn the_real_nist_leaf_validity_covers_its_pulses() {
    let [not_before, not_after] = x509_validity(&der("nist-leaf")).unwrap();
    assert!(not_before > 0 && not_after > not_before);
}

#[test]
fn malformed_der_is_e_x509_and_never_panics() {
    let good = der("leaf-san");
    let mut cases: Vec<Vec<u8>> = vec![
        vec![],
        vec![0x30],
        vec![0x30, 0x03, 0x01, 0x01, 0xff],
        b"not a cert".to_vec(),
    ];
    for cut in [1usize, 10, good.len() / 2, good.len() - 1] {
        cases.push(good[..cut].to_vec());
    }
    for case in cases {
        assert_eq!(x509_dns_names(&case).unwrap_err().code, "E_X509");
        assert_eq!(x509_validity(&case).unwrap_err().code, "E_X509");
    }
}

#[test]
fn a_generalizedtime_validity_from_2050_on_is_read() {
    // leaf-gentime.der (make_generalizedtime_fixture.sh): notBefore 2049-12-31T23:59:59Z is a
    // UTCTime, notAfter 2055-01-01T00:00:00Z is a GeneralizedTime (DER requires it from 2050).
    assert_eq!(
        x509_validity(&der("leaf-gentime")).unwrap(),
        [2524607999, 2682374400]
    );
}
