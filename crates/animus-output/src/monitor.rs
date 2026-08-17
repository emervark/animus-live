//! Choosing which display the show goes to.
//!
//! The selection logic is a pure function over a described list of
//! monitors, because the bug it replaces was exactly the kind that pure
//! functions catch and systems hide. M0-4 shipped `monitors.iter().nth(1)`
//! — "the second monitor enumerated" — and enumeration order put the TV
//! first, so the output opened fullscreen *on the operator's own screen*,
//! borderless, cursor hidden, undraggable. The worst shape a bug can take
//! for a live tool: correct on the machine it was written on, wrong at the
//! venue, minutes before a show.

use bevy::prelude::*;

/// What is known about one attached display, in a form tests can build
/// without a window server.
#[derive(Debug, Clone, PartialEq)]
pub struct MonitorDesc {
    pub entity: Entity,
    pub name: String,
    pub physical_size: UVec2,
    pub refresh_hz: f32,
    pub scale: f64,
    pub is_primary: bool,
}

impl MonitorDesc {
    /// The one line that turns a venue problem into a five-second
    /// diagnosis. Logged at startup and shown in the output panel.
    pub fn describe(&self) -> String {
        format!(
            "{} {}x{}px @ {:.1}Hz scale={:.2}{}",
            self.name,
            self.physical_size.x,
            self.physical_size.y,
            self.refresh_hz,
            self.scale,
            if self.is_primary { " (primary)" } else { "" }
        )
    }
}

/// Which monitor the output window should open on.
///
/// The rule, in order:
/// 1. An explicit override wins. At a venue, the display that reports as
///    primary is not always the one the projector is on.
/// 2. Otherwise, the first monitor that is **not** primary — selected by
///    what it is, never by where enumeration happened to put it.
/// 3. One monitor only: `None`. The caller opens a windowed fallback; a
///    fullscreen output on the only screen would bury the editor.
pub fn choose_output_monitor(
    monitors: &[MonitorDesc],
    override_index: Option<usize>,
) -> Option<&MonitorDesc> {
    if let Some(index) = override_index {
        // An out-of-range override is a configuration mistake; falling back
        // to the automatic rule silently would hide it. Return the last
        // monitor so the operator sees *something* land somewhere and the
        // log names the clamp.
        return match monitors.get(index) {
            Some(m) => Some(m),
            None => {
                let last = monitors.last();
                if let Some(m) = last {
                    warn!(
                        "--output-monitor {index} is out of range ({} attached); using {}",
                        monitors.len(),
                        m.describe()
                    );
                }
                last
            }
        };
    }

    if monitors.len() < 2 {
        return None;
    }
    monitors.iter().find(|m| !m.is_primary)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(name: &str, primary: bool) -> MonitorDesc {
        MonitorDesc {
            entity: Entity::PLACEHOLDER,
            name: name.into(),
            physical_size: UVec2::new(1920, 1080),
            refresh_hz: 60.0,
            scale: 1.0,
            is_primary: primary,
        }
    }

    #[test]
    fn the_projector_is_chosen_by_not_being_primary_not_by_position() {
        // The M0-4 incident, as a fixture: the TV enumerates FIRST. nth(1)
        // would pick the laptop; the rule must pick the TV.
        let monitors = vec![monitor("TV", false), monitor("laptop", true)];
        let chosen = choose_output_monitor(&monitors, None).expect("two monitors");
        assert_eq!(chosen.name, "TV");

        // And in the friendly order too, which is what nth(1) was tested on.
        let monitors = vec![monitor("laptop", true), monitor("projector", false)];
        assert_eq!(
            choose_output_monitor(&monitors, None).unwrap().name,
            "projector"
        );
    }

    #[test]
    fn a_single_monitor_yields_none_rather_than_burying_the_editor() {
        let monitors = vec![monitor("laptop", true)];
        assert!(choose_output_monitor(&monitors, None).is_none());
        assert!(choose_output_monitor(&[], None).is_none());
    }

    #[test]
    fn an_explicit_override_beats_the_rule() {
        // The venue case: the projector reports as primary, so the automatic
        // rule would put the show on the operator's console.
        let monitors = vec![monitor("projector", true), monitor("console", false)];
        let chosen = choose_output_monitor(&monitors, Some(0)).unwrap();
        assert_eq!(chosen.name, "projector");
    }

    #[test]
    fn an_out_of_range_override_lands_on_the_last_monitor_not_nowhere() {
        let monitors = vec![monitor("laptop", true), monitor("projector", false)];
        let chosen = choose_output_monitor(&monitors, Some(7)).unwrap();
        assert_eq!(
            chosen.name, "projector",
            "a typo in --output-monitor should still put the show somewhere visible"
        );
    }
}
