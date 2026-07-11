import http.client
import json
import os
import time
import unittest
from pathlib import Path
from urllib.parse import urlparse


TESTS_DIR = Path(__file__).resolve().parents[1]
BACKEND_DIR = TESTS_DIR.parent
ENV_FILE = BACKEND_DIR / ".env"
DEFAULT_BASE_URL = "http://127.0.0.1:8080/v1"
DEFAULT_VIDEO_MODEL = "sora-2"
DEFAULT_VIDEO_SIZE = "1280x720"
DEFAULT_VIDEO_SECONDS = 4
REQUEST_TIMEOUT_SECONDS = 600
VIDEO_POLL_TIMEOUT_SECONDS = 1800
VIDEO_POLL_INTERVAL_SECONDS = 10
OUTPUT_DIR = TESTS_DIR / "output" / "openai_video"
SUCCESS_STATUSES = {"completed", "succeeded", "success"}
TERMINAL_STATUSES = SUCCESS_STATUSES | {"failed", "cancelled", "canceled", "expired"}


def load_env_file(path):
    values = {}
    if not path.exists():
        return values

    for raw_line in path.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        key = key.strip()
        value = value.strip()
        if not key:
            continue
        if len(value) >= 2 and value[0] == value[-1] and value[0] in {"'", '"'}:
            value = value[1:-1]
        values[key] = value
    return values


ENV = load_env_file(ENV_FILE)


def env_value(name):
    return (os.environ.get(name) or ENV.get(name) or "").strip()


def normalized_base_url():
    explicit = env_value("NEOGATE_BASE_URL")
    if explicit:
        return explicit.rstrip("/")

    public_base_url = env_value("PUBLIC_BASE_URL")
    if public_base_url:
        return f"{public_base_url.rstrip('/')}/v1"

    return DEFAULT_BASE_URL


BASE_URL = normalized_base_url()
API_KEY = env_value("NEOGATE_API_KEY")
VIDEO_MODEL = env_value("NEOGATE_VIDEO_MODEL") or DEFAULT_VIDEO_MODEL
VIDEO_SIZE = env_value("NEOGATE_VIDEO_SIZE") or DEFAULT_VIDEO_SIZE
VIDEO_SECONDS = int(env_value("NEOGATE_VIDEO_SECONDS") or DEFAULT_VIDEO_SECONDS)
VIDEO_PROMPT = (
    env_value("NEOGATE_VIDEO_PROMPT")
    or "A calm five second shot of a glass teapot on a walnut table, soft morning light."
)


def require_api_key():
    if not API_KEY:
        raise unittest.SkipTest(
            f"NEOGATE_API_KEY is not set in the environment or {ENV_FILE}"
        )


def make_connection(parsed_url):
    if parsed_url.scheme == "https":
        return http.client.HTTPSConnection(
            parsed_url.hostname,
            parsed_url.port,
            timeout=REQUEST_TIMEOUT_SECONDS,
        )
    if parsed_url.scheme == "http":
        return http.client.HTTPConnection(
            parsed_url.hostname,
            parsed_url.port,
            timeout=REQUEST_TIMEOUT_SECONDS,
        )
    raise ValueError(f"unsupported URL scheme: {parsed_url.scheme}")


class NeoGateClient:
    def __init__(self, base_url, api_key):
        parsed = urlparse(base_url)
        if not parsed.scheme or not parsed.netloc:
            raise ValueError(f"invalid NEOGATE_BASE_URL: {base_url}")
        self.parsed = parsed
        self.base_path = parsed.path.rstrip("/")
        self.api_key = api_key

    def post_json(self, path, payload):
        body = json.dumps(payload).encode("utf-8")
        return self.request(
            "POST",
            path,
            body,
            {
                "Authorization": f"Bearer {self.api_key}",
                "Content-Type": "application/json",
            },
        )

    def get_json(self, path):
        status, headers, body = self.request(
            "GET",
            path,
            None,
            {"Authorization": f"Bearer {self.api_key}"},
        )
        return status, headers, parse_json_body(body)

    def get_bytes(self, path):
        return self.request(
            "GET",
            path,
            None,
            {"Authorization": f"Bearer {self.api_key}"},
        )

    def request(self, method, path, body, headers):
        conn = make_connection(self.parsed)
        request_path = f"{self.base_path}{path}"
        try:
            conn.request(method, request_path, body=body, headers=headers)
            response = conn.getresponse()
            response_body = response.read()
            response_headers = {key.lower(): value for key, value in response.getheaders()}
            return response.status, response_headers, response_body
        finally:
            conn.close()


def parse_json_body(body):
    try:
        return json.loads(body.decode("utf-8"))
    except json.JSONDecodeError as exc:
        preview = body[:500].decode("utf-8", errors="replace")
        raise AssertionError(f"response body is not JSON: {preview}") from exc


def assert_success(status, body):
    if 200 <= status < 300:
        return
    if isinstance(body, bytes):
        preview = body[:1000].decode("utf-8", errors="replace")
    else:
        preview = json.dumps(body, ensure_ascii=False)[:1000]
    raise AssertionError(f"expected HTTP 2xx, got {status}: {preview}")


def video_request_payload():
    payload = {
        "model": VIDEO_MODEL,
        "prompt": VIDEO_PROMPT,
        "size": VIDEO_SIZE,
        "seconds": VIDEO_SECONDS,
    }
    extra = env_value("NEOGATE_VIDEO_EXTRA_JSON")
    if extra:
        value = json.loads(extra)
        if not isinstance(value, dict):
            raise AssertionError("NEOGATE_VIDEO_EXTRA_JSON must be a JSON object")
        payload.update(value)
    return payload


def client():
    require_api_key()
    return NeoGateClient(BASE_URL, API_KEY)


def save_json(name, value):
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    path = unique_output_path(name, ".json")
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"saved json: {path}")
    return path


def save_video(name, headers, body):
    if not body:
        raise AssertionError("video content response was empty")
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    extension = video_extension(headers.get("content-type", ""))
    path = unique_output_path(name, extension)
    path.write_bytes(body)
    print(f"saved video: {path}")
    return path


def unique_output_path(prefix, extension):
    suffix = time.strftime("%H%M%S")
    path = OUTPUT_DIR / f"{prefix}_{suffix}{extension}"
    if not path.exists():
        return path
    index = 2
    while True:
        path = OUTPUT_DIR / f"{prefix}_{suffix}_{index}{extension}"
        if not path.exists():
            return path
        index += 1


def video_extension(content_type):
    lower = content_type.lower()
    if "webm" in lower:
        return ".webm"
    if "quicktime" in lower or "mov" in lower:
        return ".mov"
    if "mpegurl" in lower or "m3u8" in lower:
        return ".m3u8"
    return ".mp4"


def video_status(value):
    status = value.get("status")
    if isinstance(status, str) and status:
        return status.lower()
    return ""


def poll_video_until_terminal(api, video_id, initial_value):
    value = initial_value
    deadline = time.monotonic() + VIDEO_POLL_TIMEOUT_SECONDS
    while video_status(value) not in TERMINAL_STATUSES:
        if time.monotonic() >= deadline:
            raise AssertionError(f"video {video_id} did not finish before timeout: {value}")
        time.sleep(VIDEO_POLL_INTERVAL_SECONDS)
        status, _headers, value = api.get_json(f"/videos/{video_id}")
        if not 200 <= status < 300:
            raise AssertionError(f"failed to poll video {video_id}: HTTP {status} {value}")
    return value


def _test_videos_create_poll_and_download():
    api = client()
    status, _headers, body = api.post_json("/videos", video_request_payload())
    assert_success(status, body)
    value = parse_json_body(body)
    save_json("videos_create", value)

    video_id = value.get("id")
    if not isinstance(video_id, str) or not video_id:
        raise AssertionError(f"video id is missing: {value}")

    final_value = poll_video_until_terminal(api, video_id, value)
    save_json("videos_final", final_value)
    if video_status(final_value) not in SUCCESS_STATUSES:
        raise AssertionError(final_value)

    status, headers, content = api.get_bytes(f"/videos/{video_id}/content")
    assert_success(status, content)
    save_video("videos_content", headers, content)


def make_test_case(test_func):
    return unittest.FunctionTestCase(test_func, description=test_func.__name__.removeprefix("_"))


def test_videos_create_poll_and_download():
    return make_test_case(_test_videos_create_poll_and_download)


def load_tests(loader, tests, pattern):
    suite = unittest.TestSuite()
    suite.addTest(test_videos_create_poll_and_download())
    return suite


if __name__ == "__main__":
    unittest.main()
