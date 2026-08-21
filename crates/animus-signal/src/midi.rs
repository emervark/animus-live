//! MIDI in, on a thread of its own.
//!
//! Every input port the machine offers is opened at once. **Asking the
//! operator which port their controller is on is asking a question they
//! should not have to answer**: they plugged one thing in, and the tool can
//! listen to all of them for the cost of a handful of callbacks.
//!
//! Only control changes are read. Notes, clock and SysEx are a different
//! kind of thing — discrete events rather than a value that slides — and
//! wiring them into a continuous channel would mean inventing a meaning for
//! "how far through a note-on are we".

use std::sync::mpsc::{Sender, channel};

use crate::{Source, Update};

/// A live connection to every MIDI input port that would open.
///
/// **Held, not dropped.** `midir` closes a port the moment its connection
/// value goes out of scope, and a source that silently stops after the
/// function that created it returns is the kind of bug that only shows up
/// during a performance.
pub struct MidiIn {
    _connections: Vec<midir::MidiInputConnection<Sender<Update>>>,
    /// The ports that opened, for the panel to show.
    pub ports: Vec<String>,
    /// Why the rest did not, if any.
    pub trouble: Option<String>,
}

impl MidiIn {
    pub fn is_live(&self) -> bool {
        !self._connections.is_empty()
    }
}

/// Open every MIDI input port and stream control changes to `tx`.
///
/// Never fails: a machine with no MIDI at all is a normal machine, and this
/// tool has to start on it. What it could not do is reported for the panel
/// instead of thrown.
pub fn listen(tx: Sender<Update>) -> MidiIn {
    let mut connections = Vec::new();
    let mut ports = Vec::new();
    let mut trouble = None;

    // One `MidiInput` per port: `connect` consumes it.
    let probe = match midir::MidiInput::new("Animus Live") {
        Ok(p) => p,
        Err(e) => {
            return MidiIn {
                _connections: connections,
                ports,
                trouble: Some(format!("no MIDI on this machine: {e}")),
            };
        }
    };
    // `probe` goes out of scope here on purpose: `connect` consumes a
    // `MidiInput`, so each port below needs one of its own.
    let count = probe.ports().len();

    for index in 0..count {
        let input = match midir::MidiInput::new("Animus Live") {
            Ok(i) => i,
            Err(e) => {
                trouble = Some(e.to_string());
                break;
            }
        };
        let Some(port) = input.ports().get(index).cloned() else {
            continue;
        };
        let name = input
            .port_name(&port)
            .unwrap_or_else(|_| format!("port {index}"));

        match input.connect(
            &port,
            "animus-in",
            |_stamp, message, tx: &mut Sender<Update>| {
                if let Some(update) = control_change(message) {
                    // A closed receiver means the app is shutting down; there
                    // is nothing useful to do about it here.
                    let _ = tx.send(update);
                }
            },
            tx.clone(),
        ) {
            Ok(connection) => {
                connections.push(connection);
                ports.push(name);
            }
            Err(e) => trouble = Some(format!("{name}: {e}")),
        }
    }

    MidiIn {
        _connections: connections,
        ports,
        trouble,
    }
}

/// Convenience: a channel plus the listener that feeds it.
pub fn open() -> (std::sync::mpsc::Receiver<Update>, MidiIn) {
    let (tx, rx) = channel();
    let midi = listen(tx);
    (rx, midi)
}

/// A control-change message, normalised. Anything else is `None`.
///
/// Seven bits divided by 127, not 128: a controller at its top stop must
/// read exactly 1.0, or a binding can never quite reach the end of its own
/// range and the operator is left pushing against a limit that is not there.
pub fn control_change(message: &[u8]) -> Option<Update> {
    let [status, controller, value, ..] = message else {
        return None;
    };
    if status & 0xF0 != 0xB0 {
        return None;
    }
    Some(Update {
        source: Source::MidiCc {
            channel: status & 0x0F,
            cc: *controller,
        },
        value: *value as f32 / 127.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_control_change_normalises_to_zero_and_one() {
        let low = control_change(&[0xB0, 12, 0]).unwrap();
        assert_eq!(low.value, 0.0);
        let high = control_change(&[0xB0, 12, 127]).unwrap();
        assert_eq!(
            high.value, 1.0,
            "the top stop must reach exactly 1, or a binding can never \
             reach the end of its own range"
        );
    }

    #[test]
    fn the_midi_channel_is_part_of_the_source() {
        let a = control_change(&[0xB0, 12, 64]).unwrap();
        let b = control_change(&[0xB3, 12, 64]).unwrap();
        assert_ne!(
            a.source, b.source,
            "cc 12 on two MIDI channels is two controls"
        );
    }

    /// Notes, clock and SysEx are discrete events, not values that slide.
    /// Reading them as a channel would mean inventing a meaning for "how far
    /// through a note-on are we".
    #[test]
    fn anything_that_is_not_a_control_change_is_ignored() {
        assert!(control_change(&[0x90, 60, 100]).is_none(), "note on");
        assert!(control_change(&[0xF8]).is_none(), "clock");
        assert!(control_change(&[0xB0]).is_none(), "truncated");
    }
}
