use std::io;
use std::process::Child;
use std::thread;
use std::time::{Duration, Instant};

use x11rb::connection::Connection as _;
use x11rb::protocol::damage::{self, ConnectionExt as _};
use x11rb::protocol::xproto::Window;
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::NONE;

use crate::Result;

pub(crate) fn wait_quiet(
    conn: &RustConnection,
    damage_id: damage::Damage,
    window: Window,
    child: &mut Child,
    quiet_for: Duration,
    timeout: Duration,
) -> Result<u64> {
    let deadline = Instant::now() + timeout;
    let mut last_damage = Instant::now();
    let mut damage_events = 0u64;

    loop {
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "editor exited while waiting for redraws to finish: {status}"
            ))
            .into());
        }

        while let Some(event) = conn.poll_for_event()? {
            if let Event::DamageNotify(notify) = event {
                if notify.damage == damage_id && notify.drawable == window {
                    last_damage = Instant::now();
                    damage_events += 1;
                    conn.damage_subtract(damage_id, NONE, NONE)?;
                }
            }
        }
        conn.flush()?;

        if last_damage.elapsed() >= quiet_for {
            return Ok(damage_events);
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other("timed out waiting for redraw quiet period").into());
        }
        thread::sleep(Duration::from_millis(5));
    }
}
