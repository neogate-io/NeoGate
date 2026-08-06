# 后端测试

英文版本：[README.md](README.md)

本目录按用途整理后端测试和压测辅助文件：

- `smoke/`：需要连接运行中的 NeoGate 实例的冒烟测试。
- `benchmarks/`：本地压测工具和 mock 上游服务。
- `fixtures/`：测试共享的输入素材。
- `output/`：测试生成的输出文件，该目录已被 git 忽略。

## OpenAI 图片冒烟测试

OpenAI 图片冒烟测试位于 `smoke/test_openai_image.py`。测试会读取
`backend/.env` 中的默认配置；同名环境变量优先级更高。

必填：

```bash
NEOGATE_API_KEY=your_neogate_api_key
```

可选：

```bash
NEOGATE_BASE_URL=http://127.0.0.1:8080/v1
NEOGATE_IMAGE_SIZE=1536x1024
```

图片模型固定为 `gpt-image-2`，生成的图片会保存到
`tests/output/openai_image/`。请求和响应的 JSON 快照也会保存到同一目录；
multipart 请求快照会记录表单字段以及上传文件的名称、MIME 类型和字节数。

`fixtures/test2.jpg` 用于狗抠图测试。同步用例调用 `/images/edits`；异步用例
创建携带 `image_generation` 工具的 Responses 后台任务，并设置
`action: "edit"`。两个用例都会请求 `1024x1536` 的透明 PNG 结果。

在 `backend/` 目录下运行全部图片冒烟测试：

```bash
python -m unittest tests.smoke.test_openai_image
```

在 `backend/` 目录下运行单个测试：

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

## OpenAI 视频冒烟测试

`smoke/test_openai_video.py` 会创建任务、轮询结果并下载视频。测试调用真实
上游并产生费用；必填的 `NEOGATE_API_KEY` 是 NeoGate 用户 Key，不是上游 Key。

测试 GlobalAI OPC 前，确认渠道 provider 为 `openai`，endpoint 主机为
`apillm.globalaiopc.com`，并已配置 `sd_2.0_fast_discount` 或
`sd_2.0_discount` 及对应价格。在仓库根目录运行：

```bash
NEOGATE_API_KEY='你的 NeoGate 用户 API Key' \
NEOGATE_BASE_URL='http://127.0.0.1:8080/v1' \
NEOGATE_VIDEO_MODEL='sd_2.0_fast_discount' \
NEOGATE_VIDEO_SIZE='1280x720' \
NEOGATE_VIDEO_RESOLUTION='720p' \
NEOGATE_VIDEO_RATIO='16:9' \
NEOGATE_VIDEO_SECONDS='5' \
NEOGATE_VIDEO_PROMPT='A cinematic tracking shot of a futuristic maglev train crossing a rainy neon city at night.' \
python3 -m unittest -v backend.tests.smoke.test_openai_video
```

GlobalAI OPC fast 模型支持 `480p/720p`，普通模型支持
`480p/720p/1080p`，时长为 4 到 15 秒。环境变量优先于 `backend/.env`；
测试结果保存在 `tests/output/openai_video/`。

## OpenAI 音频转写冒烟测试

音频转写测试位于 `smoke/test_openai_audio.py`，会向运行中的 NeoGate 上传音频，
并验证 OpenAI 兼容 JSON 与 text 响应包含非空转写文本。测试不会保存或打印转写内容。

测试默认使用 `fixtures/audio.wav`。必填：

```bash
NEOGATE_API_KEY=your_neogate_api_key
```

测试其他音频时可通过 `NEOGATE_AUDIO_FILE` 覆盖默认夹具。可选：

```bash
NEOGATE_BASE_URL=http://127.0.0.1:8080/v1
NEOGATE_AUDIO_FILE=/path/to/audio.mp3
NEOGATE_AUDIO_MODEL=fun-asr-flash-2026-06-15
NEOGATE_AUDIO_EXPECTED_TEXT=期望出现的文本
```

在 `backend/` 目录下运行：

```bash
python -m unittest tests.smoke.test_openai_audio
```

## Relay 压测

Relay 压测工具提供一个本地 OpenAI 兼容 mock 上游服务，用于测量 NeoGate
转发开销，不需要调用真实模型供应商。

在 `backend/` 目录下启动 mock 上游：

```bash
python3 tests/benchmarks/relay_bench_mock.py --quiet
```

默认上游地址：

```text
http://127.0.0.1:18080
```

调整响应体大小：

```bash
python3 tests/benchmarks/relay_bench_mock.py --quiet --output-bytes 4096
```

模拟上游延迟：

```bash
python3 tests/benchmarks/relay_bench_mock.py --quiet --delay-ms 20
```

在 NeoGate 中配置一个测试渠道：

- protocol：`openai`
- base URL：`http://127.0.0.1:18080`
- model：`bench-model`
- key：任意非空值，例如 `bench-key`

然后创建一个拥有 `bench-model` 权限的 NeoGate 用户 API Key。

运行非流式压测：

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

也可以使用封装脚本；设置 `NEOGATE_PID` 后，脚本会同时采样 NeoGate 的
RSS 和 CPU：

```bash
NEOGATE_API_KEY='your-neogate-key' \
NEOGATE_PID="$(pgrep -n neogate)" \
python3 tests/benchmarks/relay_bench.py --duration 30s --connections 128 --threads 4
```

运行流式压测：

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

对应的封装脚本命令：

```bash
NEOGATE_API_KEY='your-neogate-key' \
NEOGATE_PID="$(pgrep -n neogate)" \
python3 tests/benchmarks/relay_bench.py --stream --duration 30s --connections 128 --threads 4
```

至少记录以下指标：

- requests/sec
- transfer/sec
- p50/p95/p99 latency
- NeoGate RSS
- NeoGate CPU
- 数据库 CPU 和连接数饱和情况
