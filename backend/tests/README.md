# Backend Tests

Chinese version: [README.zh.md](README.zh.md)

This directory keeps backend test helpers grouped by purpose:

- `smoke/`: smoke tests that call a running NeoGate instance.
- `benchmarks/`: local benchmark tools and mock upstreams.
- `fixtures/`: shared input assets used by tests.
- `output/`: generated test artifacts. This directory is ignored by git.

## OpenAI Image Smoke Tests

The OpenAI image smoke tests live in `smoke/test_openai_image.py`. They read
defaults from `backend/.env`; environment variables override values from that
file.

Required:

```bash
NEOGATE_API_KEY=your_neogate_api_key
```

Optional:

```bash
NEOGATE_BASE_URL=http://127.0.0.1:8080/v1
NEOGATE_IMAGE_SIZE=1536x1024
```

The image model is fixed to `gpt-image-2`, and generated images are saved under
`tests/output/openai_image/`. Request and response JSON snapshots are saved in
the same directory. Multipart request snapshots include form fields plus the
uploaded file name, content type, and byte size.

`fixtures/test2.jpg` is used by the dog cutout tests. The synchronous test
calls `/images/edits`; the asynchronous test uses a background Responses task
with the `image_generation` tool set to `action: "edit"`. Both request a
transparent PNG result at `1024x1536`.

Run all image smoke tests from `backend/`:

```bash
python -m unittest tests.smoke.test_openai_image
```

Run one test from `backend/`:

```bash
python -m unittest tests.smoke.test_openai_image.test_images_generation_json
python -m unittest tests.smoke.test_openai_image.test_images_edit_multipart
python -m unittest tests.smoke.test_openai_image.test_images_edit_extract_dog
python -m unittest tests.smoke.test_openai_image.test_images_generation_stream
python -m unittest tests.smoke.test_openai_image.test_images_edit_json_stream
python -m unittest tests.smoke.test_openai_image.test_images_variation
python -m unittest tests.smoke.test_openai_image.test_responses_image_generation_background
python -m unittest tests.smoke.test_openai_image.test_responses_image_edit_extract_dog_background
python -m unittest tests.smoke.test_openai_image.test_responses_image_edit_three_2k_background
```

## OpenAI Video Smoke Test

`smoke/test_openai_video.py` creates a task, polls it, and downloads the
video. It calls a real provider and incurs charges. The required
`NEOGATE_API_KEY` is a NeoGate user key, not an upstream key.

For GlobalAI OPC, confirm that the channel provider is `openai`, the endpoint
host is `apillm.globalaiopc.com`, and `sd_2.0_fast_discount` or
`sd_2.0_discount` is configured with pricing. Run from the repository root:

```bash
NEOGATE_API_KEY='your NeoGate user API key' \
NEOGATE_BASE_URL='http://127.0.0.1:8080/v1' \
NEOGATE_VIDEO_MODEL='sd_2.0_fast_discount' \
NEOGATE_VIDEO_SIZE='1280x720' \
NEOGATE_VIDEO_RESOLUTION='720p' \
NEOGATE_VIDEO_RATIO='16:9' \
NEOGATE_VIDEO_SECONDS='5' \
NEOGATE_VIDEO_PROMPT='A cinematic tracking shot of a futuristic maglev train crossing a rainy neon city at night.' \
python3 -m unittest -v backend.tests.smoke.test_openai_video
```

GlobalAI OPC limits the fast model to `480p/720p`, the regular model to
`480p/720p/1080p`, and duration to 4-15 seconds. Environment variables
override `backend/.env`; output is saved under `tests/output/openai_video/`.

The two-asset reference test first creates and polls two image assets through
`/v1/assets`, then creates a video using `asset://asset_*` references:

```bash
NEOGATE_API_KEY='your NeoGate user API key' \
NEOGATE_VIDEO_MODEL='sd_2.0_discount' \
NEOGATE_VIDEO_SIZE='1280x720' \
NEOGATE_VIDEO_RESOLUTION='720p' \
NEOGATE_VIDEO_SECONDS='5' \
python3 -m unittest -v \
  backend.tests.smoke.test_openai_video.test_videos_create_with_two_asset_references
```

The source image URLs can be overridden with `NEOGATE_ASSET_IMAGE_URL_1` and
`NEOGATE_ASSET_IMAGE_URL_2`. Asset polling waits up to 300 seconds by default;
override it with `NEOGATE_ASSET_POLL_TIMEOUT_SECONDS`. Asset creation is not
billed; video generation calls a real upstream and incurs charges.

Every API and CDN request/response is saved as separate `*_request.json` and
`*_response.json` files under `tests/output/openai_video/`. Authorization is
omitted; binary video responses record headers and byte size instead of body.

## OpenAI Audio Transcription Smoke Test

The audio transcription smoke test lives in `smoke/test_openai_audio.py`. It
uploads audio to a running NeoGate instance and verifies that the
OpenAI-compatible JSON and text responses contain non-empty transcription
text. The test does not save or print the transcript.

The test uses `fixtures/audio.wav` by default. Required:

```bash
NEOGATE_API_KEY=your_neogate_api_key
```

Set `NEOGATE_AUDIO_FILE` to test another recording. Optional:

```bash
NEOGATE_BASE_URL=http://127.0.0.1:8080/v1
NEOGATE_AUDIO_FILE=/path/to/audio.mp3
NEOGATE_AUDIO_MODEL=fun-asr-flash-2026-06-15
NEOGATE_AUDIO_EXPECTED_TEXT=expected phrase
```

Run from `backend/`:

```bash
python -m unittest tests.smoke.test_openai_audio
```

## Relay Benchmark

The relay benchmark tools provide a local OpenAI-compatible mock upstream for
measuring NeoGate relay overhead without calling a real model provider.

Start the mock upstream from `backend/`:

```bash
python3 tests/benchmarks/relay_bench_mock.py --quiet
```

The default upstream base URL is:

```text
http://127.0.0.1:18080
```

For payload-size experiments:

```bash
python3 tests/benchmarks/relay_bench_mock.py --quiet --output-bytes 4096
```

For upstream-latency experiments:

```bash
python3 tests/benchmarks/relay_bench_mock.py --quiet --delay-ms 20
```

Configure a test channel in NeoGate:

- protocol: `openai`
- base URL: `http://127.0.0.1:18080`
- model: `bench-model`
- key: any non-empty value, for example `bench-key`

Create a NeoGate user API key with permission for `bench-model`.

Run a non-streaming benchmark:

```bash
export NEOGATE_API_KEY='your-neogate-key'

wrk -t4 -c128 -d30s --latency \
  -H "authorization: Bearer ${NEOGATE_API_KEY}" \
  -H "content-type: application/json" \
  -s <(cat <<'LUA'
wrk.method = "POST"
wrk.body = '{"model":"bench-model","messages":[{"role":"user","content":"ping"}],"max_tokens":16}'
LUA
) \
  http://127.0.0.1:8080/v1/chat/completions
```

Or use the wrapper script, which can also sample NeoGate RSS/CPU when
`NEOGATE_PID` is set:

```bash
NEOGATE_API_KEY='your-neogate-key' \
NEOGATE_PID="$(pgrep -n neogate)" \
python3 tests/benchmarks/relay_bench.py --duration 30s --connections 128 --threads 4
```

Run a streaming benchmark:

```bash
wrk -t4 -c128 -d30s --latency \
  -H "authorization: Bearer ${NEOGATE_API_KEY}" \
  -H "content-type: application/json" \
  -s <(cat <<'LUA'
wrk.method = "POST"
wrk.body = '{"model":"bench-model","messages":[{"role":"user","content":"ping"}],"stream":true,"max_tokens":16}'
LUA
) \
  http://127.0.0.1:8080/v1/chat/completions
```

Wrapper equivalent:

```bash
NEOGATE_API_KEY='your-neogate-key' \
NEOGATE_PID="$(pgrep -n neogate)" \
python3 tests/benchmarks/relay_bench.py --stream --duration 30s --connections 128 --threads 4
```

Track at least:

- requests/sec
- transfer/sec
- p50/p95/p99 latency
- NeoGate RSS
- NeoGate CPU
- database CPU and connection saturation
