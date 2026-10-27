use std::{
    fs,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use guion_model::{Artifact, BigInt, SharedJson, Value};
use guion_runtime::stdlib_services::{
    calendar_parse, crypto_hmac_sha256, crypto_verify_hmac_sha256, text_casefold,
    text_collapse_whitespace, text_collapse_whitespace_if_changed, text_normalize, CalendarEvent,
    DatabaseRegistry, SqlValue, TomlDocument, TomlRegistry,
};
use guion_runtime::{HostPolicy, Runtime};

const SELF_HOSTED_COMPILER: &[u8] = include_bytes!("../../../language/self_host/compiler.gub");

fn temp_path(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("guion-{name}-{}-{nonce}", std::process::id()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn hmac_matches_rfc4231_and_rejects_wrong_digest() {
    let key = vec![0x0b; 20];
    let digest = crypto_hmac_sha256(&key, b"Hi There");
    assert_eq!(
        hex(&digest),
        "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
    );
    assert!(crypto_verify_hmac_sha256(&key, b"Hi There", &digest));
    assert!(!crypto_verify_hmac_sha256(&key, b"Hi There", &[0; 32]));
    assert!(!crypto_verify_hmac_sha256(&key, b"Hi There", &[0; 31]));
    assert_ne!(crypto_hmac_sha256(&key, b"Hi There\n"), digest);
}

#[test]
fn unicode_casefold_and_whitespace_use_unicode_properties_without_normalizing() {
    assert_eq!(text_casefold("Straße Σς K"), "strasse σσ k");
    assert_eq!(
        text_casefold("Straße"),
        text_casefold("STRASSE"),
        "aliases differing only by full Unicode case folding resolve to one key"
    );
    assert_eq!(text_collapse_whitespace(" a\u{2003}\tb\u{00a0} "), "a b");
    let decomposed = "e\u{301}";
    assert_eq!(text_casefold(decomposed), decomposed);
    assert_eq!(text_normalize(decomposed, "NFC").unwrap(), "é");
    assert!(text_normalize("x", "unknown").is_err());
}

#[test]
fn whitespace_collapse_fast_path_only_returns_changes_when_needed() {
    assert_eq!(text_collapse_whitespace_if_changed("compact JSON"), None);
    assert_eq!(text_collapse_whitespace_if_changed("one space"), None);
    assert_eq!(
        text_collapse_whitespace_if_changed(" \t one\u{2003}two  "),
        Some("one two".to_owned())
    );
    assert_eq!(
        text_collapse_whitespace_if_changed("a\u{00a0}b"),
        Some("a b".to_owned())
    );
}

#[test]
fn sqlite_bound_parameters_commit_and_survive_reopen() {
    let path = temp_path("services.sqlite");
    let mut dbs = DatabaseRegistry::with_roots([std::env::temp_dir()], 1024 * 1024, 100, 2_000);
    let db = dbs.open(&path).unwrap();
    let tx = dbs.begin(db).unwrap();
    dbs.execute(tx, "CREATE TABLE records (value TEXT NOT NULL)", &[])
        .unwrap();
    dbs.execute(
        tx,
        "INSERT INTO records(value) VALUES (?1)",
        &[SqlValue::Text("x'); DROP TABLE records; --".into())],
    )
    .unwrap();
    let rows = dbs.query(tx, "SELECT value FROM records", &[]).unwrap();
    assert_eq!(
        rows[0].values,
        vec![SqlValue::Text("x'); DROP TABLE records; --".into())]
    );
    dbs.commit(tx).unwrap();
    dbs.close(db).unwrap();

    let reopened = dbs.open(&path).unwrap();
    let tx = dbs.begin(reopened).unwrap();
    assert_eq!(
        dbs.query(tx, "SELECT count(*) FROM records", &[]).unwrap()[0].values,
        vec![SqlValue::Integer(1)]
    );
    dbs.rollback(tx).unwrap();
    dbs.close(reopened).unwrap();
    let _ = fs::remove_file(path);
}

#[test]
fn sqlite_rejects_nested_and_cross_registry_handles() {
    let path = temp_path("ownership.sqlite");
    let mut first = DatabaseRegistry::with_roots([std::env::temp_dir()], 1024, 10, 1_000);
    let mut second = DatabaseRegistry::with_roots([std::env::temp_dir()], 1024, 10, 1_000);
    let db = first.open(&path).unwrap();
    assert_eq!(second.begin(db).unwrap_err().code, "E_RESOURCE_OWNER");
    let tx = first.begin(db).unwrap();
    assert!(first.begin(db).is_err());
    first.close(db).unwrap();
    assert_eq!(first.rollback(tx).unwrap_err().code, "E_RESOURCE_CLOSED");
    let (owner, id) = db.parts();
    let stale = guion_runtime::stdlib_services::DbHandle::from_parts(owner, id + 10_000);
    assert_eq!(first.begin(stale).unwrap_err().code, "E_RESOURCE_STALE");
    let _ = fs::remove_file(path);
}

#[test]
fn toml_resources_reject_foreign_and_invocation_stale_handles() {
    let mut first = TomlRegistry::new(1024);
    let second = TomlRegistry::new(1024);
    let handle = first.parse("value = 1\n").unwrap();
    assert_eq!(second.render(handle).unwrap_err().code, "E_RESOURCE_OWNER");
    first.close_all();
    assert_eq!(first.render(handle).unwrap_err().code, "E_RESOURCE_STALE");
    let (owner, id) = handle.parts();
    let stale = guion_runtime::stdlib_services::TomlHandle::from_parts(owner, id + 1);
    assert_eq!(first.render(stale).unwrap_err().code, "E_RESOURCE_STALE");
}

#[test]
fn sqlite_denies_paths_and_sql_that_escape_the_granted_database_root() {
    let path = temp_path("denied.sqlite");
    let mut databases = DatabaseRegistry::with_roots(
        [std::env::temp_dir().join("guion-no-such-root")],
        1024,
        10,
        50,
    );
    assert!(databases.open(&path).is_err());

    let mut databases = DatabaseRegistry::with_roots([std::env::temp_dir()], 1024, 10, 50);
    let db = databases.open(&path).unwrap();
    let tx = databases.begin(db).unwrap();
    assert!(databases
        .execute(tx, "ATTACH DATABASE '/tmp/escape.sqlite' AS escape", &[])
        .is_err());
    assert!(databases
        .execute(tx, "VACUUM INTO '/tmp/escape.sqlite'", &[])
        .is_err());
    databases.rollback(tx).unwrap();
    databases.close(db).unwrap();
    let _ = fs::remove_file(path);
}

#[test]
fn dropping_a_registry_rolls_back_open_transactions_and_reopens_durably() {
    let path = temp_path("rollback.sqlite");
    {
        let mut databases = DatabaseRegistry::with_roots([std::env::temp_dir()], 1024, 10, 100);
        let db = databases.open(&path).unwrap();
        let tx = databases.begin(db).unwrap();
        databases
            .execute(tx, "CREATE TABLE records (value INTEGER)", &[])
            .unwrap();
        databases.commit(tx).unwrap();
        let tx = databases.begin(db).unwrap();
        databases
            .execute(tx, "INSERT INTO records(value) VALUES (1)", &[])
            .unwrap();
    }
    let mut databases = DatabaseRegistry::with_roots([std::env::temp_dir()], 1024, 10, 100);
    let db = databases.open(&path).unwrap();
    let tx = databases.begin(db).unwrap();
    let rows = databases
        .query(tx, "SELECT count(*) FROM records", &[])
        .unwrap();
    assert_eq!(rows[0].values, vec![SqlValue::Integer(0)]);
    databases.rollback(tx).unwrap();
    databases.close(db).unwrap();
    let _ = fs::remove_file(path);
}

#[test]
fn concurrent_sqlite_writer_observes_a_finite_busy_deadline() {
    let path = temp_path("writer-lock.sqlite");
    let root = std::env::temp_dir();
    let mut first = DatabaseRegistry::with_roots([root.clone()], 1024, 10, 40);
    let mut second = DatabaseRegistry::with_roots([root], 1024, 10, 40);
    let first_db = first.open(&path).unwrap();
    let first_tx = first.begin(first_db).unwrap();
    let second_db = second.open(&path).unwrap();
    let started = std::time::Instant::now();
    assert!(second.begin(second_db).is_err());
    assert!(started.elapsed() < Duration::from_secs(1));
    first.rollback(first_tx).unwrap();
    first.close(first_db).unwrap();
    second.close(second_db).unwrap();
    let _ = fs::remove_file(path);
}

#[test]
fn sqlite_cancellation_interrupts_work_and_close_releases_open_resources() {
    let path = temp_path("cancelled-work.sqlite");
    let mut databases = DatabaseRegistry::with_roots([std::env::temp_dir()], 1024, 10, 500);
    let db = databases.open(&path).unwrap();
    let tx = databases.begin(db).unwrap();
    databases
        .execute(tx, "CREATE TABLE values_table (value INTEGER)", &[])
        .unwrap();
    databases.commit(tx).unwrap();
    databases.close(db).unwrap();
    for _ in 0..8 {
        let db = databases.open(&path).unwrap();
        let tx = databases.begin(db).unwrap();
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        databases.set_cancelled(tx, cancelled).unwrap();
        let error = databases.execute(
            tx,
            "WITH RECURSIVE counter(value) AS (SELECT 1 UNION ALL SELECT value + 1 FROM counter WHERE value < 100000000) INSERT INTO values_table SELECT value FROM counter",
            &[],
        ).expect_err("a cancelled SQLite statement must be interrupted");
        assert_eq!(error.code, "E_SQLITE_CANCELLED");
        databases.rollback(tx).unwrap();
        databases.close(db).unwrap();
        assert_eq!(databases.open_count(), 0);
        assert_eq!(databases.transaction_count(), 0);
    }
    let db = databases.open(&path).unwrap();
    let tx = databases.begin(db).unwrap();
    assert_eq!(
        databases
            .query(tx, "SELECT count(*) FROM values_table", &[])
            .unwrap()[0]
            .values,
        vec![SqlValue::Integer(0)]
    );
    databases.rollback(tx).unwrap();
    databases.close(db).unwrap();
    let _ = fs::remove_file(path);
}

#[test]
fn sqlite_result_budgets_and_invalid_text_are_typed_errors() {
    let path = temp_path("result-limits.sqlite");
    let mut row_limited = DatabaseRegistry::with_roots([std::env::temp_dir()], 4096, 1, 500);
    let db = row_limited.open(&path).unwrap();
    let tx = row_limited.begin(db).unwrap();
    let row_error = row_limited
        .query(tx, "SELECT 1 AS value UNION ALL SELECT 2", &[])
        .unwrap_err();
    assert_eq!(row_error.code, "E_SQLITE_RESULT_LIMIT");
    let utf8_error = row_limited
        .query(tx, "SELECT CAST(x'80' AS TEXT) AS value", &[])
        .unwrap_err();
    assert_eq!(utf8_error.code, "E_SQLITE_UTF8");
    row_limited.rollback(tx).unwrap();
    row_limited.close(db).unwrap();

    let mut byte_limited = DatabaseRegistry::with_roots([std::env::temp_dir()], 8, 10, 500);
    let db = byte_limited.open(&path).unwrap();
    let tx = byte_limited.begin(db).unwrap();
    let byte_error = byte_limited
        .query(tx, "SELECT 'a long returned value' AS value", &[])
        .unwrap_err();
    assert_eq!(byte_error.code, "E_SQLITE_RESULT_LIMIT");
    byte_limited.rollback(tx).unwrap();
    byte_limited.close(db).unwrap();
    let _ = fs::remove_file(path);
}

#[test]
fn toml_noop_is_byte_exact_and_edits_keep_unrelated_comments() {
    let source = "# top\n[server]\n# keep\nport = 8080 # inline\nname = 'demo'\n";
    let mut document = TomlDocument::parse(source).unwrap();
    assert_eq!(document.render(), source);
    document.put_integer(&["server", "port"], 9090).unwrap();
    let rendered = document.render();
    assert!(rendered.contains("# top\n"));
    assert!(rendered.contains("# keep\n"), "rendered TOML: {rendered:?}");
    assert!(rendered.contains("port = 9090 # inline"));
    assert_eq!(document.get_integer(&["server", "port"]).unwrap(), 9090);
    assert!(document.get_bool(&["server", "port"]).is_err());
    assert!(TomlDocument::parse("name = 'one'\nname = 'two'\n").is_err());
}

#[test]
fn toml_targeted_scalar_edit_preserves_dotted_order_line_endings_and_final_newline() {
    let source = "z = 1\r\na.b=2\r\nlast = 3";
    let mut document = TomlDocument::parse(source).unwrap();
    document.put_integer(&["a", "b"], 2).unwrap();
    assert_eq!(document.render(), source);
    assert!(document.put_bool(&["a", "b"], true).is_err());
    assert_eq!(document.render(), source);
    document.put_integer(&["a", "b"], 9).unwrap();
    assert_eq!(document.render(), "z = 1\r\na.b=9\r\nlast = 3");
}

#[test]
fn toml_insert_into_existing_table_preserves_order_comments_and_line_endings() {
    let source = "z = 1\r\na.b=2\r\n[server]\r\n# preserve this comment\r\nport = 8080 # inline\r\n\r\n[logging]\r\nlevel = 'info'";
    let mut document = TomlDocument::parse(source).unwrap();
    document.put_bool(&["server", "enabled"], true).unwrap();
    assert_eq!(
        document.render(),
        "z = 1\r\na.b=2\r\n[server]\r\n# preserve this comment\r\nport = 8080 # inline\r\nenabled = true\r\n\r\n[logging]\r\nlevel = 'info'"
    );
    assert!(document.has(&["server", "enabled"]));
}

#[test]
fn toml_insert_into_new_nested_table_preserves_existing_source() {
    let source = "version = 1 # owner note\nname = \"demo\"\n";
    let mut document = TomlDocument::parse(source).unwrap();
    document
        .put_integer(&["sync", "timeout_seconds"], 30)
        .unwrap();
    assert_eq!(
        document.get_integer(&["sync", "timeout_seconds"]).unwrap(),
        30
    );
    assert!(document.render().contains("version = 1 # owner note"));
    assert!(document.render().contains("[sync]"));
}

#[test]
fn calendar_parses_utc_event_and_rejects_floating_times() {
    let source = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:one\r\nDTSTART:20261024T090000Z\r\nDTEND:20261024T100000Z\r\nSUMMARY:Example\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let events = calendar_parse(source).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].uid, "one");
    assert_eq!(events[0].summary, "Example");
    let start = events[0].start_resolution.as_ref().unwrap();
    assert_eq!(start.kind, "exact");
    assert_eq!(start.timezone, "UTC");
    assert_eq!(start.local.hour, 9);
    assert_eq!(start.unix_microseconds, [1_792_832_400_000_000]);
    let end = events[0].end_resolution.as_ref().unwrap();
    assert_eq!(end.unix_microseconds, [1_792_836_000_000_000]);
    let floating = source.replace("20261024T090000Z", "20261024T090000");
    let floating_error = calendar_parse(&floating).unwrap_err();
    assert_eq!(floating_error.code, "E_ICAL_FLOATING");
    assert!(floating_error.message.contains("one"));

    let all_day = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:all-day\r\nDTSTART;VALUE=DATE:20261024\r\nDTEND;VALUE=DATE:20261025\r\nSUMMARY:Holiday\r\nSTATUS:CANCELLED\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let events = calendar_parse(all_day).unwrap();
    assert_eq!(events.len(), 1);
    assert!(events[0].all_day);
    assert_eq!(events[0].start_date, Some((2026, 10, 24)));
    assert_eq!(events[0].end_date, Some((2026, 10, 25)));
    assert_eq!(events[0].start_resolution, None);
    assert_eq!(events[0].end_resolution, None);
    assert_eq!(events[0].status, "CANCELLED");
}

#[test]
fn calendar_parses_an_upstream_libical_sample() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/libical-2.ics");
    let source = fs::read_to_string(path).unwrap();
    let events = calendar_parse(&source).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].uid, "uid3@host1.com");
    assert_eq!(
        events[0].summary,
        "Calendaring Interoperability Planning Meeting"
    );
    let start = events[0].start_resolution.as_ref().unwrap();
    let end = events[0].end_resolution.as_ref().unwrap();
    assert_eq!(start.kind, "exact");
    assert_eq!(start.timezone, "UTC");
    assert_eq!(
        end.unix_microseconds[0] - start.unix_microseconds[0],
        30_600_000_000
    );
}

#[test]
fn embedded_vtimezone_observance_rules_resolve_recurrent_transitions() {
    let source = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTIMEZONE\r\nTZID:Example/Eastern\r\nBEGIN:STANDARD\r\nDTSTART:20251102T020000\r\nTZOFFSETFROM:-0400\r\nTZOFFSETTO:-0500\r\nRRULE:FREQ=YEARLY;BYMONTH=11;BYDAY=1SU\r\nTZNAME:EST\r\nEND:STANDARD\r\nBEGIN:DAYLIGHT\r\nDTSTART:20250309T020000\r\nTZOFFSETFROM:-0500\r\nTZOFFSETTO:-0400\r\nRRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=2SU\r\nTZNAME:EDT\r\nEND:DAYLIGHT\r\nEND:VTIMEZONE\r\nBEGIN:VEVENT\r\nUID:summer\r\nDTSTART;TZID=Example/Eastern:20260701T090000\r\nDTEND;TZID=Example/Eastern:20260701T100000\r\nSUMMARY:Summer\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:winter\r\nDTSTART;TZID=Example/Eastern:20260101T090000\r\nDTEND;TZID=Example/Eastern:20260101T100000\r\nSUMMARY:Winter\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let events = calendar_parse(source).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(
        events[0].start_resolution.as_ref().unwrap().timezone,
        "Example/Eastern"
    );
    assert_eq!(
        events[0]
            .start_resolution
            .as_ref()
            .unwrap()
            .unix_microseconds,
        [1_782_910_800_000_000]
    );
    assert_eq!(
        events[1].start_resolution.as_ref().unwrap().timezone,
        "Example/Eastern"
    );
    assert_eq!(
        events[1]
            .start_resolution
            .as_ref()
            .unwrap()
            .unix_microseconds,
        [1_767_276_000_000_000]
    );
}

#[test]
fn calendar_handles_iana_offsets_folding_escapes_and_invalid_event_data() {
    let source = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:one\r\nDTSTART;TZID=America/Los_Angeles:20261024T090000\r\nDTEND;TZID=America/Los_Angeles:20261024T100000\r\nSUMMARY:Long\\, folded\\nname\r\nSTATUS:TENTATIVE\r\nRRULE:FREQ=WEEKLY;BYDAY=MO\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let events = calendar_parse(source).unwrap();
    let exact = events[0].start_resolution.as_ref().unwrap();
    assert_eq!(exact.kind, "exact");
    assert_eq!(exact.timezone, "America/Los_Angeles");
    assert_eq!(exact.local.hour, 9);
    assert_eq!(exact.unix_microseconds, [1_792_857_600_000_000]);
    assert_eq!(events[0].summary, "Long, folded\nname");
    assert_eq!(events[0].status, "TENTATIVE");
    assert_eq!(
        events[0].recurrence_rule.as_deref(),
        Some("FREQ=WEEKLY;BYDAY=MO")
    );

    let folded = source.replace("Long\\, folded\\nname", "Long\\,\r\n folded\\nname");
    assert_eq!(
        calendar_parse(&folded).unwrap()[0].summary,
        "Long,folded\nname"
    );
    let ambiguous = calendar_parse(
        &source
            .replace("20261024T090000", "20261101T013000")
            .replace("20261024T100000", "20261101T023000"),
    )
    .unwrap();
    let fold_start = ambiguous[0].start_resolution.as_ref().unwrap();
    assert_eq!(fold_start.kind, "fold");
    assert_eq!(fold_start.local.hour, 1);
    assert_eq!(
        fold_start.unix_microseconds,
        [1_793_521_800_000_000, 1_793_525_400_000_000]
    );
    let exact_end = ambiguous[0].end_resolution.as_ref().unwrap();
    assert_eq!(exact_end.kind, "exact");
    assert_eq!(exact_end.unix_microseconds, [1_793_529_000_000_000]);
    let nonexistent = calendar_parse(
        &source
            .replace("20261024T090000", "20260308T023000")
            .replace("20261024T100000", "20260308T033000"),
    )
    .unwrap();
    let gap_start = nonexistent[0].start_resolution.as_ref().unwrap();
    assert_eq!(gap_start.kind, "gap");
    assert!(gap_start.unix_microseconds.is_empty());
    assert_eq!(
        nonexistent[0].end_resolution.as_ref().unwrap().kind,
        "exact"
    );
    let invalid_date =
        calendar_parse(&source.replace("20261024T090000", "20261032T090000")).unwrap_err();
    assert_eq!(invalid_date.code, "E_ICAL_DATE");
    assert!(invalid_date.message.contains("one"));
    assert_eq!(
        calendar_parse(&source.replace("UID:one\r\n", ""))
            .unwrap_err()
            .code,
        "E_ICAL_UID"
    );
    let unknown_zone =
        calendar_parse(&source.replace("America/Los_Angeles", "Not/A_Real_Zone")).unwrap_err();
    assert_eq!(unknown_zone.code, "E_ICAL_TZID");
    assert!(unknown_zone.message.contains("one"));
    let reversed =
        calendar_parse(&source.replace("20261024T100000", "20261024T080000")).unwrap_err();
    assert_eq!(reversed.code, "E_ICAL_RANGE");
    assert!(reversed.message.contains("one"));
}

#[test]
fn embedded_vtimezone_takes_precedence_over_the_matching_iana_identifier() {
    let source = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTIMEZONE\r\nTZID:America/Los_Angeles\r\nBEGIN:STANDARD\r\nDTSTART:19700101T000000\r\nTZOFFSETFROM:-0500\r\nTZOFFSETTO:-0500\r\nTZNAME:Embedded\r\nEND:STANDARD\r\nEND:VTIMEZONE\r\nBEGIN:VEVENT\r\nUID:embedded\r\nDTSTART;TZID=America/Los_Angeles:20261024T090000\r\nDTEND;TZID=America/Los_Angeles:20261024T100000\r\nSUMMARY:Embedded zone wins\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let events = calendar_parse(source).unwrap();
    let start = events[0].start_resolution.as_ref().unwrap();
    let end = events[0].end_resolution.as_ref().unwrap();
    assert_eq!(start.kind, "exact");
    assert_eq!(start.timezone, "America/Los_Angeles");
    assert_eq!(start.unix_microseconds, [1_792_850_400_000_000]);
    assert_eq!(end.unix_microseconds, [1_792_854_000_000_000]);
}

#[test]
fn calendar_parse_exposes_each_local_resolution_and_preserves_event_identity() {
    let root = temp_path("calendar-resolution-api");
    fs::create_dir_all(&root).unwrap();
    let source = r#"import "std:calendar"
import "std:types"
import "std:time"
fn main() -> Int:
    let calendar = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:fold\r\nDTSTART;TZID=America/Los_Angeles:20261101T013000\r\nDTEND;TZID=America/Los_Angeles:20261101T023000\r\nSUMMARY:Fold event\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:gap\r\nDTSTART;TZID=America/Los_Angeles:20260308T023000\r\nDTEND;TZID=America/Los_Angeles:20260308T033000\r\nSUMMARY:Gap event\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
    let events = unwrap(calendar_parse(calendar))
    assert(events[0].uid == "fold" and events[0].summary == "Fold event")
    let fold = unwrap(events[0].start_resolution)
    assert(fold.kind == "fold")
    assert(fold.local.hour == 1 and fold.local.minute == 30)
    assert(fold.timezone == "America/Los_Angeles")
    assert(len(fold.instants) == 2)
    assert(format_utc(fold.instants[0]) == "2026-11-01T08:30:00Z")
    assert(format_utc(fold.instants[1]) == "2026-11-01T09:30:00Z")
    assert(events[1].uid == "gap" and events[1].summary == "Gap event")
    let gap = unwrap(events[1].start_resolution)
    assert(gap.kind == "gap" and len(gap.instants) == 0)
    assert(gap.local.hour == 2 and gap.local.minute == 30)
    assert(unwrap(events[1].end_resolution).kind == "exact")
    return 0
"#;
    let entry = root.join("main.gu");
    fs::write(&entry, source).unwrap();
    let stdlib = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../language/stdlib");
    let artifact = guion_compiler::compile_project_with_roots(
        &entry,
        &root,
        Some(&stdlib),
        &std::collections::HashMap::new(),
    )
    .expect("calendar resolution program compiles");
    let mut runtime = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .expect("calendar text parsing requires no file capability");
    assert_eq!(
        runtime.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::from_i64(0))
    );
    assert!(runtime.effect_report().is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn embedded_vtimezone_fold_is_reported_using_embedded_rules_for_matching_iana_id() {
    let root = temp_path("calendar-embedded-fold-api");
    fs::create_dir_all(&root).unwrap();
    let source = r#"import "std:calendar"
import "std:types"
import "std:time"
fn main() -> Int:
    let calendar = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VTIMEZONE\r\nTZID:America/Los_Angeles\r\nBEGIN:STANDARD\r\nDTSTART:20261101T020000\r\nTZOFFSETFROM:-0600\r\nTZOFFSETTO:-0700\r\nRRULE:FREQ=YEARLY;BYMONTH=11;BYDAY=1SU\r\nTZNAME:Embedded Standard\r\nEND:STANDARD\r\nBEGIN:DAYLIGHT\r\nDTSTART:20260308T020000\r\nTZOFFSETFROM:-0700\r\nTZOFFSETTO:-0600\r\nRRULE:FREQ=YEARLY;BYMONTH=3;BYDAY=2SU\r\nTZNAME:Embedded Daylight\r\nEND:DAYLIGHT\r\nEND:VTIMEZONE\r\nBEGIN:VEVENT\r\nUID:embedded-fold\r\nDTSTART;TZID=America/Los_Angeles:20261101T013000\r\nDTEND;TZID=America/Los_Angeles:20261101T023000\r\nSUMMARY:Embedded fold\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"
    let event = unwrap(calendar_parse(calendar))[0]
    let start = unwrap(event.start_resolution)
    assert(start.kind == "fold")
    assert(start.timezone == "America/Los_Angeles")
    assert(start.local.hour == 1 and start.local.minute == 30)
    assert(format_utc(start.instants[0]) == "2026-11-01T07:30:00Z")
    assert(format_utc(start.instants[1]) == "2026-11-01T08:30:00Z")
    return 0
"#;
    let entry = root.join("main.gu");
    fs::write(&entry, source).unwrap();
    let stdlib = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../language/stdlib");
    let artifact = guion_compiler::compile_project_with_roots(
        &entry,
        &root,
        Some(&stdlib),
        &std::collections::HashMap::new(),
    )
    .expect("embedded-zone fold program compiles");
    let mut runtime = Runtime::vm_only()
        .load(artifact, HostPolicy::default())
        .expect("parsing supplied calendar text requires no file capability");
    assert_eq!(
        runtime.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::from_i64(0))
    );
    assert!(runtime.effect_report().is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn guion_public_service_modules_call_typed_native_apis() {
    let project = temp_path("service-program");
    fs::create_dir_all(&project).unwrap();
    let source = r##"
import "std:text"
import "std:crypto"
import "std:sqlite"
import "std:toml"
import "std:calendar"
import "std:types"

pub fn main() -> Int:
    let database = unwrap(sqlite_open("services.sqlite"))
    let transaction = unwrap(sqlite_begin(database))
    unwrap(sqlite_execute(transaction, "CREATE TABLE entries (value TEXT NOT NULL)", []))
    unwrap(sqlite_execute(transaction, "INSERT INTO entries(value) VALUES (?1)", [sql_text("persisted")]))
    let rows = unwrap(sqlite_query(transaction, "SELECT value FROM entries", []))
    unwrap(sqlite_commit(transaction))
    unwrap(sqlite_close(database))
    let document = unwrap(toml_parse("# retained\nport = 8080\n"))
    unwrap(toml_put_int(document, ["port"], 9090))
    let calendar = unwrap(calendar_parse("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:event-1\r\nDTSTART:20261024T090000Z\r\nDTEND:20261024T100000Z\r\nSUMMARY:Example\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"))
    let key = bytes_from_ints([1, 2, 3])
    let body = bytes_from_text("exact bytes")
    let signature = crypto_hmac_sha256(key, body)
    if text_casefold("Straße") == "strasse" and rows[0].values[0].text == "persisted" and toml_render(document) == "# retained\nport = 9090\n" and calendar[0].uid == "event-1" and crypto_verify_hmac_sha256(key, body, signature):
        return 1
    return 0
"##;
    fs::write(project.join("main.gu"), source).unwrap();
    let stdlib = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../language/stdlib");
    let compiler_artifact =
        Artifact::from_bytes(SELF_HOSTED_COMPILER).expect("read checked self-host compiler");
    let compiler_args = vec![
        Value::Text("main.gu".into()),
        Value::Text(source.into()),
        Value::Text("std".into()),
        Value::Json(SharedJson::from_serde(serde_json::json!({}))),
    ];
    let compiler_policy = HostPolicy {
        instruction_limit: 500_000_000,
        allocation_limit_bytes: 8_589_934_592,
        call_depth_limit: 160,
        max_collection_items: 1_000_000,
        max_file_bytes: 64 * 1024 * 1024,
        ..HostPolicy::default()
    }
    .with_file_access(&project, true, false)
    .with_file_root_alias("std", &stdlib);
    let mut compiler_runtime = Runtime::new()
        .load(compiler_artifact, compiler_policy)
        .expect("load installed self-host compiler");
    let compiled = compiler_runtime
        .invoke("compiler_compile_project", compiler_args)
        .unwrap();
    let Value::ResultOk(compiled) = compiled else {
        panic!("self-host compiler rejected service program: {compiled:?}");
    };
    let Value::Bytes(compiled) = *compiled else {
        panic!("compiler returned non-bytecode output: {compiled:?}");
    };
    let artifact = Artifact::from_bytes(compiled.as_ref())
        .expect("native verifier accepts self-hosted artifact");
    let mut runtime = Runtime::new()
        .load(
            artifact,
            HostPolicy::default().with_file_access(&project, true, true),
        )
        .unwrap();
    assert_eq!(
        runtime.invoke("main", vec![]).unwrap(),
        Value::Int(BigInt::from_i64(1))
    );
    assert!(project.join("services.sqlite").exists());
    let _ = fs::remove_dir_all(project);
}
