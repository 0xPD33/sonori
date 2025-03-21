use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use winit::{
    dpi::PhysicalPosition,
    event::{ElementState, MouseButton, MouseScrollDelta},
    event_loop::ActiveEventLoop,
};

use super::buttons::ButtonType;
use super::common::AudioVisualizationData;
use parking_lot::RwLock;

// Event handling methods that will be used by WindowState
pub struct EventHandler {
    pub cursor_position: Option<PhysicalPosition<f64>>,
    pub hovering_transcript: bool,
    pub auto_scroll: bool,
    pub recording: Option<Arc<AtomicBool>>,
}

impl EventHandler {
    pub fn new(recording: Option<Arc<AtomicBool>>) -> Self {
        Self {
            cursor_position: None,
            hovering_transcript: false,
            auto_scroll: true,
            recording,
        }
    }

    pub fn handle_scroll(
        &mut self,
        scroll_offset: &mut f32,
        max_scroll_offset: f32,
        delta: MouseScrollDelta,
    ) {
        let line_scroll_speed = 15.0;
        let pixel_scroll_multiplier = 0.75;

        let prev_scroll_offset = *scroll_offset;

        match delta {
            MouseScrollDelta::LineDelta(_, y) => {
                *scroll_offset = (*scroll_offset - y * line_scroll_speed)
                    .max(0.0)
                    .min(max_scroll_offset);
            }
            MouseScrollDelta::PixelDelta(PhysicalPosition { y, .. }) => {
                *scroll_offset = (*scroll_offset + y as f32 * pixel_scroll_multiplier)
                    .max(0.0)
                    .min(max_scroll_offset);
            }
        }

        if *scroll_offset < prev_scroll_offset {
            self.auto_scroll = false;
        } else if (max_scroll_offset - *scroll_offset).abs() < 1.0 {
            self.auto_scroll = true;
        }
    }

    pub fn handle_cursor_moved(
        &mut self,
        position: PhysicalPosition<f64>,
        text_area_width: u32,
        text_area_height: u32,
        window_width: u32,
        window_height: u32,
        button_manager: &mut super::buttons::ButtonManager,
    ) {
        // Store the current cursor position
        self.cursor_position = Some(position);

        // Check if cursor is within the window bounds first
        let is_in_window = position.x >= 0.0
            && position.x <= window_width as f64
            && position.y >= 0.0
            && position.y <= window_height as f64;

        // Then check if cursor is within the text area bounds
        let is_in_text_area = position.x >= 0.0
            && position.x <= text_area_width as f64
            && position.y >= 0.0
            && position.y <= text_area_height as f64;

        // Update hovering state - must be both in window and in text area
        self.hovering_transcript = is_in_window && is_in_text_area;

        // Only update button states when cursor is hovering over the transcript
        if self.hovering_transcript {
            button_manager.handle_mouse_move(position);
        } else {
            // Reset all buttons to normal state when cursor leaves text area or window
            button_manager.reset_hover_states();
        }
    }

    pub fn copy_transcript(audio_data: &Option<Arc<RwLock<AudioVisualizationData>>>) {
        if let Some(audio_data) = audio_data {
            let audio_data_lock = audio_data.read();
            let transcript = audio_data_lock.transcript.clone();
            drop(audio_data_lock);

            // Use wl-copy command for clipboard
            if let Err(e) = Command::new("wl-copy")
                .arg(&transcript)
                .spawn()
                .map(|child| child.wait_with_output())
            {
                println!("Failed to copy to clipboard: {:?}", e);
            } else {
                println!("Copied transcript to clipboard using wl-copy");
            }
        }
    }

    pub fn reset_transcript(
        audio_data: &Option<Arc<RwLock<AudioVisualizationData>>>,
        last_transcript_len: &mut usize,
        scroll_offset: &mut f32,
        max_scroll_offset: &mut f32,
    ) {
        if let Some(audio_data) = audio_data {
            let mut audio_data_lock = audio_data.write();

            // Clear the local transcript
            audio_data_lock.transcript.clear();

            // Set the reset flag
            audio_data_lock.reset_requested = true;

            // Reset UI state
            *last_transcript_len = 0;
            *scroll_offset = 0.0;
            *max_scroll_offset = 0.0;
        }
    }

    pub fn toggle_recording(recording: &Option<Arc<AtomicBool>>) {
        if let Some(recording) = recording {
            let was_recording = recording.load(Ordering::Relaxed);
            recording.store(!was_recording, Ordering::Relaxed);
        }
    }

    pub fn quit(running: &Option<Arc<AtomicBool>>) {
        if let Some(running) = running {
            running.store(false, Ordering::Relaxed);
        }
    }

    pub fn handle_mouse_input(
        &self,
        button: MouseButton,
        state: ElementState,
        position: PhysicalPosition<f64>,
        button_manager: &mut super::buttons::ButtonManager,
        audio_data: &Option<Arc<RwLock<AudioVisualizationData>>>,
        last_transcript_len: &mut usize,
        scroll_offset: &mut f32,
        max_scroll_offset: &mut f32,
        running: &Option<Arc<AtomicBool>>,
        event_loop: Option<&dyn ActiveEventLoop>,
    ) -> bool {
        if self.hovering_transcript {
            if let Some(button_type) = button_manager.handle_pointer_event(button, state, position)
            {
                match button_type {
                    ButtonType::Copy => {
                        Self::copy_transcript(audio_data);
                    }
                    ButtonType::Reset => {
                        Self::reset_transcript(
                            audio_data,
                            last_transcript_len,
                            scroll_offset,
                            max_scroll_offset,
                        );
                    }
                    ButtonType::Close => {
                        println!("Close button clicked, initiating shutdown sequence");
                        // First set the running flag to false
                        Self::quit(running);

                        // Do NOT immediately exit the event loop - let the monitors handle it
                    }
                    ButtonType::Pause | ButtonType::Play => {
                        // For both pause and play, toggle the recording state
                        Self::toggle_recording(&self.recording);
                    }
                }
                return true;
            }
        }
        false
    }

    pub fn handle_cursor_leave(&mut self, button_manager: &mut super::buttons::ButtonManager) {
        // When cursor leaves the window, ensure hovering_transcript is false
        self.hovering_transcript = false;

        // Reset all button states
        button_manager.reset_hover_states();
    }
}
