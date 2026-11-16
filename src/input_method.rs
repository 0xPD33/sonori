//! Inserts text into the focused text field through `zwp_input_method_v2`, as an
//! input method does: no clipboard and no key events, so terminals need no paste
//! shortcut. niri and Sway let text fields enable text input only while an input
//! method is bound. So Sonori binds one for each insertion: the focused field
//! enables, the compositor activates Sonori's input method, and Sonori commits.

use std::os::fd::AsRawFd;
use std::time::{Duration, Instant};

use wayland_client::backend::WaylandError;
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_registry::WlRegistry, wl_seat::WlSeat};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle};
use wayland_protocols_misc::zwp_input_method_v2::client::{
    zwp_input_method_manager_v2::ZwpInputMethodManagerV2,
    zwp_input_method_v2::{self, ZwpInputMethodV2},
};

/// How long the focused app may take to enable text input after the bind.
const ACTIVATE_WAIT: Duration = Duration::from_millis(300);
/// Wayland caps one message at 4 KiB, so long text goes in parts.
const PART_BYTES: usize = 3000;
/// Time for the app's answer to a part, so the next part has the current serial.
const PART_WAIT: Duration = Duration::from_millis(30);

#[derive(Default)]
struct State {
    /// Activation that the next `done` applies.
    pending: bool,
    active: bool,
    /// `commit` must carry the number of `done` events, or the compositor
    /// discards the text.
    dones: u32,
    unavailable: bool,
}

/// Inserts `text` into the focused text field. Fails when the compositor has no
/// input method protocol, another input method runs, or the focused app does
/// not take input method text; nothing is inserted then.
pub fn insert(text: &str) -> Result<(), String> {
    let conn = Connection::connect_to_env().map_err(|e| e.to_string())?;
    let (globals, mut queue): (_, EventQueue<State>) =
        registry_queue_init(&conn).map_err(|e| e.to_string())?;
    let qh = queue.handle();
    let manager: ZwpInputMethodManagerV2 = globals
        .bind(&qh, 1..=1, ())
        .map_err(|_| "the compositor has no input method protocol".to_string())?;
    let seat: WlSeat = globals
        .bind(&qh, 1..=1, ())
        .map_err(|_| "the compositor has no seat".to_string())?;
    let im = manager.get_input_method(&seat, &qh, ());
    let mut state = State::default();

    let result = (|| {
        dispatch_until(&mut queue, &mut state, ACTIVATE_WAIT, |s| {
            s.active || s.unavailable
        })?;
        if state.unavailable {
            return Err("another input method is running".to_string());
        }
        if !state.active {
            return Err("the focused app takes no input method text".to_string());
        }
        for (index, part) in parts(text).enumerate() {
            if index > 0 {
                dispatch_until(&mut queue, &mut state, PART_WAIT, |_| false)?;
            }
            if !state.active {
                return Err("the text field lost focus".to_string());
            }
            im.commit_string(part.to_string());
            im.commit(state.dones);
            queue.roundtrip(&mut state).map_err(|e| e.to_string())?;
        }
        Ok(())
    })();

    im.destroy();
    manager.destroy();
    let _ = conn.flush();
    result
}

/// Splits `text` into parts of at most `PART_BYTES`, at character boundaries.
fn parts(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = text;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let mut end = rest.len().min(PART_BYTES);
        while !rest.is_char_boundary(end) {
            end -= 1;
        }
        let (part, tail) = rest.split_at(end);
        rest = tail;
        Some(part)
    })
}

/// Handles events until `stop` holds or `wait` runs out.
fn dispatch_until(
    queue: &mut EventQueue<State>,
    state: &mut State,
    wait: Duration,
    stop: impl Fn(&State) -> bool,
) -> Result<(), String> {
    let deadline = Instant::now() + wait;
    loop {
        queue.dispatch_pending(state).map_err(|e| e.to_string())?;
        let left = deadline.saturating_duration_since(Instant::now());
        if stop(state) || left.is_zero() {
            return Ok(());
        }
        queue.flush().map_err(|e| e.to_string())?;
        let Some(guard) = queue.prepare_read() else {
            continue;
        };
        let mut fd = libc::pollfd {
            fd: guard.connection_fd().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let millis = left.as_millis().clamp(1, i32::MAX as u128) as i32;
        // SAFETY: `fd` is one valid pollfd that lives for the call.
        if unsafe { libc::poll(&mut fd, 1, millis) } > 0 {
            match guard.read() {
                Ok(_) => {}
                Err(WaylandError::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e.to_string()),
            }
        }
    }
}

impl Dispatch<ZwpInputMethodV2, ()> for State {
    fn event(
        state: &mut Self,
        _: &ZwpInputMethodV2,
        event: zwp_input_method_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwp_input_method_v2::Event::Activate => state.pending = true,
            zwp_input_method_v2::Event::Deactivate => state.pending = false,
            zwp_input_method_v2::Event::Done => {
                state.dones += 1;
                state.active = state.pending;
            }
            zwp_input_method_v2::Event::Unavailable => state.unavailable = true,
            _ => {}
        }
    }
}

impl Dispatch<ZwpInputMethodManagerV2, ()> for State {
    fn event(
        _: &mut Self,
        _: &ZwpInputMethodManagerV2,
        _: <ZwpInputMethodManagerV2 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSeat, ()> for State {
    fn event(
        _: &mut Self,
        _: &WlSeat,
        _: <WlSeat as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as wayland_client::Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
