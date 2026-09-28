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
    _connection: zbus::Connection,
    rd: RemoteDesktop<'static>,
    rd_session: Session<'static, RemoteDesktop<'static>>,
    _screencast_active: bool,
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
                Self::try_new_internal(connection, true).await.context(
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
            _connection: connection,
            rd,
            rd_session,
            _screencast_active: start_screencast,
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

    /// Send Ctrl+V, or Ctrl+Shift+V (for terminals), to paste from the clipboard.
    /// On failure every key is released, so no modifier stays stuck down.
    pub async fn paste(&self, with_shift: bool) -> Result<()> {
        let mut keys = vec![keysyms::KEY_Control_L];
        if with_shift {
            keys.push(keysyms::KEY_Shift_L);
        }
        keys.push(keysyms::KEY_v);

        let result = self.press_chord(&keys).await;
        if result.is_err() {
            for &key in keys.iter().rev() {
                let _ = self.send_key(key, KeyState::Released).await;
            }
        }
        result
    }

    async fn press_chord(&self, keys: &[u32]) -> Result<()> {
        use tokio::time::{sleep, Duration};

        for &key in keys {
            self.send_key(key, KeyState::Pressed).await?;
            sleep(Duration::from_millis(10)).await;
        }
        // Some apps miss a keypress that is released at once.
        sleep(Duration::from_millis(40)).await;
        for &key in keys.iter().rev() {
            self.send_key(key, KeyState::Released).await?;
            sleep(Duration::from_millis(10)).await;
        }
        Ok(())
    }

    async fn send_key(&self, keysym: u32, state: KeyState) -> Result<()> {
        self.rd
            .notify_keyboard_keysym(&self.rd_session, keysym as i32, state)
            .await?;
        Ok(())
    }
}
