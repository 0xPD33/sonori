use anyhow::Result;
use ndarray::{ArrayD, IxDyn};
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Tensor;
use std::sync::OnceLock;
use std::path::Path;

static ORT_ENV_INITIALIZED: OnceLock<anyhow::Result<()>> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct OnnxSessionOptions {
    pub intra_threads: usize,
    pub inter_threads: usize,
    pub execution_provider: Option<String>,
}

impl Default for OnnxSessionOptions {
    fn default() -> Self {
        Self {
            intra_threads: 1,
            inter_threads: 1,
            execution_provider: None,
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

    // Note: Execution provider configuration in ort 2.0 works differently.
    // We would need to use `with_execution_providers` and likely import providers.
    // For now, we rely on automatic provider registration if features are enabled.
    // If explicit control is needed, we will need to update this logic.

    let session = builder.commit_from_file(path)?;
    Ok(session)
}

pub fn tensor_f32_from_slice(shape: &[usize], data: &[f32]) -> Result<Tensor<f32>> {
    let array = ArrayD::from_shape_vec(IxDyn(shape), data.to_vec())?;
    Ok(Tensor::from_array(array)?)
}
