//! The Beat This! ONNX models, embedded in the binary and run by rten (pure
//! Rust, no system libraries). See `NOTICE` for provenance and licences.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use beat_this::{Model, Tensor};
use rten::{Model as Graph, NodeId, RunOptions, ThreadPool, ValueView};
use rten_tensor::Layout;

/// Log-mel front end (~270 KB).
static MEL_MODEL: &[u8] = include_bytes!("models/mel_spectrogram.onnx");
/// The small Beat This! checkpoint ("small1", ~10.6 MB).
static BEAT_MODEL: &[u8] = include_bytes!("models/beat_this_small.onnx");

/// One loaded graph with the name maps beat-this's [`Model`] trait needs.
pub struct EmbeddedModel {
    graph: Graph,
    inputs: HashMap<String, NodeId>,
    outputs: Vec<(NodeId, String)>,
    output_ids: Vec<NodeId>,
    pool: Option<Arc<ThreadPool>>,
}

impl EmbeddedModel {
    fn load(bytes: &'static [u8], pool: Option<Arc<ThreadPool>>) -> Result<Self> {
        let graph = Graph::load_static_slice(bytes)?;
        let name = |id: NodeId| graph.node_info(id)?.name().map(str::to_string);
        let inputs = graph
            .input_ids()
            .iter()
            .filter_map(|&id| Some((name(id)?, id)))
            .collect();
        let outputs = graph
            .output_ids()
            .iter()
            .filter_map(|&id| Some((id, name(id)?)))
            .collect();
        let output_ids = graph.output_ids().to_vec();
        Ok(Self {
            graph,
            inputs,
            outputs,
            output_ids,
            pool,
        })
    }

    /// The mel and beat models. `threads == 0` uses rten's shared pool (one
    /// thread per physical core, or `RTEN_NUM_THREADS`).
    pub fn load_pair(threads: usize) -> Result<(Self, Self)> {
        let pool = (threads > 0).then(|| Arc::new(ThreadPool::with_num_threads(threads)));
        Ok((
            Self::load(MEL_MODEL, pool.clone())?,
            Self::load(BEAT_MODEL, pool)?,
        ))
    }
}

impl Model for EmbeddedModel {
    fn run(&mut self, inputs: &[(&str, &Tensor)]) -> Result<HashMap<String, Tensor>> {
        let views = inputs
            .iter()
            .map(|(name, t)| {
                let id = self
                    .inputs
                    .get(*name)
                    .ok_or_else(|| anyhow!("rten: unknown input '{name}'"))?;
                let view = ValueView::from_shape(t.shape.as_slice(), t.data.as_slice())
                    .map_err(|e| anyhow!("rten: input '{name}': {e}"))?;
                Ok((*id, view.into()))
            })
            .collect::<Result<Vec<_>>>()?;
        let opts = self
            .pool
            .as_ref()
            .map(|p| RunOptions::default().with_thread_pool(Some(p.clone())));
        let values = self.graph.run(views, &self.output_ids, opts)?;
        let mut out = HashMap::new();
        for (&id, value) in self.output_ids.iter().zip(values) {
            let name = self
                .outputs
                .iter()
                .find(|(n, _)| *n == id)
                .map(|(_, s)| s.clone())
                .unwrap_or_else(|| format!("output_{id:?}"));
            let t = value
                .into_tensor::<f32>()
                .ok_or_else(|| anyhow!("rten: output '{name}' is not f32"))?;
            let shape = t.shape().to_vec();
            out.insert(
                name,
                Tensor {
                    shape,
                    data: t.into_data(),
                },
            );
        }
        Ok(out)
    }
}
