use guion_crypto::{dagcbor_encode, sha3_512};
use serde_json::Value;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn enc(json: &str) -> String {
    let value: Value = serde_json::from_str(json).unwrap();
    hex(&dagcbor_encode(&value).unwrap())
}

fn err(json: &str) -> &'static str {
    let value: Value = serde_json::from_str(json).unwrap();
    dagcbor_encode(&value).unwrap_err().code
}

#[test]
fn encodes_spec_vectors() {
    // Unsigned integers at each width boundary.
    for (json, want) in [
        ("0", "00"),
        ("23", "17"),
        ("24", "1818"),
        ("255", "18ff"),
        ("256", "190100"),
        ("65535", "19ffff"),
        ("65536", "1a00010000"),
        ("4294967295", "1affffffff"),
        ("4294967296", "1b0000000100000000"),
        ("18446744073709551615", "1bffffffffffffffff"),
        // Negative integers: major type 1 carries -1 - n.
        ("-1", "20"),
        ("-24", "37"),
        ("-25", "3818"),
        ("-256", "38ff"),
        ("-257", "390100"),
        ("-65537", "3a00010000"),
        ("-18446744073709551616", "3bffffffffffffffff"),
    ] {
        assert_eq!(enc(json), want, "{json}");
    }
    // Simple values, strings, empty containers.
    assert_eq!(enc("null"), "f6");
    assert_eq!(enc("true"), "f5");
    assert_eq!(enc("false"), "f4");
    assert_eq!(enc("\"\""), "60");
    assert_eq!(enc("\"a\""), "6161");
    assert_eq!(enc("\"\u{e9}\""), "62c3a9");
    assert_eq!(enc("{}"), "a0");
    assert_eq!(enc("[]"), "80");
    assert_eq!(enc("[1,[2],{}]"), "8301 81 02 a0".replace(' ', ""));
    // A string whose length needs the one-byte length form.
    assert_eq!(
        enc(&format!("\"{}\"", "x".repeat(24))),
        format!("7818{}", "78".repeat(24))
    );
    // Floats are always 64-bit; integral spellings with a point or exponent are floats too.
    assert_eq!(enc("1.5"), "fb3ff8000000000000");
    assert_eq!(enc("-0.0"), "fb8000000000000000");
    assert_eq!(enc("1.0"), "fb3ff0000000000000");
    assert_eq!(enc("1e2"), "fb4059000000000000");
}

#[test]
fn map_keys_sort_by_length_then_bytes() {
    // Input order b, a, aa, bbb, ab; DAG-CBOR order a, b, aa, ab, bbb (length first).
    assert_eq!(
        enc(r#"{"bbb":0,"ab":4,"aa":3,"b":1,"a":2}"#),
        "a5 6161 02 6162 01 626161 03 626162 04 63626262 00".replace(' ', "")
    );
    // Nested maps are sorted too; the key "10" (2 bytes) sorts after "z" (1 byte).
    assert_eq!(
        enc(r#"{"x":{"10":1,"z":2},"a":[{"q":1,"p":2}]}"#),
        "a2 6161 81 a2 6170 02 6171 01 6178 a2 617a 02 623130 01".replace(' ', "")
    );
}

#[test]
fn integers_out_of_range_and_non_finite_floats_are_errors() {
    assert_eq!(err("18446744073709551616"), "E_DAGCBOR_VALUE");
    assert_eq!(err("-18446744073709551617"), "E_DAGCBOR_VALUE");
}

#[test]
fn non_finite_float_is_error() {
    // 1e999 overflows f64 to infinity; JSON has no NaN or Infinity literal.
    assert_eq!(err("1e999"), "E_DAGCBOR_VALUE");
    assert_eq!(err("-1e999"), "E_DAGCBOR_VALUE");
    assert_eq!(err("[0.5, 1e400]"), "E_DAGCBOR_VALUE");
}

const CID: &str = "bafyriqa5k2d3t3r774geicueaed2wc2fosjwqeexfhwbptfgq7rcn5mwucnhfeuxu2nxbrch3rl6yqjlozhuswo5ln3xwjm35iftt3tpqlcgs";

#[test]
fn links_become_tag_42() {
    let got = enc(&format!(r#"{{"/":"{CID}"}}"#));
    // d8 2a = tag 42, 58 45 = 69-byte byte string: 0x00 identity prefix, then CIDv1 dag-cbor sha3-512.
    assert!(
        got.starts_with("d82a584500017114 40".replace(' ', "").as_str()),
        "{got}"
    );
    assert_eq!(got.len(), 2 * (2 + 2 + 69));
    // A link as a map value, among other keys.
    let nested = enc(&format!(r#"{{"cid":{{"/":"{CID}"}},"n":1}}"#));
    assert!(
        nested.starts_with("a2 616e 01 63636964 d82a5845".replace(' ', "").as_str()),
        "{nested}"
    );
    // Bytes: standard base64 with or without padding.
    assert_eq!(enc(r#"{"/":{"bytes":"AQID"}}"#), "43010203");
    assert_eq!(enc(r#"{"/":{"bytes":"AQI"}}"#), "420102");
    assert_eq!(enc(r#"{"/":{"bytes":"AQI="}}"#), "420102");
    assert_eq!(enc(r#"{"/":{"bytes":""}}"#), "40");
}

#[test]
fn bad_links_are_link_errors() {
    for json in [
        r#"{"/":"not a cid"}"#,
        r#"{"/":""}"#,
        r#"{"/":"QmYwAPJzv5CZsnA625s3Xf2nemtYgPpHdWEz79ojWnPbdG"}"#,
        // valid base32 but not a CID (version byte is not 1)
        r#"{"/":"baaaa"}"#,
        // truncated digest
        r#"{"/":"bafyriqa5k2d3t3r774geicueaed2wc2fosjwqeexfhwbptfgq7rcn5mwucnhfeuxu2nxbrch3rl6"}"#,
        r#"{"/":1}"#,
        r#"{"/":"b","x":1}"#,
        r#"{"/":{"bytes":"AQID","x":1}}"#,
        r#"{"/":{"bytes":"A"}}"#,
        r#"{"/":{"bytes":"A*ID"}}"#,
        r#"{"/":{"bytes":5}}"#,
        r#"{"/":{"other":"AQID"}}"#,
        r#"{"/":null}"#,
    ] {
        assert_eq!(err(json), "E_DAGCBOR_LINK", "{json}");
    }
}

#[test]
fn deep_nesting_is_an_error_not_a_crash() {
    // serde_json's parser stops at 128 levels, so build the deep value by hand.
    let mut value = Value::Array(vec![]);
    for _ in 0..1000 {
        value = Value::Array(vec![value]);
    }
    assert_eq!(dagcbor_encode(&value).unwrap_err().code, "E_DAGCBOR_VALUE");
    let mut object = Value::Null;
    for _ in 0..1000 {
        let mut map = serde_json::Map::new();
        map.insert("k".into(), object);
        object = Value::Object(map);
    }
    assert_eq!(dagcbor_encode(&object).unwrap_err().code, "E_DAGCBOR_VALUE");
    // Moderate nesting still encodes.
    let ok = format!("{}{}", "[".repeat(100), "]".repeat(100));
    let value: Value = serde_json::from_str(&ok).unwrap();
    assert!(dagcbor_encode(&value).is_ok());
}

fn base32_lower(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut out = String::new();
    let (mut buffer, mut bits) = (0u32, 0u32);
    for byte in bytes {
        buffer = (buffer << 8) | u32::from(*byte);
        bits += 8;
        while bits >= 5 {
            out.push(ALPHABET[((buffer >> (bits - 5)) & 31) as usize] as char);
            bits -= 5;
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

/// "b" + base32lower(0x01 0x71 0x14 0x40 || sha3_512(dag-cbor(value))): CIDv1, dag-cbor, SHA3-512.
fn cid_of(value: &Value) -> String {
    let mut bytes = vec![0x01, 0x71, 0x14, 0x40];
    bytes.extend_from_slice(&sha3_512(&dagcbor_encode(value).unwrap()));
    format!("b{}", base32_lower(&bytes))
}

/// The CURBy chain list (`GET https://random.colorado.edu/api/chains`) is an array of
/// `{ "cid": {"/": "<cid>"}, "data": {"content": {...}, "signature": "<JWS>"} }` records.
/// The CID is over the whole `data` object (content and signature), not `data.content` alone.
#[test]
fn curby_chain_cid_recomputes() {
    let chains: Value = serde_json::from_str(include_str!("fixtures/twine/chains.json")).unwrap();
    let records = chains.as_array().unwrap();
    assert!(!records.is_empty());
    for record in records {
        let claimed = record["cid"]["/"].as_str().unwrap();
        assert_eq!(cid_of(&record["data"]), claimed);
        // The content alone does not hash to the CID.
        assert_ne!(cid_of(&record["data"]["content"]), claimed);
    }
}
