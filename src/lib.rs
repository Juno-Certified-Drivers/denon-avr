//! Denon and Marantz AV receivers over the telnet control port (23).
//!
//! Line-oriented ASCII, and the device echoes every command back as its status report — so
//! the same parser handles both our writes and someone turning the front knob.
//!
//! ```text
//!   PWON / PWSTANDBY      power
//!   MV45  MVUP  MVDOWN    master volume, 00-98 in half-steps (MV455 = 45.5)
//!   MUON / MUOFF          mute
//!   SIDVD SIBD SIMPLAY…   input select, by SOURCE NAME not jack number
//!   MSMOVIE MSMUSIC…      surround mode
//! ```
//!
//! # Inputs are named, not numbered
//!
//! Denon selects sources by name (`SIBD`), while Juno's pathfinder works in connection ids —
//! so the manifest gives each jack an id and the driver maps id → source name. Getting this
//! wrong is the classic Denon integration bug: `SI1` does nothing at all.

use juno_driver_sdk::*;
use serde_json::Value;

#[derive(Default)]
pub struct DenonReceiver;

const NET: LocalId = 0;
const ZONE: LocalId = 1;

/// Connection id → Denon source name. The ids match the manifest's `[[connection]]` blocks.
fn source_name(connection: u64) -> Option<&'static str> {
    Some(match connection {
        1 => "BD",
        2 => "DVD",
        3 => "MPLAY",
        4 => "GAME",
        5 => "SAT/CBL",
        6 => "TV",
        7 => "AUX1",
        8 => "CD",
        _ => return None,
    })
}

fn connection_for(name: &str) -> Option<u64> {
    (1..=8).find(|c| source_name(*c) == Some(name))
}

/// Denon volume is 00–98 where 80 is reference level, not a percentage. Presenting it as a
/// percentage and sending it raw is how a demo becomes painfully loud.
fn percent_to_mv(percent: u64) -> String {
    let mv = (percent.min(100) * 98) / 100;
    format!("{mv:02}")
}

fn mv_to_percent(raw: &str) -> Option<u64> {
    // "45" is 45; "455" is 45.5 — three digits carry a half-step.
    let digits: String = raw.chars().filter(char::is_ascii_digit).collect();
    let value: f64 = match digits.len() {
        2 => digits.parse::<f64>().ok()?,
        3 => digits[..2].parse::<f64>().ok()? + 0.5,
        _ => return None,
    };
    Some(((value * 100.0) / 98.0).round() as u64)
}

impl DenonReceiver {
    fn send(cmd: &str) -> HostCall {
        HostCall::Tx {
            control: NET,
            data: format!("{cmd}\r").into_bytes(),
        }
    }
}

impl DriverModule for DenonReceiver {
    fn on_command(
        &self,
        _inst: &mut Instance,
        _proxy: LocalId,
        cmd: &str,
        args: &Args,
    ) -> Vec<HostCall> {
        match cmd {
            "on" => vec![Self::send("PWON")],
            "off" => vec![Self::send("PWSTANDBY")],
            "power_toggle" => vec![Self::send("PW?")],

            "set_input" => {
                let Some(c) = args.get("input").and_then(Value::as_u64) else {
                    return vec![HostCall::warn("denon: set_input needs an input")];
                };
                let Some(name) = source_name(c) else {
                    return vec![HostCall::warn(format!("denon: no source for connection {c}"))];
                };
                vec![Self::send(&format!("SI{name}"))]
            }

            "set_volume" => {
                let Some(p) = args.get("level").and_then(Value::as_u64) else {
                    return vec![HostCall::warn("denon: set_volume needs a level")];
                };
                vec![Self::send(&format!("MV{}", percent_to_mv(p)))]
            }
            "volume_up" => vec![Self::send("MVUP")],
            "volume_down" => vec![Self::send("MVDOWN")],

            "set_mute" => {
                let on = args.get("mute").and_then(Value::as_bool).unwrap_or(true);
                vec![Self::send(if on { "MUON" } else { "MUOFF" })]
            }
            "mute_toggle" => vec![Self::send("MU?")],

            "set_surround_mode" => {
                let Some(mode) = args.get("mode").and_then(Value::as_str) else {
                    return vec![HostCall::warn("denon: set_surround_mode needs a mode")];
                };
                vec![Self::send(&format!("MS{}", mode.to_uppercase()))]
            }

            other => vec![HostCall::warn(format!("denon: unhandled `{other}`"))],
        }
    }

    /// The receiver reports its own state, whether we caused the change or the front panel
    /// did. Without this the UI goes stale the moment anyone touches the knob.
    fn on_event(
        &self,
        _inst: &mut Instance,
        _control: LocalId,
        note: &str,
        args: &Args,
    ) -> Vec<HostCall> {
        if note != "rx" {
            return Vec::new();
        }
        let Some(text) = args.get("data").and_then(Value::as_str) else {
            return Vec::new();
        };

        let mut out = Vec::new();
        for line in text.split(['\r', '\n']).filter(|l| !l.is_empty()) {
            let line = line.trim();

            if let Some(rest) = line.strip_prefix("PW") {
                let mut a = Args::new();
                a.insert("on".into(), json!(rest == "ON"));
                out.push(HostCall::notify(ZONE, "power_changed", a));
            } else if let Some(rest) = line.strip_prefix("MV") {
                // MVMAX is the ceiling report, not the current volume.
                if rest.starts_with("MAX") {
                    continue;
                }
                if let Some(p) = mv_to_percent(rest) {
                    let mut a = Args::new();
                    a.insert("level".into(), json!(p));
                    out.push(HostCall::notify(ZONE, "volume_changed", a));
                }
            } else if let Some(rest) = line.strip_prefix("MU") {
                let mut a = Args::new();
                a.insert("mute".into(), json!(rest == "ON"));
                out.push(HostCall::notify(ZONE, "mute_changed", a));
            } else if let Some(rest) = line.strip_prefix("SI") {
                if let Some(c) = connection_for(rest) {
                    let mut a = Args::new();
                    a.insert("input".into(), json!(c));
                    out.push(HostCall::notify(ZONE, "input_changed", a));
                }
            } else if let Some(rest) = line.strip_prefix("MS") {
                let mut a = Args::new();
                a.insert("mode".into(), json!(rest.to_lowercase()));
                out.push(HostCall::notify(ZONE, "surround_mode_changed", a));
            }
        }
        out
    }

    fn on_bind(&self, _inst: &mut Instance) -> Vec<HostCall> {
        let mut a = Args::new();
        a.insert("online".into(), json!(true));
        // Ask where it stands rather than assuming. Each `?` is answered by a status line
        // that on_event already knows how to read.
        vec![
            HostCall::notify(ZONE, "online_changed", a),
            Self::send("PW?"),
            Self::send("MV?"),
            Self::send("SI?"),
        ]
    }
}


export_driver!(DenonReceiver);
