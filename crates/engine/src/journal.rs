//! The trading journal.
//!
//! Notes live in the same append-only event log as everything else (ADR 0011),
//! so an edit is a later event with the same id rather than a mutation. The
//! entry a reader sees is the last write for that id.

use crate::session::{Event, LoggedEvent};
use replay_core::Timestamp;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalEntry {
    pub id: u64,
    /// The cursor the note was written at, so it sits where it belongs in the
    /// replay rather than at wall-clock time (ADR 0010).
    pub cursor: Timestamp,
    pub text: String,
    pub tags: Vec<String>,
    /// The ledger row this note is about, if any.
    pub trade: Option<usize>,
}

/// Fold the log into the current set of notes, oldest first.
///
/// Events after `cursor` are ignored so the journal matches what the replay is
/// showing; stepping back hides notes written later, exactly as it hides trades.
pub fn entries(events: &[LoggedEvent], cursor: Timestamp) -> Vec<JournalEntry> {
    let mut out: Vec<JournalEntry> = Vec::new();
    for logged in events.iter().filter(|e| e.cursor <= cursor) {
        let Event::NoteSet {
            note,
            text,
            tags,
            trade,
        } = &logged.event
        else {
            continue;
        };
        let entry = JournalEntry {
            id: *note,
            cursor: logged.cursor,
            text: text.clone(),
            tags: tags.clone(),
            trade: *trade,
        };
        match out.iter_mut().find(|e| e.id == *note) {
            // A later write for the same id is an edit, not a second note.
            Some(existing) => *existing = entry,
            None => out.push(entry),
        }
    }
    out
}

/// Every tag in use, deduplicated, for filtering in the UI.
pub fn tags(entries: &[JournalEntry]) -> Vec<String> {
    let mut all: Vec<String> = entries.iter().flat_map(|e| e.tags.clone()).collect();
    all.sort();
    all.dedup();
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(seq: u64, at: i64, id: u64, text: &str, tags: &[&str]) -> LoggedEvent {
        LoggedEvent {
            seq,
            cursor: Timestamp(at),
            event: Event::NoteSet {
                note: id,
                text: text.into(),
                tags: tags.iter().map(|s| s.to_string()).collect(),
                trade: None,
            },
        }
    }

    #[test]
    fn the_last_write_for_an_id_is_the_note() {
        let log = vec![
            note(1, 100, 1, "first thought", &["fomo"]),
            note(2, 200, 1, "on reflection", &["fomo", "patience"]),
        ];
        let out = entries(&log, Timestamp(999));
        assert_eq!(out.len(), 1, "an edit is not a second note");
        assert_eq!(out[0].text, "on reflection");
        assert_eq!(out[0].tags, vec!["fomo", "patience"]);
    }

    #[test]
    fn notes_written_after_the_cursor_are_not_visible() {
        let log = vec![note(1, 100, 1, "early", &[]), note(2, 500, 2, "later", &[])];
        let out = entries(&log, Timestamp(200));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "early");
    }

    #[test]
    fn tags_are_deduplicated_and_sorted() {
        let log = vec![
            note(1, 100, 1, "a", &["b", "a"]),
            note(2, 200, 2, "b", &["a", "c"]),
        ];
        assert_eq!(tags(&entries(&log, Timestamp(999))), vec!["a", "b", "c"]);
    }

    #[test]
    fn a_log_with_no_notes_yields_an_empty_journal() {
        let log = vec![LoggedEvent {
            seq: 1,
            cursor: Timestamp(0),
            event: Event::CursorSet { to: Timestamp(0) },
        }];
        assert!(entries(&log, Timestamp(999)).is_empty());
    }
}
