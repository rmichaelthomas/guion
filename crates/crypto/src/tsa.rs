//! RFC 3161 time-stamp requests and token verification.
//!
//! [`tsa_request`] builds a DER `TimeStampReq`; [`tsa_response_token`] takes the `TimeStampToken`
//! out of a `TimeStampResp`; [`tsa_verify`] checks a token against a digest and a pinned TSA
//! certificate and its issuer; [`tsa_certificates`] lists the certificates a token carries.
//!
//! The ASN.1 here is read with a small strict-DER reader (definite, minimal lengths; no high tag
//! numbers) instead of the `cms` and `x509-tsp` crates: verification has to hash the exact bytes
//! of `eContent` and of the signed attributes as they appear in the token, and a hand reader
//! keeps those slices without re-encoding anything. Certificates are read with `x509-cert`, and
//! signatures go through the same code that checks certificate signatures. Nothing here panics:
//! malformed input is `E_TSA_FORMAT` (or `E_X509` for an unreadable certificate), well-formed
//! input that fails a check is `Ok(None)`.

use sha1::Sha1;
use sha2::{Digest, Sha256, Sha384, Sha512};
use x509_cert::der::{Decode, Encode};
use x509_cert::ext::pkix::{ExtendedKeyUsage, SubjectKeyIdentifier};
use x509_cert::Certificate;

use crate::signature::{
    certificate_signature_algorithm, key_curve, verify_under_key, x509_error, x509_validity,
    x509_verify_issued_by, CertHash,
};
use crate::CryptoError;

const SHA512_OID: &str = "2.16.840.1.101.3.4.2.3";
const SHA256_OID: &str = "2.16.840.1.101.3.4.2.1";
const SHA384_OID: &str = "2.16.840.1.101.3.4.2.2";
const SIGNED_DATA_OID: &str = "1.2.840.113549.1.7.2";
const TST_INFO_OID: &str = "1.2.840.113549.1.9.16.1.4";
const CONTENT_TYPE_OID: &str = "1.2.840.113549.1.9.3";
const MESSAGE_DIGEST_OID: &str = "1.2.840.113549.1.9.4";
const SIGNING_CERTIFICATE_OID: &str = "1.2.840.113549.1.9.16.2.12";
const SIGNING_CERTIFICATE_V2_OID: &str = "1.2.840.113549.1.9.16.2.47";
const TIME_STAMPING_EKU_OID: &str = "1.3.6.1.5.5.7.3.8";
const RSA_ENCRYPTION_OID: &str = "1.2.840.113549.1.1.1";
const RSA_PSS_OID: &str = "1.2.840.113549.1.1.10";
const EC_PUBLIC_KEY_OID: &str = "1.2.840.10045.2.1";

fn format_error(message: impl Into<String>) -> CryptoError {
    CryptoError::new("E_TSA_FORMAT", message)
}

// ---------------------------------------------------------------------------------------------
// A strict DER reader
// ---------------------------------------------------------------------------------------------

/// One DER element: its tag byte, its content, and its whole encoding (header included).
#[derive(Clone, Copy)]
struct Tlv<'a> {
    tag: u8,
    content: &'a [u8],
    full: &'a [u8],
}

/// A cursor over a sequence of DER elements.
#[derive(Clone, Copy)]
struct Reader<'a> {
    data: &'a [u8],
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Reader { data }
    }

    fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    fn peek_tag(&self) -> Option<u8> {
        self.data.first().copied()
    }

    fn next(&mut self) -> Result<Tlv<'a>, CryptoError> {
        let data = self.data;
        let tag = *data.first().ok_or_else(|| format_error("unexpected end"))?;
        if tag & 0x1f == 0x1f {
            return Err(format_error("high tag numbers are not used here"));
        }
        let first = *data
            .get(1)
            .ok_or_else(|| format_error("truncated length"))?;
        let (header, length) = if first < 0x80 {
            (2usize, usize::from(first))
        } else {
            let count = usize::from(first & 0x7f);
            if count == 0 {
                return Err(format_error("indefinite lengths are not DER"));
            }
            if count > 4 {
                return Err(format_error("length is too long"));
            }
            let bytes = data
                .get(2..2 + count)
                .ok_or_else(|| format_error("truncated length"))?;
            if bytes[0] == 0 {
                return Err(format_error("length is not minimal"));
            }
            let mut length = 0usize;
            for byte in bytes {
                length = (length << 8) | usize::from(*byte);
            }
            if length < 0x80 {
                return Err(format_error("length is not minimal"));
            }
            (2 + count, length)
        };
        let end = header
            .checked_add(length)
            .ok_or_else(|| format_error("length overflows"))?;
        let full = data
            .get(..end)
            .ok_or_else(|| format_error("an element runs past the end of its container"))?;
        self.data = &data[end..];
        Ok(Tlv {
            tag,
            content: &full[header..],
            full,
        })
    }

    fn expect(&mut self, tag: u8) -> Result<Tlv<'a>, CryptoError> {
        let element = self.next()?;
        if element.tag == tag {
            Ok(element)
        } else {
            Err(format_error(format!(
                "expected tag 0x{tag:02x}, found 0x{:02x}",
                element.tag
            )))
        }
    }

    /// The next element if its tag is `tag`, otherwise `None` and the cursor stays.
    fn optional(&mut self, tag: u8) -> Result<Option<Tlv<'a>>, CryptoError> {
        if self.peek_tag() == Some(tag) {
            Ok(Some(self.next()?))
        } else {
            Ok(None)
        }
    }

    fn finish(&self) -> Result<(), CryptoError> {
        if self.is_empty() {
            Ok(())
        } else {
            Err(format_error("unexpected trailing bytes"))
        }
    }
}

const SEQUENCE: u8 = 0x30;
const SET: u8 = 0x31;
const INTEGER: u8 = 0x02;
const OCTET_STRING: u8 = 0x04;
const OID: u8 = 0x06;
const BIT_STRING: u8 = 0x03;
const GENERALIZED_TIME: u8 = 0x18;
const BOOLEAN: u8 = 0x01;

fn oid_string(content: &[u8]) -> Result<String, CryptoError> {
    x509_cert::der::asn1::ObjectIdentifier::from_bytes(content)
        .map(|oid| oid.to_string())
        .map_err(|_| format_error("malformed object identifier"))
}

/// An `AlgorithmIdentifier`: its OID, and whether its parameters are absent or NULL.
fn algorithm(element: Tlv<'_>) -> Result<(String, bool), CryptoError> {
    if element.tag != SEQUENCE {
        return Err(format_error("AlgorithmIdentifier is not a SEQUENCE"));
    }
    let mut reader = Reader::new(element.content);
    let oid = oid_string(reader.expect(OID)?.content)?;
    let mut plain = true;
    if !reader.is_empty() {
        let parameters = reader.next()?;
        plain = parameters.tag == 0x05 && parameters.content.is_empty();
        reader.finish()?;
    }
    Ok((oid, plain))
}

/// The minimal unsigned big-endian bytes of a non-negative DER INTEGER (`[0]` for zero).
fn unsigned_bytes(content: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let first = *content
        .first()
        .ok_or_else(|| format_error("empty INTEGER"))?;
    if first & 0x80 != 0 {
        return Err(format_error(
            "negative INTEGER where a positive one is required",
        ));
    }
    if content.len() > 1 && first == 0 && content[1] & 0x80 == 0 {
        return Err(format_error("INTEGER is not minimal"));
    }
    if first == 0 && content.len() > 1 {
        Ok(content[1..].to_vec())
    } else {
        Ok(content.to_vec())
    }
}

fn small_unsigned(content: &[u8]) -> Result<u64, CryptoError> {
    let bytes = unsigned_bytes(content)?;
    if bytes.len() > 8 {
        return Err(format_error("INTEGER is too large"));
    }
    Ok(bytes
        .iter()
        .fold(0u64, |value, byte| (value << 8) | u64::from(*byte)))
}

/// A DER header (tag, then a definite length) around `body`.
fn der(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    let length = body.len();
    if length < 0x80 {
        out.push(length as u8);
    } else {
        let bytes = length.to_be_bytes();
        let skip = bytes.iter().take_while(|byte| **byte == 0).count();
        out.push(0x80 | (bytes.len() - skip) as u8);
        out.extend_from_slice(&bytes[skip..]);
    }
    out.extend_from_slice(body);
    out
}

// ---------------------------------------------------------------------------------------------
// Requests
// ---------------------------------------------------------------------------------------------

/// A DER `TimeStampReq` for a SHA-512 `digest` (64 bytes): version 1, the SHA-512 imprint with
/// an explicit NULL parameter (byte for byte what `openssl ts -query` writes; a response may carry
/// the algorithm with or without it), `certReq TRUE`, no policy, and
/// `nonce` as a DER INTEGER (omitted when empty). A nonce is at most 32 bytes and its first byte
/// is `0x01` to `0x7f`, so the INTEGER is minimal and positive and the bytes round-trip. A digest
/// that is not 64 bytes is `E_TSA_DIGEST`; a bad nonce is `E_TSA_NONCE`.
pub fn tsa_request(digest: &[u8], nonce: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if digest.len() != 64 {
        return Err(CryptoError::new(
            "E_TSA_DIGEST",
            format!(
                "a timestamp request carries a SHA-512 digest of 64 bytes; got {}",
                digest.len()
            ),
        ));
    }
    if nonce.len() > 32 {
        return Err(CryptoError::new(
            "E_TSA_NONCE",
            format!("a nonce is at most 32 bytes; got {}", nonce.len()),
        ));
    }
    if let Some(first) = nonce.first() {
        if *first == 0 || *first >= 0x80 {
            return Err(CryptoError::new(
                "E_TSA_NONCE",
                "a nonce's first byte must be 0x01 to 0x7f so that it is a minimal positive INTEGER",
            ));
        }
    }
    let oid = x509_cert::der::asn1::ObjectIdentifier::new_unwrap(SHA512_OID)
        .to_der()
        .map_err(|error| format_error(error.to_string()))?;
    let imprint = der(
        SEQUENCE,
        &[
            der(SEQUENCE, &[oid, vec![0x05, 0x00]].concat()),
            der(OCTET_STRING, digest),
        ]
        .concat(),
    );
    let mut body = der(INTEGER, &[1]);
    body.extend_from_slice(&imprint);
    if !nonce.is_empty() {
        body.extend_from_slice(&der(INTEGER, nonce));
    }
    body.extend_from_slice(&der(BOOLEAN, &[0xff]));
    Ok(der(SEQUENCE, &body))
}

// ---------------------------------------------------------------------------------------------
// Responses
// ---------------------------------------------------------------------------------------------

const STATUS_NAMES: [&str; 6] = [
    "granted",
    "grantedWithMods",
    "rejection",
    "waiting",
    "revocationWarning",
    "revocationNotification",
];

/// The `PKIFailureInfo` bit names RFC 3161 uses.
fn fail_info_name(bit: usize) -> Option<&'static str> {
    Some(match bit {
        0 => "badAlg",
        2 => "badRequest",
        5 => "badDataFormat",
        14 => "timeNotAvailable",
        15 => "unacceptedPolicy",
        16 => "unacceptedExtension",
        17 => "addInfoNotAvailable",
        25 => "systemFailure",
        _ => return None,
    })
}

/// The `timeStampToken` (a `ContentInfo`, DER) of a `TimeStampResp`. Status 0 (`granted`) and 1
/// (`grantedWithMods`) return it; statuses 2 to 5 are `E_TSA_STATUS` with the status, the
/// `statusString` and the `failInfo` bit names in the message; anything else that does not parse is
/// `E_TSA_FORMAT`.
pub fn tsa_response_token(response: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let mut outer = Reader::new(response);
    let sequence = outer.expect(SEQUENCE)?;
    outer.finish()?;
    let mut body = Reader::new(sequence.content);
    let status_info = body.expect(SEQUENCE)?;
    let mut info = Reader::new(status_info.content);
    let status = small_unsigned(info.expect(INTEGER)?.content)?;
    let mut strings = Vec::new();
    if let Some(text) = info.optional(SEQUENCE)? {
        let mut list = Reader::new(text.content);
        while !list.is_empty() {
            let string = list.expect(0x0c)?;
            strings.push(String::from_utf8_lossy(string.content).into_owned());
        }
    }
    let mut failures: Vec<String> = Vec::new();
    if let Some(bits) = info.optional(BIT_STRING)? {
        let (unused, bytes) = bits
            .content
            .split_first()
            .ok_or_else(|| format_error("empty failInfo"))?;
        if *unused > 7 {
            return Err(format_error("failInfo has an invalid unused-bits count"));
        }
        for (index, byte) in bytes.iter().enumerate() {
            for bit in 0..8 {
                if byte & (0x80 >> bit) != 0 {
                    let number = index * 8 + bit;
                    failures.push(
                        fail_info_name(number)
                            .map(str::to_string)
                            .unwrap_or_else(|| format!("bit {number}")),
                    );
                }
            }
        }
    }
    info.finish()?;
    if status > 5 {
        return Err(format_error(format!("unknown PKIStatus {status}")));
    }
    if status >= 2 {
        let text = if strings.is_empty() {
            "none".to_string()
        } else {
            format!("\"{}\"", strings.join("; "))
        };
        let fail = if failures.is_empty() {
            "none".to_string()
        } else {
            failures.join(", ")
        };
        return Err(CryptoError::new(
            "E_TSA_STATUS",
            format!(
                "the TSA did not grant the request: status {status} ({}); statusString {text}; failInfo {fail}",
                STATUS_NAMES[status as usize]
            ),
        ));
    }
    let token = body
        .optional(SEQUENCE)?
        .ok_or_else(|| format_error("a granted response carries no timeStampToken"))?;
    body.finish()?;
    Ok(token.full.to_vec())
}

// ---------------------------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------------------------

/// What a verified token says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TsaToken {
    /// `genTime` as Unix milliseconds (a finer fraction is truncated).
    pub gen_time_ms: i64,
    /// `accuracy` as milliseconds (microseconds rounded up); 0 when absent.
    pub accuracy_ms: i64,
    /// The serial number, minimal unsigned big-endian bytes.
    pub serial: Vec<u8>,
    /// The policy OID in dotted form.
    pub policy: String,
    /// The nonce, minimal unsigned big-endian bytes; empty when absent.
    pub nonce: Vec<u8>,
}

struct SignerInfo<'a> {
    sid: Tlv<'a>,
    digest_algorithm: (String, bool),
    signed_attrs: Tlv<'a>,
    signature_algorithm: Tlv<'a>,
    signature: &'a [u8],
}

struct ParsedToken<'a> {
    e_content: &'a [u8],
    certificates: Vec<&'a [u8]>,
    signer: SignerInfo<'a>,
}

fn parse_token(token: &[u8]) -> Result<ParsedToken<'_>, CryptoError> {
    let mut outer = Reader::new(token);
    let content_info = outer.expect(SEQUENCE)?;
    outer.finish()?;
    let mut info = Reader::new(content_info.content);
    if oid_string(info.expect(OID)?.content)? != SIGNED_DATA_OID {
        return Err(format_error("the token is not a CMS SignedData"));
    }
    let wrapper = info.expect(0xa0)?;
    info.finish()?;
    let mut wrapped = Reader::new(wrapper.content);
    let signed_data = wrapped.expect(SEQUENCE)?;
    wrapped.finish()?;
    let mut data = Reader::new(signed_data.content);
    data.expect(INTEGER)?;
    data.expect(SET)?;
    let encapsulated = data.expect(SEQUENCE)?;
    let mut encap = Reader::new(encapsulated.content);
    if oid_string(encap.expect(OID)?.content)? != TST_INFO_OID {
        return Err(format_error("the token does not carry a TSTInfo"));
    }
    let e_content_wrapper = encap
        .optional(0xa0)?
        .ok_or_else(|| format_error("the token carries no eContent"))?;
    encap.finish()?;
    let mut e_wrapped = Reader::new(e_content_wrapper.content);
    let e_content = e_wrapped.expect(OCTET_STRING)?.content;
    e_wrapped.finish()?;
    let mut certificates = Vec::new();
    if let Some(set) = data.optional(0xa0)? {
        let mut list = Reader::new(set.content);
        while !list.is_empty() {
            let choice = list.next()?;
            if choice.tag == SEQUENCE {
                certificates.push(choice.full);
            }
        }
    }
    // Revocation lists, if any, are not used.
    data.optional(0xa1)?;
    let signer_infos = data.expect(SET)?;
    data.finish()?;
    let mut signers = Reader::new(signer_infos.content);
    let signer = signers.expect(SEQUENCE)?;
    if !signers.is_empty() {
        return Err(format_error("the token has more than one SignerInfo"));
    }
    let mut fields = Reader::new(signer.content);
    fields.expect(INTEGER)?;
    let sid = fields.next()?;
    if sid.tag != SEQUENCE && sid.tag != 0x80 {
        return Err(format_error("unknown SignerIdentifier"));
    }
    let digest_algorithm = algorithm(fields.expect(SEQUENCE)?)?;
    let signed_attrs = fields
        .optional(0xa0)?
        .ok_or_else(|| format_error("the SignerInfo has no signed attributes"))?;
    let signature_algorithm = fields.expect(SEQUENCE)?;
    let signature = fields.expect(OCTET_STRING)?.content;
    // Unsigned attributes are ignored.
    fields.optional(0xa1)?;
    fields.finish()?;
    Ok(ParsedToken {
        e_content,
        certificates,
        signer: SignerInfo {
            sid,
            digest_algorithm,
            signed_attrs,
            signature_algorithm,
            signature,
        },
    })
}

/// The DER certificates embedded in a token's `SignedData`, in order.
pub fn tsa_certificates(token: &[u8]) -> Result<Vec<Vec<u8>>, CryptoError> {
    let parsed = parse_token(token)?;
    Ok(parsed.certificates.iter().map(|der| der.to_vec()).collect())
}

#[derive(Debug)]
struct TstInfo {
    policy: String,
    imprint_algorithm: String,
    imprint: Vec<u8>,
    serial: Vec<u8>,
    gen_time_ms: i64,
    accuracy_ms: i64,
    nonce: Vec<u8>,
}

fn parse_tst_info(content: &[u8]) -> Result<TstInfo, CryptoError> {
    let mut outer = Reader::new(content);
    let sequence = outer.expect(SEQUENCE)?;
    outer.finish()?;
    let mut fields = Reader::new(sequence.content);
    if small_unsigned(fields.expect(INTEGER)?.content)? != 1 {
        return Err(format_error("TSTInfo version is not 1"));
    }
    let policy = oid_string(fields.expect(OID)?.content)?;
    let imprint = fields.expect(SEQUENCE)?;
    let mut imprint_fields = Reader::new(imprint.content);
    let (imprint_algorithm, plain) = algorithm(imprint_fields.expect(SEQUENCE)?)?;
    if !plain {
        return Err(format_error(
            "the imprint hash algorithm has parameters other than NULL",
        ));
    }
    let digest = imprint_fields.expect(OCTET_STRING)?.content.to_vec();
    imprint_fields.finish()?;
    let serial = unsigned_bytes(fields.expect(INTEGER)?.content)?;
    let gen_time_ms = generalized_time_ms(fields.expect(GENERALIZED_TIME)?.content)?;
    let mut accuracy_ms = 0i64;
    if let Some(accuracy) = fields.optional(SEQUENCE)? {
        let mut parts = Reader::new(accuracy.content);
        let mut total: u64 = 0;
        if let Some(seconds) = parts.optional(INTEGER)? {
            let seconds = small_unsigned(seconds.content)?;
            if seconds > 1 << 40 {
                return Err(format_error("accuracy seconds are out of range"));
            }
            total += seconds * 1000;
        }
        if let Some(millis) = parts.optional(0x80)? {
            let millis = small_unsigned(millis.content)?;
            if !(1..=999).contains(&millis) {
                return Err(format_error("accuracy millis are outside 1 to 999"));
            }
            total += millis;
        }
        if let Some(micros) = parts.optional(0x81)? {
            let micros = small_unsigned(micros.content)?;
            if !(1..=999).contains(&micros) {
                return Err(format_error("accuracy micros are outside 1 to 999"));
            }
            total += micros.div_ceil(1000);
        }
        parts.finish()?;
        accuracy_ms = i64::try_from(total).map_err(|_| format_error("accuracy is out of range"))?;
    }
    fields.optional(BOOLEAN)?;
    let nonce = match fields.optional(INTEGER)? {
        Some(nonce) => unsigned_bytes(nonce.content)?,
        None => Vec::new(),
    };
    // `tsa [0]` and `extensions [1]` are not examined, but they must be well-formed elements.
    fields.optional(0xa0)?;
    fields.optional(0xa1)?;
    fields.finish()?;
    Ok(TstInfo {
        policy,
        imprint_algorithm,
        imprint: digest,
        serial,
        gen_time_ms,
        accuracy_ms,
        nonce,
    })
}

/// Days from 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// `YYYYMMDDHHMMSS[.f...]Z` as Unix milliseconds. The fraction (one or more digits) is truncated to
/// milliseconds. Only UTC (`Z`) is accepted, and a second of 60 (a leap second) is an error.
fn generalized_time_ms(content: &[u8]) -> Result<i64, CryptoError> {
    let bad = || format_error("genTime is not a UTC GeneralizedTime YYYYMMDDHHMMSS[.f]Z");
    let text = std::str::from_utf8(content).map_err(|_| bad())?;
    let body = text.strip_suffix('Z').ok_or_else(bad)?;
    let (whole, fraction) = match body.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (body, None),
    };
    if whole.len() != 14 || !whole.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(bad());
    }
    let number = |range: std::ops::Range<usize>| whole[range].parse::<i64>().map_err(|_| bad());
    let (year, month, day) = (number(0..4)?, number(4..6)?, number(6..8)?);
    let (hour, minute, second) = (number(8..10)?, number(10..12)?, number(12..14)?);
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => return Err(bad()),
    };
    if day < 1 || day > days_in_month || hour > 23 || minute > 59 || second > 59 {
        return Err(bad());
    }
    let mut millis = 0i64;
    if let Some(fraction) = fraction {
        if fraction.is_empty() || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(bad());
        }
        let mut digits = fraction.bytes().take(3).map(|byte| i64::from(byte - b'0'));
        for scale in [100, 10, 1] {
            millis += digits.next().unwrap_or(0) * scale;
        }
    }
    let seconds = days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second;
    Ok(seconds * 1000 + millis)
}

struct SignedAttributes<'a> {
    content_type: Option<String>,
    message_digest: Option<&'a [u8]>,
    signing_certificate_v2: Option<Tlv<'a>>,
    signing_certificate: Option<Tlv<'a>>,
}

fn parse_signed_attributes<'a>(set: Tlv<'a>) -> Result<SignedAttributes<'a>, CryptoError> {
    let mut attributes = SignedAttributes {
        content_type: None,
        message_digest: None,
        signing_certificate_v2: None,
        signing_certificate: None,
    };
    let mut list = Reader::new(set.content);
    while !list.is_empty() {
        let attribute = list.expect(SEQUENCE)?;
        let mut fields = Reader::new(attribute.content);
        let oid = oid_string(fields.expect(OID)?.content)?;
        let values = fields.expect(SET)?;
        fields.finish()?;
        let mut value_reader = Reader::new(values.content);
        let value = value_reader.next()?;
        value_reader.finish()?;
        let duplicate = || format_error(format!("signed attribute {oid} occurs twice"));
        match oid.as_str() {
            CONTENT_TYPE_OID => {
                if attributes.content_type.is_some() {
                    return Err(duplicate());
                }
                if value.tag != OID {
                    return Err(format_error("contentType is not an OID"));
                }
                attributes.content_type = Some(oid_string(value.content)?);
            }
            MESSAGE_DIGEST_OID => {
                if attributes.message_digest.is_some() {
                    return Err(duplicate());
                }
                if value.tag != OCTET_STRING {
                    return Err(format_error("messageDigest is not an OCTET STRING"));
                }
                attributes.message_digest = Some(value.content);
            }
            SIGNING_CERTIFICATE_V2_OID => {
                if attributes.signing_certificate_v2.replace(value).is_some() {
                    return Err(duplicate());
                }
            }
            SIGNING_CERTIFICATE_OID => {
                if attributes.signing_certificate.replace(value).is_some() {
                    return Err(duplicate());
                }
            }
            _ => {}
        }
    }
    Ok(attributes)
}

/// The hash an ESSCertID names, as bytes to compare, from the first certificate id of a
/// `SigningCertificate` (v1, SHA-1) or `SigningCertificateV2` (SHA-256 unless it says otherwise).
/// `issuerSerial` is deliberately not compared: at least one public TSA writes it wrongly, and the
/// hash of the whole certificate already names it.
fn ess_certificate_matches(
    value: Tlv<'_>,
    version2: bool,
    certificate_der: &[u8],
) -> Result<bool, CryptoError> {
    if value.tag != SEQUENCE {
        return Err(format_error(
            "signing certificate attribute is not a SEQUENCE",
        ));
    }
    let mut fields = Reader::new(value.content);
    let certs = fields.expect(SEQUENCE)?;
    // `policies` (optional) is ignored.
    let mut ids = Reader::new(certs.content);
    let first = ids.expect(SEQUENCE)?;
    let mut id = Reader::new(first.content);
    let mut hash = if version2 { "sha256" } else { "sha1" };
    if version2 {
        if let Some(element) = id.optional(SEQUENCE)? {
            hash = match algorithm(element)?.0.as_str() {
                SHA256_OID => "sha256",
                SHA384_OID => "sha384",
                SHA512_OID => "sha512",
                other => {
                    return Err(CryptoError::new(
                        "E_TSA_ALG",
                        format!("ESSCertIDv2 hash algorithm {other} is not supported"),
                    ))
                }
            };
        }
    }
    let wanted = id.expect(OCTET_STRING)?.content;
    let actual = match hash {
        "sha1" => Sha1::digest(certificate_der).to_vec(),
        "sha384" => Sha384::digest(certificate_der).to_vec(),
        "sha512" => Sha512::digest(certificate_der).to_vec(),
        _ => Sha256::digest(certificate_der).to_vec(),
    };
    Ok(wanted == actual.as_slice())
}

fn signer_hash(oid: &str) -> Result<CertHash, CryptoError> {
    CertHash::from_oid(oid).ok_or_else(|| {
        CryptoError::new(
            "E_TSA_ALG",
            format!("signer digest algorithm {oid} is not supported (SHA-256, SHA-384 or SHA-512)"),
        )
    })
}

/// Whether the `SignerInfo` names `certificate` (by issuer and serial number, or by key identifier).
fn signer_identifier_matches(sid: Tlv<'_>, certificate: &Certificate) -> Result<bool, CryptoError> {
    if sid.tag == 0x80 {
        return Ok(certificate
            .tbs_certificate
            .get::<SubjectKeyIdentifier>()
            .map_err(|error| x509_error(format!("malformed SubjectKeyIdentifier: {error}")))?
            .map(|(_, identifier)| identifier.0.as_bytes() == sid.content)
            .unwrap_or(false));
    }
    let mut fields = Reader::new(sid.content);
    let issuer = fields.expect(SEQUENCE)?;
    let serial = fields.expect(INTEGER)?;
    fields.finish()?;
    let certificate_issuer = certificate
        .tbs_certificate
        .issuer
        .to_der()
        .map_err(|error| x509_error(format!("certificate issuer name: {error}")))?;
    let strip = |bytes: &[u8]| {
        let start = bytes
            .iter()
            .position(|byte| *byte != 0)
            .unwrap_or(bytes.len().saturating_sub(1));
        bytes[start..].to_vec()
    };
    Ok(issuer.full == certificate_issuer.as_slice()
        && strip(serial.content) == strip(certificate.tbs_certificate.serial_number.as_bytes()))
}

/// Verifies the CMS signature over the signed attributes with the key of `tsa`.
fn signature_verifies(
    signer: &SignerInfo<'_>,
    digest_hash: CertHash,
    tsa_der: &[u8],
    tsa: &Certificate,
) -> Result<bool, CryptoError> {
    use crate::signature::CertSignature;
    // The signature covers the attributes as a DER SET OF, not with their [0] tag.
    let message = der(SET, signer.signed_attrs.content);
    let algorithm_der = signer.signature_algorithm.full;
    let parsed = x509_cert::spki::AlgorithmIdentifierOwned::from_der(algorithm_der)
        .map_err(|_| format_error("malformed signature AlgorithmIdentifier"))?;
    let oid = parsed.oid.to_string();
    let info = &tsa.tbs_certificate.subject_public_key_info;
    // The shared key check reports `E_X509_ALG`; for the TSA's own key it is `E_TSA_ALG`.
    let curve = key_curve(info).map_err(|error| {
        if error.code == "E_X509_ALG" {
            CryptoError::new(
                "E_TSA_ALG",
                format!("TSA certificate key: {}", error.message),
            )
        } else {
            error
        }
    })?;
    let disagree = |other: &str| {
        CryptoError::new(
            "E_TSA_ALG",
            format!("signature algorithm {other} disagrees with the signer digest algorithm"),
        )
    };
    let algorithm = match oid.as_str() {
        // The signer digest algorithm picks the hash.
        RSA_ENCRYPTION_OID => CertSignature::RsaPkcs1v15(digest_hash),
        "1.2.840.113549.1.1.11" | "1.2.840.113549.1.1.12" | "1.2.840.113549.1.1.13" => {
            let named = certificate_signature_algorithm(&parsed)?;
            match named {
                CertSignature::RsaPkcs1v15(hash) if hash == digest_hash => named,
                _ => return Err(disagree(&oid)),
            }
        }
        RSA_PSS_OID => {
            let named = certificate_signature_algorithm(&parsed)?;
            match named {
                CertSignature::RsaPss(hash, _) if hash == digest_hash => named,
                _ => return Err(disagree(&oid)),
            }
        }
        EC_PUBLIC_KEY_OID => CertSignature::Ecdsa(digest_hash),
        "1.2.840.10045.4.3.2" | "1.2.840.10045.4.3.3" | "1.2.840.10045.4.3.4" => {
            let named = certificate_signature_algorithm(&parsed)?;
            match named {
                CertSignature::Ecdsa(hash) if hash == digest_hash => named,
                _ => return Err(disagree(&oid)),
            }
        }
        other => {
            return Err(CryptoError::new(
                "E_TSA_ALG",
                format!("signature algorithm {other} is not supported (RSA PKCS#1 v1.5 or PSS, ECDSA P-256 or P-384)"),
            ))
        }
    };
    verify_under_key(tsa_der, info, algorithm, curve, &message, signer.signature)
}

/// Verifies an RFC 3161 `token` (a `ContentInfo` DER) for `digest` (SHA-512, as `tsa_request`
/// makes it) against a pinned TSA certificate and its issuer, and returns what it says.
///
/// `Ok(Some)` only when all of these hold; any other well-formed outcome is `Ok(None)`:
///
/// 1. The token is `SignedData` whose `eContentType` is `id-ct-TSTInfo`, with one `SignerInfo` and
///    signed attributes; otherwise `E_TSA_FORMAT`.
/// 2. `TSTInfo.messageImprint` is SHA-512 and equals `digest`.
/// 3. The signed attribute `contentType` is `id-ct-TSTInfo`, and `messageDigest` is the signer's
///    digest algorithm (SHA-256, SHA-384 or SHA-512; another is `E_TSA_ALG`) of the `eContent` bytes.
/// 4. `signingCertificateV2` (ESSCertIDv2, SHA-256 by default) or `signingCertificate` (ESSCertID,
///    SHA-1) is present and its hash is that of `tsa_cert_der`; every one present must match.
/// 5. The `SignerInfo` names `tsa_cert_der` (issuer and serial number, or subject key identifier).
/// 6. The signature over the DER `SET OF` signed attributes verifies under the key of
///    `tsa_cert_der`: RSA PKCS#1 v1.5, RSA-PSS, ECDSA P-256 or P-384 (another algorithm is `E_TSA_ALG`).
/// 7. `tsa_cert_der` was issued by `issuer_cert_der` ([`x509_verify_issued_by`]).
/// 8. `tsa_cert_der` has the extended key usage `id-kp-timeStamping`. RFC 3161 says the extension
///    must be critical; that is not enforced (an issuer's profile does that, and a pinned
///    certificate is already trusted by being pinned).
/// 9. `genTime` is within the validity of `tsa_cert_der`.
///
/// A `digest` that is not 64 bytes is `E_TSA_DIGEST`. A TSA certificate whose key is not RSA,
/// P-256 or P-384 is `E_TSA_ALG`. An unreadable certificate is `E_X509`. The certificate's own validity against today's date and
/// revocation are not examined; the time a token proves is its `genTime`, not the clock.
pub fn tsa_verify(
    token: &[u8],
    digest: &[u8],
    tsa_cert_der: &[u8],
    issuer_cert_der: &[u8],
) -> Result<Option<TsaToken>, CryptoError> {
    if digest.len() != 64 {
        return Err(CryptoError::new(
            "E_TSA_DIGEST",
            format!(
                "the digest is {} bytes; a SHA-512 digest is 64 bytes",
                digest.len()
            ),
        ));
    }
    let parsed = parse_token(token)?;
    let tst = parse_tst_info(parsed.e_content)?;
    let attributes = parse_signed_attributes(parsed.signer.signed_attrs)?;
    let tsa = Certificate::from_der(tsa_cert_der)
        .map_err(|error| x509_error(format!("not a valid DER X.509 certificate: {error}")))?;
    Certificate::from_der(issuer_cert_der).map_err(|error| {
        x509_error(format!(
            "issuer is not a valid DER X.509 certificate: {error}"
        ))
    })?;

    // 2
    if tst.imprint_algorithm != SHA512_OID || tst.imprint != digest {
        return Ok(None);
    }
    // 3
    if attributes.content_type.as_deref() != Some(TST_INFO_OID) {
        return Ok(None);
    }
    let digest_hash = signer_hash(&parsed.signer.digest_algorithm.0)?;
    if !parsed.signer.digest_algorithm.1 {
        return Err(format_error(
            "the signer digest algorithm has parameters other than NULL",
        ));
    }
    match attributes.message_digest {
        Some(wanted) if wanted == digest_hash.digest(parsed.e_content).as_slice() => {}
        _ => return Ok(None),
    }
    // 4
    if attributes.signing_certificate_v2.is_none() && attributes.signing_certificate.is_none() {
        return Ok(None);
    }
    if let Some(value) = attributes.signing_certificate_v2 {
        if !ess_certificate_matches(value, true, tsa_cert_der)? {
            return Ok(None);
        }
    }
    if let Some(value) = attributes.signing_certificate {
        if !ess_certificate_matches(value, false, tsa_cert_der)? {
            return Ok(None);
        }
    }
    // 5
    if !signer_identifier_matches(parsed.signer.sid, &tsa)? {
        return Ok(None);
    }
    // 6
    if !signature_verifies(&parsed.signer, digest_hash, tsa_cert_der, &tsa)? {
        return Ok(None);
    }
    // 7
    if !x509_verify_issued_by(tsa_cert_der, issuer_cert_der)? {
        return Ok(None);
    }
    // 8
    let has_time_stamping = tsa
        .tbs_certificate
        .get::<ExtendedKeyUsage>()
        .map_err(|error| x509_error(format!("malformed ExtendedKeyUsage: {error}")))?
        .map(|(_, usage)| {
            usage
                .0
                .iter()
                .any(|oid| oid.to_string() == TIME_STAMPING_EKU_OID)
        })
        .unwrap_or(false);
    if !has_time_stamping {
        return Ok(None);
    }
    // 9
    let [not_before, not_after] = x509_validity(tsa_cert_der)?;
    let generated = tst.gen_time_ms.div_euclid(1000);
    if generated < not_before || generated > not_after {
        return Ok(None);
    }
    Ok(Some(TsaToken {
        gen_time_ms: tst.gen_time_ms,
        accuracy_ms: tst.accuracy_ms,
        serial: tst.serial,
        policy: tst.policy,
        nonce: tst.nonce,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generalized_time_fractions_and_failures() {
        // 2026-11-23 19:40:59 UTC
        let base = 1_795_462_859_000i64;
        assert_eq!(generalized_time_ms(b"20261123194059Z").unwrap(), base);
        for (text, extra) in [
            ("20261123194059.5Z", 500),
            ("20261123194059.12Z", 120),
            ("20261123194059.133Z", 133),
            ("20261123194059.1234Z", 123),
            ("20261123194059.12345Z", 123),
            ("20261123194059.147944Z", 147),
            ("20261123194059.9999999Z", 999),
            ("20261123194059.0Z", 0),
        ] {
            assert_eq!(
                generalized_time_ms(text.as_bytes()).unwrap(),
                base + extra,
                "{text}"
            );
        }
        for bad in [
            "20261123194060Z",
            "20261123194059",
            "20261123194059+0100",
            "20261123194059.Z",
            "20261123194059,5Z",
            "2026112319405Z",
            "20261323194059Z",
            "20261123244059Z",
            "20260230000000Z",
            "261123194059Z",
            "",
        ] {
            let error = generalized_time_ms(bad.as_bytes()).unwrap_err();
            assert_eq!(error.code, "E_TSA_FORMAT", "{bad}");
        }
        assert_eq!(
            generalized_time_ms(b"20240229000000Z").unwrap(),
            1_709_164_800_000
        );
        assert_eq!(generalized_time_ms(b"19700101000000Z").unwrap(), 0);
    }

    fn status_response(status: u8, text: Option<&str>, fail_bits: Option<&[u8]>) -> Vec<u8> {
        let mut info = der(INTEGER, &[status]);
        if let Some(text) = text {
            info.extend(der(SEQUENCE, &der(0x0c, text.as_bytes())));
        }
        if let Some(bits) = fail_bits {
            info.extend(der(BIT_STRING, bits));
        }
        der(SEQUENCE, &der(SEQUENCE, &info))
    }

    #[test]
    fn refusals_name_status_text_and_bits() {
        // status 2, "no", failInfo bit 15 (unacceptedPolicy) and bit 0 (badAlg).
        let response = status_response(2, Some("no"), Some(&[0, 0x80, 0x01]));
        let error = tsa_response_token(&response).unwrap_err();
        assert_eq!(error.code, "E_TSA_STATUS");
        assert!(
            error.message.contains("status 2 (rejection)"),
            "{}",
            error.message
        );
        assert!(error.message.contains("\"no\""));
        assert!(error.message.contains("badAlg"));
        assert!(error.message.contains("unacceptedPolicy"));
        for (status, name) in [
            (3, "waiting"),
            (4, "revocationWarning"),
            (5, "revocationNotification"),
        ] {
            let error = tsa_response_token(&status_response(status, None, None)).unwrap_err();
            assert_eq!(error.code, "E_TSA_STATUS");
            assert!(error.message.contains(&format!("status {status} ({name})")));
        }
        assert_eq!(
            tsa_response_token(&status_response(6, None, None))
                .unwrap_err()
                .code,
            "E_TSA_FORMAT"
        );
        // Granted but no token.
        assert_eq!(
            tsa_response_token(&status_response(0, None, None))
                .unwrap_err()
                .code,
            "E_TSA_FORMAT"
        );
    }

    #[test]
    fn granted_response_returns_the_token_bytes() {
        let token = der(SEQUENCE, &der(OID, &[0x2a, 0x03]));
        let mut body = der(SEQUENCE, &der(INTEGER, &[1]));
        body.extend(&token);
        let response = der(SEQUENCE, &body);
        assert_eq!(tsa_response_token(&response).unwrap(), token);
    }

    #[test]
    fn partial_accuracy_is_read() {
        fn tst(accuracy: &[u8]) -> Vec<u8> {
            let oid = x509_cert::der::asn1::ObjectIdentifier::new_unwrap(SHA512_OID)
                .to_der()
                .unwrap();
            let mut body = der(INTEGER, &[1]);
            body.extend(der(OID, &[0x2a, 0x03, 0x04]));
            body.extend(der(
                SEQUENCE,
                &[der(SEQUENCE, &oid), der(OCTET_STRING, &[7; 64])].concat(),
            ));
            body.extend(der(INTEGER, &[5]));
            body.extend(der(GENERALIZED_TIME, b"20261123194059Z"));
            body.extend(der(SEQUENCE, accuracy));
            der(SEQUENCE, &body)
        }
        let seconds_only = tst(&der(INTEGER, &[2]));
        assert_eq!(parse_tst_info(&seconds_only).unwrap().accuracy_ms, 2000);
        let millis_only = tst(&der(0x80, &[0x01, 0xf4]));
        assert_eq!(parse_tst_info(&millis_only).unwrap().accuracy_ms, 500);
        let micros_only = tst(&der(0x81, &[0x64]));
        assert_eq!(parse_tst_info(&micros_only).unwrap().accuracy_ms, 1);
        let micros_exact = tst(&der(0x81, &[0x03, 0xe7]));
        assert_eq!(parse_tst_info(&micros_exact).unwrap().accuracy_ms, 1);
        let all = tst(&[
            der(INTEGER, &[1]),
            der(0x80, &[0x01, 0xf4]),
            der(0x81, &[0x64]),
        ]
        .concat());
        let parsed = parse_tst_info(&all).unwrap();
        assert_eq!(parsed.accuracy_ms, 1501);
        assert_eq!(parsed.serial, vec![5]);
        assert!(parsed.nonce.is_empty());
        let empty = tst(&[]);
        assert_eq!(parse_tst_info(&empty).unwrap().accuracy_ms, 0);
        let zero_millis = tst(&der(0x80, &[0]));
        assert_eq!(
            parse_tst_info(&zero_millis).unwrap_err().code,
            "E_TSA_FORMAT"
        );
    }

    #[test]
    fn der_reader_rejects_what_der_forbids() {
        for bad in [
            &[0x30u8, 0x80, 0x00, 0x00][..],
            &[0x30, 0x81, 0x05, 1, 2, 3, 4, 5],
            &[0x30, 0x82, 0x00, 0x05],
            &[0x1f, 0x01, 0x00],
            &[0x30, 0x05, 1],
            &[0x30],
            &[],
        ] {
            assert!(Reader::new(bad).next().is_err());
        }
    }
}
