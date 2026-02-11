use anyhow::Result;
use ndarray::{ArrayD, IxDyn};
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Tensor;
use std::path::Path;
use std::sync::OnceLock;

static ORT_ENV_INITIALIZED: OnceLock<anyhow::Result<()>> = OnceLock::new();

/// Execution provider preference for ONNX sessions
#[derive(Debug, Clone, Default)]
pub enum ExecutionProviderPreference {
    /// Use CPU only
    #[default]
    CpuOnly,
    /// Prefer GPU (CUDA), fall back to CPU if unavailable
    PreferGpu,
}

#[derive(Debug, Clone)]
pub struct OnnxSessionOptions {
    pub intra_threads: usize,
    pub inter_threads: usize,
    pub execution_provider: ExecutionProviderPreference,
}

impl Default for OnnxSessionOptions {
    fn default() -> Self {
        Self {
            intra_threads: 1,
            inter_threads: 1,
            execution_provider: ExecutionProviderPreference::CpuOnly,
        }
    }
}

pub fn init_ort_environment() -> Result<()> {
    let init_result = ORT_ENV_INITIALIZED
        .get_or_init(|| ort::init().commit().map(|_| ()).map_err(|e| e.into()));
    init_result
        .as_ref()
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!(e.to_string()))
}

pub fn load_session(path: impl AsRef<Path>, options: &OnnxSessionOptions) -> Result<Session> {
    init_ort_environment()?;

    let builder = Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level3)?
        .with_intra_threads(options.intra_threads)?
        .with_inter_threads(options.inter_threads)?;

    // Configure execution providers based on preference
    #[cfg(feature = "ort-cuda")]
    if matches!(
        options.execution_provider,
        ExecutionProviderPreference::PreferGpu
    ) {
        use ort::execution_providers::cuda::CUDAExecutionProvider;
        builder = builder.with_execution_providers([CUDAExecutionProvider::default().build()])?;
        println!("ONNX session configured with CUDA execution provider");
    }

    let session = builder.commit_from_file(path)?;
    Ok(session)
}

pub fn tensor_f32_from_slice(shape: &[usize], data: &[f32]) -> Result<Tensor<f32>> {
    let array = ArrayD::from_shape_vec(IxDyn(shape), data.to_vec())?;
    Ok(Tensor::from_array(array)?)
}
