use anyhow::{Context, Result};
use ashpd::desktop::remote_desktop::{DeviceType, KeyState, RemoteDesktop};
use ashpd::desktop::screencast::{CursorMode, Screencast, SourceType};
use ashpd::desktop::PersistMode;
use ashpd::desktop::Session;
use ashpd::zbus;
use xkbcommon::xkb::keysyms;

use crate::portal_tokens::PortalTokens;

/// Manages an XDG Desktop Portal RemoteDesktop session to inject keystrokes
pub struct PortalInput {
    connection: zbus::Connection,
    rd: RemoteDesktop<'static>,
    rd_session: Session<'static, RemoteDesktop<'static>>,
    screencast_active: bool,
}

impl PortalInput {
    /// Create a new portal input session, preferring keyboard-only access and
    /// falling back to a screencast request on compositors that require it.
    pub async fn new() -> Result<Self> {
        let connection = zbus::Connection::session().await?;

        match Self::try_new_internal(connection.clone(), false).await {
            Ok(instance) => Ok(instance),
            Err(first_err) => {
                eprintln!(
                    "Portal keyboard session without screencast failed ({}), retrying with screencast",
                    first_err
                );
                Self::try_new_internal(connection, true)
                    .await
                    .context(
                        "Failed to establish portal keyboard control even with screencast fallback",
                    )
            }
        }
    }

    async fn try_new_internal(
        connection: zbus::Connection,
        start_screencast: bool,
    ) -> Result<Self> {
        let rd = RemoteDesktop::new().await?;
        let mut tokens = PortalTokens::load();
        let (rd_session, tokens_updated) =
            Self::configure_remote_desktop(&rd, start_screencast, &mut tokens).await?;

        if tokens_updated {
            if let Err(e) = tokens.save() {
                eprintln!("Failed to persist portal restore tokens: {}", e);
            }
        }

        Ok(Self {
            connection,
            rd,
            rd_session,
            screencast_active: start_screencast,
        })
    }

    async fn configure_remote_desktop(
        rd: &RemoteDesktop<'static>,
        start_screencast: bool,
        tokens: &mut PortalTokens,
    ) -> Result<(Session<'static, RemoteDesktop<'static>>, bool)> {
        let mut tokens_updated = false;
        let rd_session = rd.create_session().await?;

        let keyboard_restore = tokens.remote_keyboard.as_deref();
        rd.select_devices(
            &rd_session,
            DeviceType::Keyboard.into(),
            keyboard_restore,
            PersistMode::ExplicitlyRevoked,
        )
        .await?
        .response()?;

        if start_screencast {
            let screencast = Screencast::new().await?;
            let screencast_restore = tokens.remote_screencast.as_deref();
            screencast
                .select_sources(
                    &rd_session,
                    CursorMode::Hidden,
                    SourceType::Monitor.into(),
                    false,
                    screencast_restore,
                    PersistMode::ExplicitlyRevoked,
                )
                .await?
                .response()?;
            let streams = screencast.start(&rd_session, None).await?.response()?;
            if let Some(token) = streams.restore_token() {
                tokens_updated |= tokens
                    .remote_screencast
                    .replace(token.to_string())
                    .as_deref()
                    != Some(token);
            } else if tokens.remote_screencast.take().is_some() {
                tokens_updated = true;
            }
        }

        let started = rd.start(&rd_session, None).await?.response()?;
        if let Some(token) = started.restore_token() {
            tokens_updated |=
                tokens.remote_keyboard.replace(token.to_string()).as_deref() != Some(token);
        } else if tokens.remote_keyboard.take().is_some() {
            tokens_updated = true;
        }

        Ok((rd_session, tokens_updated))
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

    /// Send Ctrl+Shift+V via keysym to paste from clipboard (for terminals)
    pub async fn paste_via_ctrl_shift_v(&self) -> Result<()> {
        use tokio::time::{sleep, Duration};

        // Press Control
        self.rd
            .notify_keyboard_keysym(
                &self.rd_session,
                keysyms::KEY_Control_L as i32,
                KeyState::Pressed,
            )
            .await?;
        sleep(Duration::from_millis(10)).await;

        // Press Shift
        self.rd
            .notify_keyboard_keysym(
                &self.rd_session,
                keysyms::KEY_Shift_L as i32,
                KeyState::Pressed,
            )
            .await?;
        sleep(Duration::from_millis(10)).await;

        // Press 'v'
        self.rd
            .notify_keyboard_keysym(&self.rd_session, keysyms::KEY_v as i32, KeyState::Pressed)
            .await?;
        sleep(Duration::from_millis(50)).await;

        // Release 'v'
        self.rd
            .notify_keyboard_keysym(&self.rd_session, keysyms::KEY_v as i32, KeyState::Released)
            .await?;
        sleep(Duration::from_millis(10)).await;

        // Release Shift
        self.rd
            .notify_keyboard_keysym(
                &self.rd_session,
                keysyms::KEY_Shift_L as i32,
                KeyState::Released,
            )
            .await?;
        sleep(Duration::from_millis(10)).await;

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
