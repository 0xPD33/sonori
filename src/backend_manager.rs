use parking_lot::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::backend::factory::create_backend;
use crate::backend::{BackendConfig, BackendType, TranscriptionBackend};
use crate::ui::common::{BackendStatus, BackendStatusState};

pub enum BackendCommand {
    /// Reload the backend with new config.
    /// `model_name` is the user-facing name (e.g. "large-v3-turbo"), not a filesystem path.
    Reload {
        backend_config: BackendConfig,
        model_name: String,
    },
    /// Shutdown the backend manager
    Shutdown,
}

pub struct BackendManager {
    backend: Arc<parking_lot::Mutex<Option<Arc<TranscriptionBackend>>>>,
    backend_ready: Arc<AtomicBool>,
    status: Arc<RwLock<BackendStatus>>,
    command_tx: mpsc::UnboundedSender<BackendCommand>,
    command_rx: Option<mpsc::UnboundedReceiver<BackendCommand>>,
    task_handle: Option<tokio::task::JoinHandle<()>>,
}

impl BackendManager {
    pub fn new(
        backend: Arc<parking_lot::Mutex<Option<Arc<TranscriptionBackend>>>>,
        backend_ready: Arc<AtomicBool>,
        status: Arc<RwLock<BackendStatus>>,
    ) -> Self {
        let (command_tx, command_rx) = mpsc::unbounded_channel();

        Self {
            backend,
            backend_ready,
            status,
            command_tx,
            command_rx: Some(command_rx),
            task_handle: None,
        }
    }

    pub fn start(&mut self) {
        let rx = self.command_rx.take().expect("start called twice");
        let backend = self.backend.clone();
        let backend_ready = self.backend_ready.clone();
        let status = self.status.clone();

        self.task_handle = Some(tokio::spawn(async move {
            Self::run_command_loop(rx, backend, backend_ready, status).await;
        }));
    }

    pub fn command_sender(&self) -> mpsc::UnboundedSender<BackendCommand> {
        self.command_tx.clone()
    }

    pub fn status(&self) -> Arc<RwLock<BackendStatus>> {
        self.status.clone()
    }

    async fn run_command_loop(
        mut rx: mpsc::UnboundedReceiver<BackendCommand>,
        backend: Arc<parking_lot::Mutex<Option<Arc<TranscriptionBackend>>>>,
        backend_ready: Arc<AtomicBool>,
        status: Arc<RwLock<BackendStatus>>,
    ) {
        while let Some(command) = rx.recv().await {
            match command {
                BackendCommand::Reload {
                    backend_config,
                    model_name,
                } => {
                    // Signal that backend is not ready during reload
                    backend_ready.store(false, Ordering::SeqCst);

                    let (prev_backend_name, prev_model_name) = {
                        let mut s = status.write();
                        let prev = (s.backend_name.clone(), s.model_name.clone());
                        s.backend_name = match backend_config.backend {
                            BackendType::CTranslate2 => "CTranslate2".to_string(),
                            BackendType::WhisperCpp => "WhisperCpp".to_string(),
                            BackendType::Moonshine => "Moonshine".to_string(),
                            BackendType::Parakeet => "Parakeet".to_string(),
                        };
                        s.model_name = model_name.clone();
                        s.state =
                            BackendStatusState::Loading("Resolving model...".to_string());
                        prev
                    };

                    {
                        let mut s = status.write();
                        s.download_progress = None;
                    }

                    let status_for_progress = status.clone();
                    let on_progress = move |progress: f64| {
                        let mut s = status_for_progress.write();
                        s.download_progress = Some(progress as f32);
                    };

                    let model_path = match crate::download::resolve_model_path_with_progress(
                        &model_name,
                        backend_config.backend,
                        &backend_config.quantization_level,
                        Some(&on_progress),
                    )
                    .await
                    {
                        Ok(p) => {
                            status.write().download_progress = None;
                            p
                        }
                        Err(e) => {
                            let mut s = status.write();
                            s.download_progress = None;
                            s.backend_name = prev_backend_name;
                            s.model_name = prev_model_name;
                            s.state = BackendStatusState::Error(format!(
                                "Model resolution failed: {}",
                                e
                            ));
                            s.error_time = Some(std::time::Instant::now());
                            // Restore backend_ready since old backend is still valid
                            backend_ready.store(true, Ordering::SeqCst);
                            eprintln!("BackendManager: Model resolution failed: {}", e);
                            continue;
                        }
                    };

                    {
                        let mut s = status.write();
                        s.state =
                            BackendStatusState::Loading("Loading backend...".to_string());
                    }

                    match create_backend(backend_config.backend, &model_path, &backend_config).await
                    {
                        Ok(new_backend) => {
                            *backend.lock() = Some(Arc::new(new_backend));
                            backend_ready.store(true, Ordering::SeqCst);

                            let mut s = status.write();
                            s.state = BackendStatusState::Ready;

                            println!("BackendManager: Backend reloaded successfully");
                        }
                        Err(e) => {
                            // Restore backend_ready since old backend is still valid
                            backend_ready.store(true, Ordering::SeqCst);

                            let mut s = status.write();
                            s.backend_name = prev_backend_name;
                            s.model_name = prev_model_name;
                            s.state =
                                BackendStatusState::Error(format!("Reload failed: {}", e));
                            s.error_time = Some(std::time::Instant::now());

                            eprintln!("BackendManager: Backend reload failed: {}", e);
                        }
                    }
                }
                BackendCommand::Shutdown => {
                    println!("BackendManager: Shutting down");
                    break;
                }
            }
        }
    }
}
