"""Run a local streaming recognizer over a WAV file and report speed and accuracy.

This is the offline candidate for dictation: sherpa-onnx with a streaming zipformer, fed exactly the
way a live session feeds it (100 ms blocks). It prints how fast the model consumes audio, how far
into the audio the first words appear, and the transcript, so it can be compared with the rows the
app writes to stt-latency.csv.

    pip install sherpa-onnx numpy
    python local-stt.py <model-dir> <file.wav>

<model-dir> is an extracted release from https://github.com/k2-fsa/sherpa-onnx/releases/tag/asr-models
e.g. sherpa-onnx-streaming-zipformer-en-2023-06-26 (int8 encoder, 70 MB).
"""
import sys
import time
import wave

import numpy as np
import sherpa_onnx


def main() -> int:
    if len(sys.argv) != 3:
        print(__doc__)
        return 2
    model, wav = sys.argv[1], sys.argv[2]
    rec = sherpa_onnx.OnlineRecognizer.from_transducer(
        tokens=f"{model}/tokens.txt",
        encoder=f"{model}/encoder-epoch-99-avg-1-chunk-16-left-128.int8.onnx",
        decoder=f"{model}/decoder-epoch-99-avg-1-chunk-16-left-128.int8.onnx",
        joiner=f"{model}/joiner-epoch-99-avg-1-chunk-16-left-128.int8.onnx",
        num_threads=4,
        sample_rate=16000,
        feature_dim=80,
        decoding_method="greedy_search",
    )

    with wave.open(wav, "rb") as f:
        sr = f.getframerate()
        audio = np.frombuffer(f.readframes(f.getnframes()), dtype=np.int16).astype(np.float32) / 32768.0
    seconds = len(audio) / sr

    stream = rec.create_stream()
    block = int(sr * 0.1)
    first_at = None
    shown = ""
    started = time.perf_counter()
    for i in range(0, len(audio), block):
        stream.accept_waveform(sr, audio[i : i + block])
        while rec.is_ready(stream):
            rec.decode_stream(stream)
        text = rec.get_result(stream)
        if text != shown:
            shown = text
            if first_at is None and text:
                first_at = i / sr
            print(f"  +{i / sr:5.2f}s  {text}", flush=True)
    stream.accept_waveform(sr, np.zeros(int(sr * 0.5), dtype=np.float32))
    stream.input_finished()
    while rec.is_ready(stream):
        rec.decode_stream(stream)
    final = rec.get_result(stream)
    elapsed = time.perf_counter() - started

    print(f"audio        {seconds:.2f} s")
    print(f"compute      {elapsed * 1000:.0f} ms  (real-time factor {elapsed / seconds:.2f})")
    print(f"first words  {first_at:.2f} s into the audio" if first_at is not None else "first words  never")
    print(f"final        {final!r}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
