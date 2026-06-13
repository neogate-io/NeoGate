# Relay Benchmark

This directory contains a tiny OpenAI-compatible mock upstream for measuring
NeoGate relay overhead without calling a real model provider.

## Start the mock upstream

```bash
cd backend
python3 tests/relay_bench_mock.py --quiet
```

The default upstream base URL is:

```text
http://127.0.0.1:18080
```

For payload-size experiments:

```bash
python3 tests/relay_bench_mock.py --quiet --output-bytes 4096
```

For upstream-latency experiments:

```bash
python3 tests/relay_bench_mock.py --quiet --delay-ms 20
```

## Configure NeoGate

Create or update a test channel in NeoGate:

- protocol: `openai`
- base URL: `http://127.0.0.1:18080`
- model: `bench-model`
- key: any non-empty value, for example `bench-key`

Create a NeoGate user API key with permission for `bench-model`.

## Run a non-streaming benchmark

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
python3 tests/relay_bench.py --duration 30s --connections 128 --threads 4
```

## Run a streaming benchmark

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
python3 tests/relay_bench.py --stream --duration 30s --connections 128 --threads 4
```

Track at least:

- requests/sec
- transfer/sec
- p50/p95/p99 latency
- NeoGate RSS
- NeoGate CPU
- database CPU and connection saturation
