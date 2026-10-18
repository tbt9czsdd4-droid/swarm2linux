//! One app at a time: the first owns `io.github.swarm2linux.Gui` on the session bus; a later start asks it to show
//! its window (making it if needed) and exits.

use slint::Weak;
use zbus::blocking::Connection;
use zbus::fdo::{RequestNameFlags, RequestNameReply};
use zbus::interface;

use crate::Tray;

const NAME: &str = "io.github.swarm2linux.Gui";
const PATH: &str = "/io/github/swarm2linux/Gui";
const INTERFACE: &str = "io.github.swarm2linux.Gui1";

struct Gui {
    tray: Weak<Tray>,
}

#[interface(name = "io.github.swarm2linux.Gui1")]
impl Gui {
    /// Show the window.
    fn activate(&self) {
        // the tray's Open makes the window if needed
        let _ = self.tray.upgrade_in_event_loop(|t| t.invoke_open());
    }
}

/// Become the running app and keep the returned connection, or, if another one runs, have it show its window and
/// get `None`.
pub fn claim(tray: Weak<Tray>) -> zbus::Result<Option<Connection>> {
    let conn = Connection::session()?;
    conn.object_server().at(PATH, Gui { tray })?;
    match conn.request_name_with_flags(NAME, RequestNameFlags::DoNotQueue.into()) {
        Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) => Ok(Some(conn)),
        Ok(_) | Err(zbus::Error::NameTaken) => {
            conn.call_method(Some(NAME), PATH, Some(INTERFACE), "Activate", &())?;
            Ok(None)
        }
        Err(e) => Err(e),
    }
}
