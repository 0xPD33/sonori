//! Sonori serves the clipboard itself while it pastes: an app reads it only when
//! it takes the paste, so no read means no paste. Afterwards, Sonori offers the
//! clipboard content from before the paste again.

use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};
use wayland_client::backend::ObjectId;
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_registry::WlRegistry, wl_seat::WlSeat};
use wayland_client::{event_created_child, Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use wayland_protocols_wlr::data_control::v1::client::{
    zwlr_data_control_device_v1::{self, ZwlrDataControlDeviceV1},
    zwlr_data_control_manager_v1::{self, ZwlrDataControlManagerV1},
    zwlr_data_control_offer_v1::{self, ZwlrDataControlOfferV1},
    zwlr_data_control_source_v1::{self, ZwlrDataControlSourceV1},
};

/// The types wl-copy offers for text.
const TEXT_TYPES: [&str; 5] = [
    "text/plain;charset=utf-8",
    "text/plain",
    "UTF8_STRING",
    "STRING",
    "TEXT",
];
/// KDE's password hint. Klipper, and cliphist through `wl-paste --watch`,
/// keep no history entry for a selection that offers it.
const SECRET_HINT: &str = "x-kde-passwordManagerHint";
/// Limits for saving the old clipboard. Past them, Sonori keeps the transcript
/// in the clipboard, as without a restore.
const SAVE_TIME: Duration = Duration::from_millis(500);
const SAVE_BYTES: usize = 20 << 20;
/// Some apps read a paste in more than one type; the restore waits for them.
const RESTORE_GRACE: Duration = Duration::from_millis(200);

/// Clipboard content: each offered type with its bytes.
type Clip = Vec<(String, Arc<[u8]>)>;

/// The clipboard content before the paste.
enum Saved {
    Empty,
    Content(Clip),
    /// Too large, too slow, or unreadable: Sonori does not restore.
    Unknown,
}

#[derive(Default)]
struct Shared {
    reads: Mutex<Reads>,
    changed: Condvar,
}

#[derive(Default)]
struct Reads {
    /// When an app read the transcript as text.
    times: Vec<Instant>,
    /// Another client took the selection from the transcript.
    replaced: bool,
    /// The source that holds the selection; the thread ends without one.
    current: Option<ObjectId>,
}

/// What one source serves, and whether its reads count as pastes.
struct Served {
    clip: Clip,
    transcript: bool,
}

struct State {
    shared: Arc<Shared>,
    /// Types the compositor announced for each offer.
    offers: HashMap<ObjectId, Vec<String>>,
    selection: Option<ZwlrDataControlOfferV1>,
}

/// The transcript in the clipboard, served by Sonori until another client
/// takes the selection or `restore` puts the old content back.
pub struct Offer {
    shared: Arc<Shared>,
    saved: Saved,
    conn: Connection,
    manager: ZwlrDataControlManagerV1,
    device: ZwlrDataControlDeviceV1,
    qh: QueueHandle<State>,
}

impl Offer {
    /// Saves the clipboard and makes `text` the selection. Fails when the
    /// compositor has no data-control protocol (GNOME).
    pub fn set(text: &str) -> Result<Self, String> {
        let conn = Connection::connect_to_env().map_err(|e| e.to_string())?;
        let (globals, mut queue): (_, EventQueue<State>) =
            registry_queue_init(&conn).map_err(|e| e.to_string())?;
        let qh = queue.handle();
        let manager: ZwlrDataControlManagerV1 = globals
            .bind(&qh, 1..=2, ())
            .map_err(|_| "the compositor has no wlr data-control protocol".to_string())?;
        let seat: WlSeat = globals
            .bind(&qh, 1..=1, ())
            .map_err(|_| "the compositor has no seat".to_string())?;
        let device = manager.get_data_device(&seat, &qh, ());
        let shared = Arc::new(Shared::default());
        let mut state = State {
            shared: shared.clone(),
            offers: HashMap::new(),
            selection: None,
        };
        // The device reports the current selection right away.
        queue.roundtrip(&mut state).map_err(|e| e.to_string())?;
        let saved = save(&conn, &mut queue, &mut state);

        let text: Arc<[u8]> = Arc::from(text.as_bytes());
        let mut clip: Clip = TEXT_TYPES
            .iter()
            .map(|mime| (mime.to_string(), text.clone()))
            .collect();
        clip.push((SECRET_HINT.to_string(), Arc::from(&b"secret"[..])));
        let source = new_source(&manager, &qh, clip, true);
        shared.reads.lock().current = Some(source.id());
        device.set_selection(Some(&source));
        queue.roundtrip(&mut state).map_err(|e| e.to_string())?;

        std::thread::Builder::new()
            .name("paste-check".into())
            .spawn(move || {
                // Serves reads while one of Sonori's sources holds the selection.
                while state.shared.reads.lock().current.is_some() {
                    if queue.blocking_dispatch(&mut state).is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            shared,
            saved,
            conn,
            manager,
            device,
            qh,
        })
    }

    /// Waits up to `wait` for an app to read the text after `since`. `None`
    /// means Sonori cannot tell, because another client took the selection.
    pub fn read_since(&self, since: Instant, wait: Duration) -> Option<bool> {
        let deadline = Instant::now() + wait;
        let mut reads = self.shared.reads.lock();
        loop {
            if reads.times.iter().any(|&t| t >= since) {
                return Some(true);
            }
            if reads.replaced {
                return None;
            }
            if self
                .shared
                .changed
                .wait_until(&mut reads, deadline)
                .timed_out()
            {
                return Some(reads.times.iter().any(|&t| t >= since));
            }
        }
    }

    /// Puts the clipboard content from before the paste back. Does nothing if
    /// another client took the selection or Sonori could not save the content.
    pub fn restore(self) {
        std::thread::sleep(RESTORE_GRACE);
        let mut reads = self.shared.reads.lock();
        if reads.replaced || reads.current.is_none() {
            return;
        }
        match self.saved {
            Saved::Unknown => {}
            Saved::Empty => {
                reads.current = None;
                self.device.set_selection(None);
            }
            Saved::Content(clip) => {
                let source = new_source(&self.manager, &self.qh, clip, false);
                reads.current = Some(source.id());
                self.device.set_selection(Some(&source));
            }
        }
        let _ = self.conn.flush();
    }
}

fn new_source(
    manager: &ZwlrDataControlManagerV1,
    qh: &QueueHandle<State>,
    clip: Clip,
    transcript: bool,
) -> ZwlrDataControlSourceV1 {
    let mimes: Vec<String> = clip.iter().map(|(mime, _)| mime.clone()).collect();
    let source = manager.create_data_source(qh, Arc::new(Served { clip, transcript }));
    for mime in mimes {
        source.offer(mime);
    }
    source
}

/// Reads every type of the current selection, within `SAVE_TIME` and `SAVE_BYTES`.
fn save(conn: &Connection, queue: &mut EventQueue<State>, state: &mut State) -> Saved {
    let Some(offer) = state.selection.take() else {
        return Saved::Empty;
    };
    let mimes = state.offers.remove(&offer.id()).unwrap_or_default();
    let deadline = Instant::now() + SAVE_TIME;
    let mut clip = Clip::new();
    let mut total = 0;
    for mime in mimes {
        // X11 meta targets from Xwayland carry no content.
        if matches!(
            mime.as_str(),
            "TARGETS" | "MULTIPLE" | "TIMESTAMP" | "SAVE_TARGETS"
        ) {
            continue;
        }
        let Ok((reader, writer)) = std::io::pipe() else {
            return Saved::Unknown;
        };
        offer.receive(mime.clone(), writer.as_fd());
        drop(writer);
        if conn.flush().is_err() {
            return Saved::Unknown;
        }
        let Some(bytes) = read_until(OwnedFd::from(reader), deadline, SAVE_BYTES - total) else {
            return Saved::Unknown;
        };
        total += bytes.len();
        clip.push((mime, Arc::from(bytes)));
    }
    offer.destroy();
    let _ = queue.roundtrip(state);
    if clip.is_empty() {
        Saved::Unknown
    } else {
        Saved::Content(clip)
    }
}

/// Reads a pipe to its end. `None` past the deadline or past `limit` bytes.
fn read_until(fd: OwnedFd, deadline: Instant, limit: usize) -> Option<Vec<u8>> {
    let raw = fd.as_raw_fd();
    // SAFETY: `raw` is an open pipe that `fd` owns for this whole function.
    unsafe {
        let flags = libc::fcntl(raw, libc::F_GETFL);
        libc::fcntl(raw, libc::F_SETFL, flags | libc::O_NONBLOCK);
    }
    let mut file = std::fs::File::from(fd);
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        match file.read(&mut chunk) {
            Ok(0) => return Some(bytes),
            Ok(n) => {
                bytes.extend_from_slice(&chunk[..n]);
                if bytes.len() > limit {
                    return None;
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                let left = deadline.checked_duration_since(Instant::now())?;
                let mut poll = libc::pollfd {
                    fd: raw,
                    events: libc::POLLIN,
                    revents: 0,
                };
                // SAFETY: one valid pollfd for an open descriptor.
                let ready = unsafe { libc::poll(&mut poll, 1, left.as_millis() as i32) };
                if ready <= 0 {
                    return None;
                }
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    }
}

impl Dispatch<ZwlrDataControlSourceV1, Arc<Served>> for State {
    fn event(
        state: &mut Self,
        source: &ZwlrDataControlSourceV1,
        event: zwlr_data_control_source_v1::Event,
        served: &Arc<Served>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_data_control_source_v1::Event::Send { mime_type, fd } => {
                let Some((_, bytes)) = served.clip.iter().find(|(m, _)| *m == mime_type) else {
                    return;
                };
                // A clipboard manager reads the hint, not the text.
                if served.transcript && mime_type != SECRET_HINT {
                    state.shared.reads.lock().times.push(Instant::now());
                    state.shared.changed.notify_all();
                }
                write_bytes(fd, bytes.clone());
            }
            zwlr_data_control_source_v1::Event::Cancelled => {
                let mut reads = state.shared.reads.lock();
                if reads.current.as_ref() == Some(&source.id()) {
                    reads.current = None;
                    reads.replaced |= served.transcript;
                }
                drop(reads);
                state.shared.changed.notify_all();
                source.destroy();
            }
            _ => {}
        }
    }
}

/// A reader that never drains the pipe must not stall the event loop.
fn write_bytes(fd: OwnedFd, bytes: Arc<[u8]>) {
    std::thread::spawn(move || {
        let _ = std::fs::File::from(fd).write_all(&bytes);
    });
}

impl Dispatch<ZwlrDataControlDeviceV1, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZwlrDataControlDeviceV1,
        event: zwlr_data_control_device_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_data_control_device_v1::Event::Selection { id } = event {
            if let Some(old) = std::mem::replace(&mut state.selection, id) {
                state.offers.remove(&old.id());
                old.destroy();
            }
        }
    }

    event_created_child!(State, ZwlrDataControlDeviceV1, [
        zwlr_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ZwlrDataControlOfferV1, ()),
    ]);
}

impl State {
    fn record_offer(&mut self, offer: &ZwlrDataControlOfferV1, mime: String) {
        self.offers.entry(offer.id()).or_default().push(mime);
    }
}

impl Dispatch<ZwlrDataControlOfferV1, ()> for State {
    fn event(
        state: &mut Self,
        offer: &ZwlrDataControlOfferV1,
        event: zwlr_data_control_offer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_data_control_offer_v1::Event::Offer { mime_type } = event {
            state.record_offer(offer, mime_type);
        }
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: wayland_client::protocol::wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSeat, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlSeat,
        _: wayland_client::protocol::wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrDataControlManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwlrDataControlManagerV1,
        _: zwlr_data_control_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
