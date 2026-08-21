//! OSC in, on a thread of its own.
//!
//! A UDP socket and a blocking read, which is exactly why it is not on the
//! frame loop. The socket also carries a read timeout so the thread can
//! notice the app has gone and stop, rather than living on as the one thing
//! keeping the process alive.
//!
//! **The address is the channel.** OSC senders name what they are sending —
//! `/puppet/lean`, `/fader/3` — and that name is better than anything this
//! tool could invent, so it goes straight into the panel.

use std::net::UdpSocket;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use crate::{Source, Update};

/// The port Animus listens on unless told otherwise. 9000 is what most
/// phone and tablet controllers send to out of the box.
pub const DEFAULT_PORT: u16 = 9000;

/// A running OSC listener.
pub struct OscIn {
    stop: Arc<AtomicBool>,
    /// What we are listening on, if anything.
    pub bound: Option<String>,
    /// Why not, if not.
    pub trouble: Option<String>,
}

impl OscIn {
    pub fn is_live(&self) -> bool {
        self.bound.is_some()
    }
}

impl Drop for OscIn {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Listen on `port` and stream whatever arrives to `tx`.
///
/// Never fails: a port already in use is a normal thing to find, and the
/// editor has to open anyway. What went wrong is reported for the panel.
pub fn listen(port: u16, tx: Sender<Update>) -> OscIn {
    let stop = Arc::new(AtomicBool::new(false));
    let socket = match UdpSocket::bind(("0.0.0.0", port)) {
        Ok(s) => s,
        Err(e) => {
            return OscIn {
                stop,
                bound: None,
                trouble: Some(format!("port {port} is not available: {e}")),
            };
        }
    };
    // Without a timeout the thread would sit in `recv_from` for ever and
    // outlive the window it belongs to.
    if let Err(e) = socket.set_read_timeout(Some(Duration::from_millis(250))) {
        return OscIn {
            stop,
            bound: None,
            trouble: Some(e.to_string()),
        };
    }

    let flag = stop.clone();
    std::thread::Builder::new()
        .name("animus-osc".into())
        .spawn(move || {
            let mut buffer = [0_u8; 2048];
            while !flag.load(Ordering::Relaxed) {
                let Ok((len, _from)) = socket.recv_from(&mut buffer) else {
                    // A timeout lands here too, which is what makes the stop
                    // flag reachable.
                    continue;
                };
                for update in decode(&buffer[..len]) {
                    if tx.send(update).is_err() {
                        return;
                    }
                }
            }
        })
        .ok();

    OscIn {
        stop,
        bound: Some(format!("0.0.0.0:{port}")),
        trouble: None,
    }
}

/// Convenience: a channel plus the listener that feeds it.
pub fn open(port: u16) -> (Receiver<Update>, OscIn) {
    let (tx, rx) = channel();
    let osc = listen(port, tx);
    (rx, osc)
}

/// Every value a datagram carries, flattened.
///
/// Bundles are unwrapped because a sender is free to batch, and an operator
/// who sees nothing arriving should not have to know whether the thing on
/// the other end happens to bundle.
pub fn decode(bytes: &[u8]) -> Vec<Update> {
    let Ok((_, packet)) = rosc::decoder::decode_udp(bytes) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    flatten(packet, &mut out);
    out
}

fn flatten(packet: rosc::OscPacket, out: &mut Vec<Update>) {
    match packet {
        rosc::OscPacket::Message(message) => {
            if let Some(value) = first_number(&message.args) {
                out.push(Update {
                    source: Source::Osc {
                        address: message.addr,
                    },
                    value,
                });
            }
        }
        rosc::OscPacket::Bundle(bundle) => {
            for inner in bundle.content {
                flatten(inner, out);
            }
        }
    }
}

/// The first argument that can be read as a number, normalised to 0..1.
///
/// **Integers are divided by 127 and floats are not.** An OSC sender using
/// an int is nearly always relaying a MIDI-shaped value; one using a float
/// is nearly always already in 0..1, which is what the OSC convention says.
/// Guessing wrong in either direction gives a control that moves a hundred
/// times too far or not at all, and this guess is right far more often than
/// treating the two the same.
fn first_number(args: &[rosc::OscType]) -> Option<f32> {
    args.iter().find_map(|arg| match arg {
        rosc::OscType::Float(v) => Some(*v),
        rosc::OscType::Double(v) => Some(*v as f32),
        rosc::OscType::Int(v) => Some(*v as f32 / 127.0),
        rosc::OscType::Bool(v) => Some(if *v { 1.0 } else { 0.0 }),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rosc::{OscMessage, OscPacket, OscType};

    fn encode(addr: &str, args: Vec<OscType>) -> Vec<u8> {
        rosc::encoder::encode(&OscPacket::Message(OscMessage {
            addr: addr.into(),
            args,
        }))
        .expect("encodes")
    }

    /// **The address is the channel.** A sender names what it is sending,
    /// and that name is better than anything this tool could invent.
    #[test]
    fn a_float_message_becomes_one_update_named_after_its_address() {
        let updates = decode(&encode("/puppet/lean", vec![OscType::Float(0.61)]));
        assert_eq!(updates.len(), 1);
        assert_eq!(
            updates[0].source,
            Source::Osc {
                address: "/puppet/lean".into()
            }
        );
        assert!((updates[0].value - 0.61).abs() < 1e-6);
    }

    #[test]
    fn an_int_is_read_as_a_midi_shaped_value() {
        let updates = decode(&encode("/fader/3", vec![OscType::Int(127)]));
        assert_eq!(updates[0].value, 1.0);
    }

    #[test]
    fn a_message_with_nothing_numeric_produces_nothing() {
        let updates = decode(&encode("/hello", vec![OscType::String("world".into())]));
        assert!(updates.is_empty());
    }

    /// A sender is free to batch, and an operator seeing nothing arrive
    /// should not have to know whether the far end bundles.
    #[test]
    fn a_bundle_is_unwrapped_into_its_messages() {
        let bundle = OscPacket::Bundle(rosc::OscBundle {
            timetag: rosc::OscTime {
                seconds: 0,
                fractional: 0,
            },
            content: vec![
                OscPacket::Message(OscMessage {
                    addr: "/a".into(),
                    args: vec![OscType::Float(0.25)],
                }),
                OscPacket::Message(OscMessage {
                    addr: "/b".into(),
                    args: vec![OscType::Float(0.75)],
                }),
            ],
        });
        let bytes = rosc::encoder::encode(&bundle).expect("encodes");
        let updates = decode(&bytes);
        assert_eq!(updates.len(), 2);
    }

    #[test]
    fn rubbish_on_the_socket_is_ignored_rather_than_fatal() {
        assert!(decode(&[0xFF, 0x00, 0x13]).is_empty());
    }
}
