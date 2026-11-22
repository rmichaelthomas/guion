//! Output a native function already wrote before it fell back to the VM.
//!
//! The VM replays such a function from its start and prints the same lines again. What native code
//! wrote is remembered here and the replay is compared with it as it is produced: bytes that match
//! are dropped, and the first byte that does not match ends the skipping, so the rest of the replay
//! is written. Native code and the VM are meant to print the same bytes; this keeps a difference
//! (a float formatted two ways, a future native intrinsic) from swallowing or garbling unrelated
//! output.

use std::collections::VecDeque;

/// Written bytes kept for comparison per native run and stream. Output beyond this is skipped by
/// count alone, unverified.
pub(crate) const VERIFIED_BYTES_PER_RUN: usize = 1024 * 1024;

/// Appends the front of `text`, one flush of a stream's output, to `kept` (what is kept for
/// comparison), up to `cap` bytes in all. `before` is how many bytes the stream had written before
/// this flush. The kept text is always a prefix of the written output and ends on a character
/// boundary: once the cap or a boundary cuts a flush, nothing later is kept, and the rest of the
/// run is skipped by count (`ReplaySkip::expect`).
pub(crate) fn keep_written(kept: &mut Vec<u8>, before: usize, text: &str, cap: usize) {
    if kept.len() != before {
        return;
    }
    let mut take = text.len().min(cap.saturating_sub(kept.len()));
    while !text.is_char_boundary(take) {
        take -= 1;
    }
    kept.extend_from_slice(&text.as_bytes()[..take]);
}

#[derive(Debug)]
enum Segment {
    /// Bytes that were written; the replay must match them.
    Written(Vec<u8>),
    /// This many written bytes that were not kept (over the cap); skipped by count.
    Uncompared(usize),
}

/// The pending skips for one output stream, oldest first.
#[derive(Debug, Default)]
pub(crate) struct ReplaySkip {
    segments: VecDeque<Segment>,
    /// How much of the front segment has been matched already.
    offset: usize,
}

impl ReplaySkip {
    /// Records one native run's written output: its kept `text` and the `total` it wrote (which
    /// exceeds `text.len()` when the cap cut it).
    pub(crate) fn expect(&mut self, text: &[u8], total: usize) {
        if !text.is_empty() {
            self.segments.push_back(Segment::Written(text.to_vec()));
        }
        let rest = total.saturating_sub(text.len());
        if rest > 0 {
            self.segments.push_back(Segment::Uncompared(rest));
        }
    }

    pub(crate) fn clear(&mut self) {
        self.segments.clear();
        self.offset = 0;
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// The part of `text` that still has to be written. Drops the front of `text` while it matches
    /// what native code wrote. Returns `(rest, mismatch)`; on a mismatch every pending skip is
    /// forgotten and `rest` starts at the first byte that differed.
    pub(crate) fn filter<'a>(&mut self, text: &'a str) -> (&'a str, bool) {
        let mut rest = text;
        while !rest.is_empty() {
            let Some(front) = self.segments.front() else {
                break;
            };
            let (take, finished) = match front {
                Segment::Written(bytes) => {
                    let pending = &bytes[self.offset..];
                    let take = pending.len().min(rest.len());
                    let matched = pending
                        .iter()
                        .zip(rest.as_bytes())
                        .take_while(|(left, right)| left == right)
                        .count();
                    if matched < take {
                        // Drop the bytes that did match, write the rest from the difference.
                        let mut cut = matched;
                        while !rest.is_char_boundary(cut) {
                            cut -= 1;
                        }
                        self.clear();
                        return (&rest[cut..], true);
                    }
                    (take, take == pending.len())
                }
                Segment::Uncompared(count) => {
                    let take = (*count).min(rest.len());
                    (take, take == *count)
                }
            };
            if !rest.is_char_boundary(take) {
                self.clear();
                return (rest, true);
            }
            rest = &rest[take..];
            if finished {
                self.segments.pop_front();
                self.offset = 0;
            } else {
                match self.segments.front_mut() {
                    Some(Segment::Uncompared(count)) => *count -= take,
                    _ => self.offset += take,
                }
            }
        }
        (rest, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_matching_replay_is_dropped_across_chunks() {
        let mut skip = ReplaySkip::default();
        skip.expect(b"one\ntwo\n", 8);
        assert_eq!(skip.filter("one\n"), ("", false));
        assert_eq!(skip.filter("two\nthree\n"), ("three\n", false));
        assert!(skip.is_empty());
        assert_eq!(skip.filter("four\n"), ("four\n", false));
    }

    #[test]
    fn a_difference_ends_the_skip_and_the_rest_is_written() {
        let mut skip = ReplaySkip::default();
        skip.expect(b"value 1.5\nafter\n", 16);
        // The replay formats the number differently: "value 1.5" matches, then it diverges.
        assert_eq!(skip.filter("value 1.50\n"), ("0\n", true));
        assert!(skip.is_empty());
        // Nothing later is swallowed.
        assert_eq!(skip.filter("after\n"), ("after\n", false));
    }

    #[test]
    fn bytes_beyond_the_kept_text_are_skipped_by_count() {
        let mut skip = ReplaySkip::default();
        skip.expect(b"ab", 5);
        assert_eq!(skip.filter("abXYZtail"), ("tail", false));
        assert!(skip.is_empty());
    }

    #[test]
    fn several_runs_queue_in_order() {
        let mut skip = ReplaySkip::default();
        skip.expect(b"a\n", 2);
        skip.expect(b"b\n", 2);
        assert_eq!(skip.filter("a\nb\nc\n"), ("c\n", false));
    }

    #[test]
    fn clear_forgets_everything() {
        let mut skip = ReplaySkip::default();
        skip.expect(b"a\n", 2);
        skip.clear();
        assert_eq!(skip.filter("a\n"), ("a\n", false));
    }

    #[test]
    fn a_difference_inside_a_multibyte_character_cuts_at_a_boundary() {
        let mut skip = ReplaySkip::default();
        skip.expect("é".as_bytes(), 2);
        // Same first byte, different second byte: the cut lands before the whole character.
        let (rest, mismatch) = skip.filter("\u{e8}");
        assert!(mismatch);
        assert_eq!(rest, "\u{e8}");
    }

    #[test]
    fn a_cap_inside_a_multibyte_character_does_not_print_the_replay_twice() {
        // 4 bytes cap; "ab" + "é" (2 bytes) + "cd\n" is written in two flushes, the cap falls inside é.
        let flushes = ["ab\u{e9}", "cd\n"];
        let mut kept = Vec::new();
        let mut before = 0;
        for flush in flushes {
            keep_written(&mut kept, before, flush, 3);
            before += flush.len();
        }
        assert_eq!(kept, b"ab");
        let mut skip = ReplaySkip::default();
        skip.expect(&kept, before);
        // The VM replays the same output: nothing is written a second time, and no mismatch.
        assert_eq!(skip.filter("ab\u{e9}"), ("", false));
        assert_eq!(skip.filter("cd\n"), ("", false));
        assert!(skip.is_empty());
        // Output after the replay is written.
        assert_eq!(skip.filter("next\n"), ("next\n", false));
    }

    #[test]
    fn nothing_is_kept_after_a_cut_even_when_room_is_left() {
        let mut kept = Vec::new();
        keep_written(&mut kept, 0, "a\u{e9}", 2);
        assert_eq!(kept, b"a");
        // One byte of room remains, but the next flush follows bytes that were not kept.
        keep_written(&mut kept, 3, "bc", 2);
        assert_eq!(kept, b"a");
    }
}
