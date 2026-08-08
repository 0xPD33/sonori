use serde::{Deserialize, Serialize};

/// Window position presets for layer-shell anchoring
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum WindowPosition {
    BottomLeft,
    #[default]
    BottomCenter,
    BottomRight,
    TopLeft,
    TopCenter,
    TopRight,
    MiddleLeft,
    MiddleCenter,
    MiddleRight,
    Custom,
}

impl WindowPosition {
    /// Convert window position to Wayland layer-shell anchor flags
    /// Returns the anchor flags used to position the window at the desired location
    #[cfg(target_os = "linux")]
    pub fn to_wayland_anchor(&self) -> winit::platform::wayland::Anchor {
        use winit::platform::wayland::Anchor;

        match self {
            WindowPosition::BottomLeft => Anchor::BOTTOM | Anchor::LEFT,
            WindowPosition::BottomCenter => Anchor::BOTTOM,
            WindowPosition::BottomRight => Anchor::BOTTOM | Anchor::RIGHT,
            WindowPosition::TopLeft => Anchor::TOP | Anchor::LEFT,
            WindowPosition::TopCenter => Anchor::TOP,
            WindowPosition::TopRight => Anchor::TOP | Anchor::RIGHT,
            WindowPosition::MiddleLeft => Anchor::LEFT,
            WindowPosition::MiddleCenter => Anchor::empty(), // No anchors = centered
            WindowPosition::MiddleRight => Anchor::RIGHT,
            WindowPosition::Custom => Anchor::TOP | Anchor::LEFT,
        }
    }
}

/// Pixel position for a user-dragged overlay window.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct CustomWindowPosition {
    pub x: i32,
    pub y: i32,
}

/// Configuration for display and rendering settings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplayConfig {
    /// VSync mode: "Auto", "Enabled", "Adaptive", "Disabled", or "Mailbox"
    /// - Auto: Use first available present mode (default behavior)
    /// - Enabled: Traditional vsync (Fifo) - waits for vertical blank, no tearing
    /// - Adaptive: Adaptive vsync (FifoRelaxed) - vsync when above refresh rate, immediate when below
    /// - Disabled: No vsync (Immediate) - lowest latency, potential tearing
    /// - Mailbox: Triple-buffered vsync - no tearing, lowest latency with vsync
    pub vsync_mode: String,

    /// Target FPS when vsync is disabled (prevents unbounded frame rates)
    pub target_fps: u32,

    /// Window position on screen (layer-shell anchor configuration)
    /// Available positions: BottomLeft, BottomCenter, BottomRight,
    /// TopLeft, TopCenter, TopRight, MiddleLeft, MiddleCenter, MiddleRight, Custom
    pub window_position: WindowPosition,

    /// Position used when window_position is Custom.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_window_position: Option<CustomWindowPosition>,
}

/// Configuration for system tray behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowBehaviorConfig {
    /// Whether to show the application icon in the system tray
    pub show_in_system_tray: bool,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            vsync_mode: "Enabled".to_string(), // Default to traditional vsync
            target_fps: 60,                    // Cap at 60 FPS when vsync disabled
            window_position: WindowPosition::default(),
            custom_window_position: None,
        }
    }
}

impl Default for WindowBehaviorConfig {
    fn default() -> Self {
        Self {
            show_in_system_tray: true, // Show tray icon by default
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum VisualThemePreset {
    #[default]
    Focus,
    Pulse,
    Terminal,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum SpectrogramSkin {
    #[default]
    Bars,
    Waveform,
    Meter,
}

#[derive(Debug, Clone, Copy)]
pub struct ResolvedVisualTheme {
    pub speaking_color: [f32; 4],
    pub idle_color: [f32; 4],
    pub recording_indicator_color: [f32; 4],
    pub typewriter_default: bool,
}

impl VisualThemePreset {
    pub fn resolve(self) -> ResolvedVisualTheme {
        match self {
            VisualThemePreset::Focus => ResolvedVisualTheme {
                speaking_color: [0.1, 0.9, 0.5, 1.0],
                idle_color: [1.0, 0.85, 0.15, 1.0],
                recording_indicator_color: [0.9, 0.2, 0.2, 1.0],
                typewriter_default: false,
            },
            VisualThemePreset::Pulse => ResolvedVisualTheme {
                speaking_color: [0.14, 0.95, 0.72, 1.0],
                idle_color: [0.82, 0.88, 1.0, 1.0],
                recording_indicator_color: [1.0, 0.18, 0.32, 1.0],
                typewriter_default: false,
            },
            VisualThemePreset::Terminal => ResolvedVisualTheme {
                speaking_color: [0.40, 1.0, 0.52, 1.0],
                idle_color: [0.70, 0.95, 0.72, 1.0],
                recording_indicator_color: [0.40, 1.0, 0.52, 1.0],
                typewriter_default: true,
            },
        }
    }
}

/// Configuration for UI appearance settings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    /// Curated visual theme preset
    pub visual_theme: VisualThemePreset,

    /// Spectrogram rendering skin
    pub spectrogram_skin: SpectrogramSkin,

    /// Base font size for transcript text (default: 10.0)
    /// Actual rendered size is font_size * display_scale
    pub font_size: f32,

    /// Text color when speaking/actively transcribing [r, g, b, a] (0.0-1.0)
    pub speaking_color: [f32; 4],

    /// Text color when idle/not speaking [r, g, b, a] (0.0-1.0)
    pub idle_color: [f32; 4],

    /// Recording indicator dot color [r, g, b, a] (0.0-1.0)
    pub recording_indicator_color: [f32; 4],

    /// Whether to show the pulsing recording indicator
    pub show_recording_indicator: bool,

    /// Whether to enable typewriter effect when transcription completes (manual mode)
    pub typewriter_effect: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            visual_theme: VisualThemePreset::Focus,
            spectrogram_skin: SpectrogramSkin::Bars,
            font_size: 10.0,
            speaking_color: [0.1, 0.9, 0.5, 1.0], // Teal-green
            idle_color: [1.0, 0.85, 0.15, 1.0],   // Gold
            recording_indicator_color: [0.9, 0.2, 0.2, 1.0], // Red
            show_recording_indicator: true,
            typewriter_effect: false,
        }
    }
}

impl UiConfig {
    pub fn effective_speaking_color(&self) -> [f32; 4] {
        match self.visual_theme {
            VisualThemePreset::Focus => self.speaking_color,
            _ => self.visual_theme.resolve().speaking_color,
        }
    }

    pub fn effective_idle_color(&self) -> [f32; 4] {
        match self.visual_theme {
            VisualThemePreset::Focus => self.idle_color,
            _ => self.visual_theme.resolve().idle_color,
        }
    }

    pub fn effective_recording_indicator_color(&self) -> [f32; 4] {
        match self.visual_theme {
            VisualThemePreset::Focus => self.recording_indicator_color,
            _ => self.visual_theme.resolve().recording_indicator_color,
        }
    }

    pub fn effective_spectrogram_color(&self) -> [f32; 4] {
        match self.visual_theme {
            VisualThemePreset::Focus => [1.0, 1.0, 1.0, 1.0],
            VisualThemePreset::Pulse => [0.18, 0.95, 0.72, 1.0],
            VisualThemePreset::Terminal => [0.40, 1.0, 0.52, 1.0],
        }
    }

    pub fn effective_typewriter_enabled(&self) -> bool {
        self.typewriter_effect || self.visual_theme.resolve().typewriter_default
    }
}

impl DisplayConfig {
    /// Convert string vsync_mode to wgpu::PresentMode, with fallback logic
    pub fn to_present_mode(&self, available_modes: &[wgpu::PresentMode]) -> wgpu::PresentMode {
        let preferred = match self.vsync_mode.as_str() {
            "Enabled" => wgpu::PresentMode::Fifo,
            "Adaptive" => wgpu::PresentMode::FifoRelaxed,
            "Disabled" => wgpu::PresentMode::Immediate,
            "Mailbox" => wgpu::PresentMode::Mailbox,
            "Auto" => {
                // Auto mode: prefer Fifo, but accept whatever is available
                return available_modes
                    .first()
                    .copied()
                    .unwrap_or(wgpu::PresentMode::Fifo);
            }
            _ => {
                // Auto mode: prefer Fifo, but accept whatever is available
                return available_modes
                    .first()
                    .copied()
                    .unwrap_or(wgpu::PresentMode::Fifo);
            }
        };

        // Check if preferred mode is available
        if available_modes.contains(&preferred) {
            preferred
        } else {
            // Fallback to Fifo (guaranteed to be available), or first available
            if available_modes.contains(&wgpu::PresentMode::Fifo) {
                println!(
                    "Warning: Preferred vsync mode '{}' not available, falling back to Fifo",
                    self.vsync_mode
                );
                wgpu::PresentMode::Fifo
            } else {
                println!(
                    "Warning: Preferred vsync mode '{}' not available, using first available mode",
                    self.vsync_mode
                );
                available_modes
                    .first()
                    .copied()
                    .unwrap_or(wgpu::PresentMode::Fifo)
            }
        }
    }
}

