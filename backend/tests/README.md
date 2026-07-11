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
`tests/output/openai_image/`.

Run all image smoke tests from `backend/`:

```bash
python -m unittest tests.smoke.test_openai_image
```

Run one test from `backend/`:

```bash
python -m unittest tests.smoke.test_openai_image.test_images_generation_json
python -m unittest tests.smoke.test_openai_image.test_images_edit_multipart
python -m unittest tests.smoke.test_openai_image.test_images_generation_stream
python -m unittest tests.smoke.test_openai_image.test_images_edit_json_stream
python -m unittest tests.smoke.test_openai_image.test_images_variation
python -m unittest tests.smoke.test_openai_image.test_responses_image_generation_background
```

## OpenAI Video Smoke Test

The OpenAI-compatible video smoke test lives in `smoke/test_openai_video.py`.
It creates a video task, polls `GET /v1/videos/{id}` until the task is
terminal, then downloads `GET /v1/videos/{id}/content` when the task succeeds.

Required:

```bash
NEOGATE_API_KEY=your_neogate_api_key
```

Optional:

```bash
NEOGATE_BASE_URL=http://127.0.0.1:8080/v1
NEOGATE_VIDEO_MODEL=sora-2
NEOGATE_VIDEO_SIZE=1280x720
NEOGATE_VIDEO_SECONDS=4
NEOGATE_VIDEO_PROMPT='A calm five second shot of a glass teapot on a walnut table.'
NEOGATE_VIDEO_EXTRA_JSON='{"resolution":"720p","ratio":"16:9"}'
```

Generated JSON snapshots and video content are saved under
`tests/output/openai_video/`.

Run the video smoke test from `backend/`:

```bash
python -m unittest tests.smoke.test_openai_video
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
