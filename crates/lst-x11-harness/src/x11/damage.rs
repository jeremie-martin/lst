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
    wait_impl(
        conn,
        damage_id,
        window,
        child,
        quiet_for,
        timeout,
        RequireDamage::No,
    )
}

pub(crate) fn wait_for_damage_then_quiet(
    conn: &RustConnection,
    damage_id: damage::Damage,
    window: Window,
    child: &mut Child,
    quiet_for: Duration,
    timeout: Duration,
) -> Result<u64> {
    wait_impl(
        conn,
        damage_id,
        window,
        child,
        quiet_for,
        timeout,
        RequireDamage::Yes,
    )
}

fn wait_impl(
    conn: &RustConnection,
    damage_id: damage::Damage,
    window: Window,
    child: &mut Child,
    quiet_for: Duration,
    timeout: Duration,
    require_damage: RequireDamage,
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

        if require_damage.satisfied_by(damage_events) && last_damage.elapsed() >= quiet_for {
            return Ok(damage_events);
        }
        if Instant::now() >= deadline {
            let detail = if damage_events == 0 {
                " without observing any matching damage events"
            } else {
                ""
            };
            return Err(io::Error::other(format!(
                "timed out waiting for redraw quiet period{detail}"
            ))
            .into());
        }
        thread::sleep(Duration::from_millis(5));
    }
}

#[derive(Clone, Copy)]
enum RequireDamage {
    No,
    Yes,
}

impl RequireDamage {
    fn satisfied_by(self, damage_events: u64) -> bool {
        match self {
            Self::No => true,
            Self::Yes => damage_events > 0,
        }
    }
}
