use anyhow::Result;
use ashpd::desktop::remote_desktop::{DeviceType, KeyState, RemoteDesktop};
use ashpd::desktop::screencast::{CursorMode, Screencast, SourceType};
use ashpd::desktop::PersistMode;
use ashpd::desktop::Session;
use ashpd::zbus;
use xkbcommon::xkb::keysyms;

/// Manages an XDG Desktop Portal RemoteDesktop session to inject keystrokes
pub struct PortalInput {
    connection: zbus::Connection,
    rd: RemoteDesktop<'static>,
    rd_session: Session<'static, RemoteDesktop<'static>>,
    screencast_active: bool,
}

impl PortalInput {
    /// Create a new portal input session, optionally starting a screencast (commonly required)
    pub async fn new(start_screencast: bool) -> Result<Self> {
        let connection = zbus::Connection::session().await?;
        let rd = RemoteDesktop::new().await?;

        // Many backends require a screencast session to enable input control
        if start_screencast {
            let screencast = Screencast::new().await?;
            // Create a session and pick the entire monitor (no persist)
            let session = screencast.create_session().await?;
            screencast
                .select_sources(
                    &session,
                    CursorMode::Hidden,
                    SourceType::Monitor.into(),
                    false,
                    None,
                    PersistMode::DoNot,
                )
                .await?;
            // Start with a dummy window identifier (use None for no parent window)
            let _streams = screencast.start(&session, None).await?;
        }

        // Create RemoteDesktop session and request keyboard control
        let rd_session = rd.create_session().await?;
        rd.select_devices(
            &rd_session,
            DeviceType::Keyboard.into(),
            None,
            PersistMode::DoNot,
        )
        .await?;
        rd.start(&rd_session, None).await?;

        Ok(Self {
            connection,
            rd,
            rd_session,
            screencast_active: start_screencast,
        })
    }

    /// Send Ctrl+V via keysym to paste from clipboard
    pub async fn paste_via_ctrl_v(&self) -> Result<()> {
        // Press Control
        self.rd
            .notify_keyboard_keysym(
                &self.rd_session,
                keysyms::KEY_Control_L as i32,
                KeyState::Pressed,
            )
            .await?;
        // Press 'v'
        self.rd
            .notify_keyboard_keysym(&self.rd_session, keysyms::KEY_v as i32, KeyState::Pressed)
            .await?;
        // Release 'v'
        self.rd
            .notify_keyboard_keysym(&self.rd_session, keysyms::KEY_v as i32, KeyState::Released)
            .await?;
        // Release Control
        self.rd
            .notify_keyboard_keysym(
                &self.rd_session,
                keysyms::KEY_Control_L as i32,
                KeyState::Released,
            )
            .await?;
        Ok(())
    }
}
