//! One app at a time: the first owns `io.github.swarm2linux.Gui` on the session bus; a later start asks it to show
//! its window and exits.

use slint::{ComponentHandle, Weak};
use zbus::blocking::Connection;
use zbus::fdo::{RequestNameFlags, RequestNameReply};
use zbus::interface;

use crate::AppWindow;

const NAME: &str = "io.github.swarm2linux.Gui";
const PATH: &str = "/io/github/swarm2linux/Gui";
const INTERFACE: &str = "io.github.swarm2linux.Gui1";

struct Gui {
    window: Weak<AppWindow>,
}

#[interface(name = "io.github.swarm2linux.Gui1")]
impl Gui {
    /// Show the window.
    fn activate(&self) {
        let _ = self.window.upgrade_in_event_loop(|w| {
            let _ = w.show();
        });
    }
}

/// Become the running app and keep the returned connection, or, if another one runs, have it show its window and
/// get `None`.
pub fn claim(window: Weak<AppWindow>) -> zbus::Result<Option<Connection>> {
    let conn = Connection::session()?;
    conn.object_server().at(PATH, Gui { window })?;
    match conn.request_name_with_flags(NAME, RequestNameFlags::DoNotQueue.into()) {
        Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) => Ok(Some(conn)),
        Ok(_) | Err(zbus::Error::NameTaken) => {
            conn.call_method(Some(NAME), PATH, Some(INTERFACE), "Activate", &())?;
            Ok(None)
        }
        Err(e) => Err(e),
    }
}
