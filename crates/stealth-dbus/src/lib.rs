//! The D-Bus side of `stealthd`: names, conversions between [`Typed`] values and D-Bus values, and a blocking
//! client for the CLI and the GUI.
//!
//! Interface `io.github.swarm2linux.Headset1` at `/io/github/swarm2linux/Headset`, bus name
//! `io.github.swarm2linux.Stealthd`:
//!
//! - `Values() -> a{sv}`: every known value by name (see `tb_proto::values`), plus `connected` and `device`
//! - `Set(s name, v value)`: set a boolean or integer value, e.g. `("anc", true)`
//! - `SetBand(s eq, u band, i tenths)`: one band (1–10) of the `"game"` or `"mic"` equalizer, in tenths of a dB
//! - `SavePreset(s eq, s name)`, `DeletePreset(s eq, s name)`: custom EQ presets
//! - signal `Changed(a{sv})`: the values that changed
//! - property `Connected` (b)

use std::collections::{BTreeMap, HashMap};

use tb_proto::values::Typed;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedValue, Value};

pub const BUS_NAME: &str = "io.github.swarm2linux.Stealthd";
pub const PATH: &str = "/io/github/swarm2linux/Headset";
pub const INTERFACE: &str = "io.github.swarm2linux.Headset1";

/// The D-Bus form of a value.
pub fn to_value(v: &Typed) -> Value<'static> {
    match v {
        Typed::Bool(b) => Value::from(*b),
        Typed::Int(n) => Value::from(*n as i32),
        Typed::Text(s) => Value::from(s.clone()),
        Typed::Ints(list) => Value::from(list.iter().map(|&n| n as i32).collect::<Vec<i32>>()),
        Typed::Texts(list) => Value::from(list.clone()),
    }
}

/// The D-Bus form of a value, owned.
pub fn to_owned_value(v: &Typed) -> OwnedValue {
    OwnedValue::try_from(to_value(v)).expect("values carry no file descriptors")
}

/// A value received over D-Bus; `None` for shapes the protocol never uses.
pub fn from_value(v: &Value<'_>) -> Option<Typed> {
    Some(match v {
        Value::Bool(b) => Typed::Bool(*b),
        Value::U8(n) => Typed::Int(i64::from(*n)),
        Value::I16(n) => Typed::Int(i64::from(*n)),
        Value::U16(n) => Typed::Int(i64::from(*n)),
        Value::I32(n) => Typed::Int(i64::from(*n)),
        Value::U32(n) => Typed::Int(i64::from(*n)),
        Value::I64(n) => Typed::Int(*n),
        Value::Str(s) => Typed::Text(s.to_string()),
        Value::Value(inner) => return from_value(inner),
        Value::Array(a) => {
            let items: Vec<Typed> = a.iter().map(from_value).collect::<Option<_>>()?;
            if items.iter().all(|t| matches!(t, Typed::Text(_))) && !items.is_empty() {
                Typed::Texts(items.into_iter().map(|t| if let Typed::Text(s) = t { s } else { unreachable!() }).collect())
            } else {
                Typed::Ints(items.into_iter().map(|t| if let Typed::Int(n) = t { Some(n) } else { None }).collect::<Option<_>>()?)
            }
        }
        _ => return None,
    })
}

fn to_map(raw: HashMap<String, OwnedValue>) -> BTreeMap<String, Typed> {
    raw.into_iter().filter_map(|(k, v)| from_value(&v).map(|t| (k, t))).collect()
}

/// A blocking client for a running `stealthd`.
pub struct DaemonClient {
    proxy: Proxy<'static>,
}

impl DaemonClient {
    /// Connect to `stealthd` if it is running on the session bus.
    pub fn connect() -> zbus::Result<Option<Self>> {
        let conn = Connection::session()?;
        let dbus = zbus::blocking::fdo::DBusProxy::new(&conn)?;
        if !dbus.name_has_owner(BUS_NAME.try_into()?)? {
            return Ok(None);
        }
        let proxy = Proxy::new_owned(conn, BUS_NAME, PATH, INTERFACE)?;
        Ok(Some(Self { proxy }))
    }

    /// Every known value, plus `connected` and `device`.
    pub fn values(&self) -> zbus::Result<BTreeMap<String, Typed>> {
        let raw: HashMap<String, OwnedValue> = self.proxy.call("Values", &())?;
        Ok(to_map(raw))
    }

    pub fn set(&self, name: &str, value: &Typed) -> zbus::Result<()> {
        self.proxy.call::<_, _, ()>("Set", &(name, to_value(value)))
    }

    pub fn set_band(&self, eq: &str, band: u32, tenths: i32) -> zbus::Result<()> {
        self.proxy.call::<_, _, ()>("SetBand", &(eq, band, tenths))
    }

    pub fn save_preset(&self, eq: &str, name: &str) -> zbus::Result<()> {
        self.proxy.call::<_, _, ()>("SavePreset", &(eq, name))
    }

    pub fn delete_preset(&self, eq: &str, name: &str) -> zbus::Result<()> {
        self.proxy.call::<_, _, ()>("DeletePreset", &(eq, name))
    }

    /// The `Changed` signals, as they arrive (blocks between signals).
    pub fn changes(&self) -> zbus::Result<impl Iterator<Item = BTreeMap<String, Typed>> + '_> {
        let signals = self.proxy.receive_signal("Changed")?;
        Ok(signals.filter_map(|msg| {
            let (raw,): (HashMap<String, OwnedValue>,) = msg.body().deserialize().ok()?;
            Some(to_map(raw))
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_values() {
        for v in [
            Typed::Bool(true),
            Typed::Int(-85),
            Typed::Text("Bass Boost".into()),
            Typed::Ints(vec![50, 50, 35, 0, 0, 0, 0, 0, 0, 0]),
            Typed::Texts(vec!["TEST".into(), "".into()]),
        ] {
            assert_eq!(from_value(&to_value(&v)), Some(v));
        }
    }
}
