"""Convert musicnn's MTT_musicnn checkpoint to the ONNX graph hocket-core embeds
(crates/hocket-core/src/automix/analysis/models/musicnn_mtt.onnx), and write the
reference data the `parity_with_reference` test checks the Rust front end against.

The graph is the network only: log-mel patches [N, 187, 96] in, the 50
MagnaTagATune tag probabilities out (input `patches:0`, output `tags:0`), built
with is_training=False so batch norm and dropout are in inference mode.

Run it in a throwaway container (TensorFlow 2.15 still has the tf.compat.v1 layers
musicnn uses):

    git clone https://github.com/jordipons/musicnn /tmp/musicnn   # 516acb2a0ff5
    mkdir -p /tmp/musicnn-out
    docker run --rm -v /tmp/musicnn:/m:ro -v "$PWD/scripts":/s:ro -v /tmp/musicnn-out:/out \
        python:3.11-slim bash -c "pip install -q tensorflow-cpu==2.15.1 tf_keras==2.15.1 \
        tf2onnx==1.16.1 'numpy<2' onnxruntime librosa==0.10.2 && python /s/musicnn-to-onnx.py"

Then copy /tmp/musicnn-out/musicnn_mtt.onnx into models/ and run the parity test with
HOCKET_MUSICNN_REF=/tmp/musicnn-out.
"""
import sys

import librosa
import numpy as np
import tensorflow as tf

sys.path.insert(0, "/m")
from musicnn import configuration as config  # noqa: E402
from musicnn import models  # noqa: E402

tf.compat.v1.disable_eager_execution()
N_FRAMES = 187  # 3 s at 16 kHz, hop 256 (musicnn's extractor)

g = tf.Graph()
with g.as_default():
    x = tf.compat.v1.placeholder(tf.float32, [None, N_FRAMES, config.N_MELS], name="patches")
    with tf.compat.v1.name_scope("model"):
        y, *_ = models.define_model(x, False, "MTT_musicnn", 50)
    tags = tf.identity(tf.nn.sigmoid(y), name="tags")
    sess = tf.compat.v1.Session(graph=g)
    sess.run(tf.compat.v1.global_variables_initializer())
    tf.compat.v1.train.Saver().restore(sess, "/m/musicnn/MTT_musicnn/")
    frozen = tf.compat.v1.graph_util.convert_variables_to_constants(
        sess, g.as_graph_def(), ["tags"]
    )

# Reference: an 8 s test signal through librosa's front end (as musicnn's extractor) and TF.
rng = np.random.default_rng(1)
t = np.arange(16000 * 8) / 16000
wave = (
    0.3 * np.sin(2 * np.pi * 220 * t * (1 + 0.05 * np.sin(2 * np.pi * 5 * t)))
    + 0.05 * rng.standard_normal(t.size)
).astype(np.float32)
mel = librosa.feature.melspectrogram(y=wave, sr=16000, hop_length=256, n_fft=512, n_mels=96).T
logmel = np.log10(10000 * mel + 1).astype(np.float32)
patches = np.stack([logmel[s : s + N_FRAMES] for s in range(0, logmel.shape[0] - N_FRAMES + 1, 62)])
ref = sess.run(tags, {x: patches})
np.save("/out/ref_wave.npy", wave)
np.save("/out/ref_logmel.npy", logmel)
np.save("/out/ref_tags.npy", ref.astype(np.float32))
np.save("/out/ref_melbasis.npy", librosa.filters.mel(sr=16000, n_fft=512, n_mels=96).astype(np.float32))

import onnxruntime as ort  # noqa: E402
import tf2onnx  # noqa: E402

tf2onnx.convert.from_graph_def(
    frozen,
    input_names=["patches:0"],
    output_names=["tags:0"],
    opset=13,
    output_path="/out/musicnn_mtt.onnx",
)
s = ort.InferenceSession("/out/musicnn_mtt.onnx")
o = s.run(None, {"patches:0": patches})[0]
print("onnx vs tf max diff", float(np.abs(o - ref).max()))
