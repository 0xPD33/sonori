use crate::backend::traits::TranscriptionError;
use ndarray::{s, ArrayD, IxDyn};
use ort::session::Session;
use ort::value::Tensor;
use parking_lot::Mutex;

/// Decoder state: the prediction network's RNN hidden states.
struct DecoderState {
    /// Decoder hidden output [B, 640, 1] (float)
    output: ArrayD<f32>,
    /// RNN state: states [2, B, 640] (float)
    states: ArrayD<f32>,
    /// RNN state: the concat buffer [2, 1, 640] (float)
    concat_state: ArrayD<f32>,
}

/// Run the prediction network (decoder) with a single token.
///
/// Decoder inputs:
///   targets: [B, 1] (int32)
///   target_length: [B] (int32)
///   states.1: [2, B, 640] (float) — RNN hidden state
///   onnx::Slice_3: [2, 1, 640] (float) — concat buffer
///
/// Decoder outputs:
///   outputs: [B, 640, 1] (float) — prediction network output
///   prednet_lengths: [B] (int32)
///   states: [2, B, 640] (float) — updated RNN state
///   162: [2, concat_dim, 640] (float) — updated concat buffer
fn run_decoder(
    decoder: &Mutex<Session>,
    token_id: u32,
    prev_state: &DecoderState,
) -> Result<DecoderState, TranscriptionError> {
    let targets = ndarray::Array2::from_shape_vec((1, 1), vec![token_id as i32])
        .map_err(|e| TranscriptionError::InferenceError(format!("Shape error: {}", e)))?;
    let targets_tensor = Tensor::from_array(targets)
        .map_err(|e| TranscriptionError::InferenceError(format!("Tensor error: {}", e)))?;

    let target_length = ndarray::Array1::from(vec![1i32]);
    let target_length_tensor = Tensor::from_array(target_length)
        .map_err(|e| TranscriptionError::InferenceError(format!("Tensor error: {}", e)))?;

    let states_tensor = Tensor::from_array(prev_state.states.clone())
        .map_err(|e| TranscriptionError::InferenceError(format!("States tensor error: {}", e)))?;

    let concat_tensor = Tensor::from_array(prev_state.concat_state.clone())
        .map_err(|e| TranscriptionError::InferenceError(format!("Concat tensor error: {}", e)))?;

    let mut session = decoder.lock();
    let outputs = session
        .run(ort::inputs! {
            "targets" => targets_tensor,
            "target_length" => target_length_tensor,
            "states.1" => states_tensor,
            "onnx::Slice_3" => concat_tensor
        })
        .map_err(|e| TranscriptionError::InferenceError(format!("Decoder failed: {}", e)))?;

    let dec_output = outputs
        .get("outputs")
        .ok_or_else(|| TranscriptionError::InferenceError("Missing decoder 'outputs'".into()))?
        .try_extract_array::<f32>()
        .map(|a| a.to_owned())
        .map_err(|e| {
            TranscriptionError::InferenceError(format!("Decoder output error: {}", e))
        })?;

    let new_states = outputs
        .get("states")
        .ok_or_else(|| TranscriptionError::InferenceError("Missing decoder 'states'".into()))?
        .try_extract_array::<f32>()
        .map(|a| a.to_owned())
        .map_err(|e| {
            TranscriptionError::InferenceError(format!("Decoder states error: {}", e))
        })?;

    let new_concat = outputs
        .get("162")
        .ok_or_else(|| TranscriptionError::InferenceError("Missing decoder '162'".into()))?
        .try_extract_array::<f32>()
        .map(|a| a.to_owned())
        .map_err(|e| {
            TranscriptionError::InferenceError(format!("Decoder concat error: {}", e))
        })?;

    Ok(DecoderState {
        output: dec_output,
        states: new_states,
        concat_state: new_concat,
    })
}

/// Run joiner: combines encoder frame + decoder output to produce logits.
///
/// Joiner inputs:
///   encoder_outputs: [B, 1024, 1] (float)
///   decoder_outputs: [B, 640, 1] (float)
///
/// Joiner outputs:
///   outputs: [B, 1, 1, 8198] (float) — vocab_size + num_durations
fn run_joiner(
    joiner: &Mutex<Session>,
    enc_frame: ArrayD<f32>,
    decoder_out: &ArrayD<f32>,
) -> Result<Vec<f32>, TranscriptionError> {
    let enc_tensor = Tensor::from_array(enc_frame).map_err(|e| {
        TranscriptionError::InferenceError(format!("Encoder frame tensor error: {}", e))
    })?;
    let dec_tensor = Tensor::from_array(decoder_out.clone()).map_err(|e| {
        TranscriptionError::InferenceError(format!("Decoder output tensor error: {}", e))
    })?;

    let mut session = joiner.lock();
    let outputs = session
        .run(ort::inputs! {
            "encoder_outputs" => enc_tensor,
            "decoder_outputs" => dec_tensor
        })
        .map_err(|e| TranscriptionError::InferenceError(format!("Joiner failed: {}", e)))?;

    let logits = outputs
        .get("outputs")
        .ok_or_else(|| TranscriptionError::InferenceError("Missing joiner 'outputs'".into()))?
        .try_extract_array::<f32>()
        .map(|a| a.to_owned())
        .map_err(|e| {
            TranscriptionError::InferenceError(format!("Joiner output error: {}", e))
        })?;

    Ok(logits.iter().copied().collect())
}

pub fn greedy_decode_tdt(
    encoder_output: &ArrayD<f32>, // shape: [B, 1024, T]
    encoded_length: usize,        // actual number of valid encoder frames
    decoder: &Mutex<Session>,
    joiner: &Mutex<Session>,
    vocab_size: usize,
    blank_id: u32,
    max_steps: usize,
) -> Result<Vec<u32>, TranscriptionError> {
    let num_frames = encoded_length;
    let mut tokens: Vec<u32> = Vec::new();
    let mut t: usize = 0;

    // Initialize decoder state with zeros
    let initial_state = DecoderState {
        output: ArrayD::zeros(IxDyn(&[1, 640, 1])),
        states: ArrayD::zeros(IxDyn(&[2, 1, 640])),
        concat_state: ArrayD::zeros(IxDyn(&[2, 1, 640])),
    };

    // Run decoder with blank token to get initial state
    let mut dec_state = run_decoder(decoder, blank_id, &initial_state)?;

    let mut step_count = 0;
    while t < num_frames && step_count < max_steps {
        step_count += 1;

        // Extract single encoder frame: [B, 1024, 1]
        let enc_frame = encoder_output
            .slice(s![.., .., t..t + 1])
            .to_owned()
            .into_dyn();

        // Run joiner to get combined logits
        let logits_flat = run_joiner(joiner, enc_frame, &dec_state.output)?;

        // Split logits: first vocab_size are token logits, next 5 are duration logits
        if logits_flat.len() < vocab_size + 5 {
            return Err(TranscriptionError::InferenceError(format!(
                "Joiner output too small: {} (expected at least {})",
                logits_flat.len(),
                vocab_size + 5
            )));
        }

        let token_logits = &logits_flat[..vocab_size];
        let duration_logits = &logits_flat[vocab_size..vocab_size + 5];

        let token = argmax(token_logits) as u32;
        let duration = argmax(duration_logits);

        if token != blank_id {
            tokens.push(token);
            dec_state = run_decoder(decoder, token, &dec_state)?;
        }

        t += duration.max(1);
    }

    Ok(tokens)
}

fn argmax(slice: &[f32]) -> usize {
    slice
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(idx, _)| idx)
        .unwrap_or(0)
}
