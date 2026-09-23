//! A selection owner that advertises text, begins INCR, and never sends a chunk.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use x11rb::{
    connection::Connection,
    protocol::{xproto::*, Event},
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
    CURRENT_TIME, NONE,
};

pub struct StalledClipboard {
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<crate::support::TestResult>>,
    started: mpsc::Receiver<()>,
}

impl StalledClipboard {
    pub fn new() -> crate::support::SupportResult<Self> {
        let (conn, screen) = RustConnection::connect(None)?;
        let atom =
            |name: &[u8]| -> crate::support::SupportResult<Atom> { Ok(conn.intern_atom(false, name)?.reply()?.atom) };
        let clipboard = atom(b"CLIPBOARD")?;
        let targets = atom(b"TARGETS")?;
        let utf8 = atom(b"UTF8_STRING")?;
        let incr = atom(b"INCR")?;
        let window = conn.generate_id()?;
        conn.create_window(
            0,
            window,
            conn.setup().roots[screen].root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_ONLY,
            0,
            &CreateWindowAux::new(),
        )?
        .check()?;
        conn.set_selection_owner(window, clipboard, CURRENT_TIME)?.check()?;
        if conn.get_selection_owner(clipboard)?.reply()?.owner != window {
            return Err("stalled clipboard fixture did not acquire the selection".into());
        }
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let (sender, started) = mpsc::channel();
        let worker = thread::spawn(move || -> crate::support::TestResult {
            while !worker_stop.load(Ordering::Relaxed) {
                if let Some(Event::SelectionRequest(request)) = conn.poll_for_event()? {
                    let property = if request.property == NONE {
                        request.target
                    } else {
                        request.property
                    };
                    let supported = if request.target == targets {
                        conn.change_property32(
                            PropMode::REPLACE,
                            request.requestor,
                            property,
                            AtomEnum::ATOM,
                            &[utf8],
                        )?
                        .check()?;
                        true
                    } else if request.target == utf8 {
                        conn.change_property32(PropMode::REPLACE, request.requestor, property, incr, &[1024])?
                            .check()?;
                        true
                    } else {
                        false
                    };
                    conn.send_event(
                        false,
                        request.requestor,
                        EventMask::NO_EVENT,
                        SelectionNotifyEvent {
                            response_type: SELECTION_NOTIFY_EVENT,
                            sequence: 0,
                            time: request.time,
                            requestor: request.requestor,
                            selection: request.selection,
                            target: request.target,
                            property: if supported { property } else { NONE },
                        },
                    )?
                    .check()?;
                    conn.flush()?;
                    if request.target == utf8 && supported {
                        sender.send(())?;
                    }
                } else {
                    thread::sleep(Duration::from_millis(1));
                }
            }
            Ok(())
        });
        Ok(Self {
            stop,
            worker: Some(worker),
            started,
        })
    }

    pub fn wait_started(&self) -> crate::support::TestResult {
        self.started.recv_timeout(Duration::from_secs(3))?;
        Ok(())
    }

    pub fn finish(&mut self) -> crate::support::TestResult {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            worker.join().map_err(|_| "stalled clipboard fixture panicked")??;
        }
        Ok(())
    }
}

impl Drop for StalledClipboard {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}
