import http.client
import json
import os
import time
import unittest
from pathlib import Path
from urllib.parse import urlparse
from urllib.request import Request, urlopen


TESTS_DIR = Path(__file__).resolve().parents[1]
BACKEND_DIR = TESTS_DIR.parent
ENV_FILE = BACKEND_DIR / ".env"
DEFAULT_BASE_URL = "http://127.0.0.1:8080/v1"
DEFAULT_VIDEO_MODEL = "dreamina-seedance-2-0-260128"
DEFAULT_VIDEO_SIZE = "854x480"
DEFAULT_VIDEO_SECONDS = 4
REQUEST_TIMEOUT_SECONDS = 600
VIDEO_POLL_TIMEOUT_SECONDS = 1800
VIDEO_POLL_INTERVAL_SECONDS = 10
ASSET_POLL_TIMEOUT_SECONDS = int(
    os.environ.get("NEOGATE_ASSET_POLL_TIMEOUT_SECONDS") or 300
)
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
VIDEO_RATIO = env_value("NEOGATE_VIDEO_RATIO")
VIDEO_RESOLUTION = env_value("NEOGATE_VIDEO_RESOLUTION")
VIDEO_PROMPT = (
    env_value("NEOGATE_VIDEO_PROMPT")
    or "A short calm shot of a glass teapot on a walnut table, soft morning light."
)
ASSET_IMAGE_URL_1 = env_value("NEOGATE_ASSET_IMAGE_URL_1") or (
    "https://images.unsplash.com/photo-1517841905240-472988babdf9?w=512"
)
ASSET_IMAGE_URL_2 = env_value("NEOGATE_ASSET_IMAGE_URL_2") or (
    "https://images.unsplash.com/photo-1535713875002-d1d0cf377fde?w=512"
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
        self.request_index = 0

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
            request_value=payload,
            expect_json=True,
        )

    def get_json(self, path):
        status, headers, body = self.request(
            "GET",
            path,
            None,
            {"Authorization": f"Bearer {self.api_key}"},
            expect_json=True,
        )
        return status, headers, parse_json_body(body)

    def get_bytes(self, path):
        return self.request(
            "GET",
            path,
            None,
            {"Authorization": f"Bearer {self.api_key}"},
        )

    def request(
        self, method, path, body, headers, request_value=None, expect_json=False
    ):
        self.request_index += 1
        log_name = request_log_name(self.request_index, method, path)
        safe_headers = {
            key: value
            for key, value in headers.items()
            if key.lower() != "authorization"
        }
        save_json(
            f"{log_name}_request",
            {
                "method": method,
                "url": (
                    f"{self.parsed.scheme}://{self.parsed.netloc}"
                    f"{self.base_path}{path}"
                ),
                "headers": safe_headers,
                "body": request_value,
            },
        )
        conn = make_connection(self.parsed)
        request_path = f"{self.base_path}{path}"
        try:
            conn.request(method, request_path, body=body, headers=headers)
            response = conn.getresponse()
            response_body = response.read()
            response_headers = {key.lower(): value for key, value in response.getheaders()}
            response_value = response_body_value(response_body, expect_json)
            save_json(
                f"{log_name}_response",
                {
                    "status": response.status,
                    "headers": response_headers,
                    "body": response_value,
                    "body_bytes": len(response_body),
                },
            )
            return response.status, response_headers, response_body
        except Exception as exc:
            save_json(
                f"{log_name}_response",
                {"error": {"type": type(exc).__name__, "message": str(exc)}},
            )
            raise
        finally:
            conn.close()


def parse_json_body(body):
    try:
        return json.loads(body.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        preview = body[:500].decode("utf-8", errors="replace")
        raise AssertionError(f"response body is not JSON: {preview}") from exc


def request_log_name(index, method, path):
    safe_path = path.strip("/").replace("/", "_").replace(":", "_") or "root"
    return f"http_{index:03d}_{method.lower()}_{safe_path}"


def response_body_value(body, expect_json):
    if not expect_json:
        return None
    try:
        return json.loads(body.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError):
        return body[:2000].decode("utf-8", errors="replace")


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
    if VIDEO_MODEL.startswith("dreamina-seedance-") or VIDEO_MODEL in {
        "sd_2.0_discount",
        "sd_2.0_fast_discount",
    }:
        payload.update(
            {
                "ratio": VIDEO_RATIO or "16:9",
                "resolution": VIDEO_RESOLUTION or "480p",
            }
        )
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


def save_video_url(name, url):
    request_headers = {"User-Agent": "NeoGate video smoke test"}
    save_json(
        f"{name}_cdn_request",
        {"method": "GET", "url": url, "headers": request_headers, "body": None},
    )
    request = Request(url, headers=request_headers)
    try:
        with urlopen(request, timeout=REQUEST_TIMEOUT_SECONDS) as response:
            headers = {key.lower(): value for key, value in response.headers.items()}
            body = response.read()
            save_json(
                f"{name}_cdn_response",
                {
                    "status": response.status,
                    "headers": headers,
                    "body": None,
                    "body_bytes": len(body),
                },
            )
    except Exception as exc:
        save_json(
            f"{name}_cdn_response",
            {"error": {"type": type(exc).__name__, "message": str(exc)}},
        )
        raise
    return save_video(name, headers, body)


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
    status = nested_value(value, ("status",))
    if not isinstance(status, str):
        status = nested_value(value, ("task_status",))
    if not isinstance(status, str):
        status = nested_value(value, ("output", "status"))
    if not isinstance(status, str):
        status = nested_value(value, ("output", "task_status"))
    if isinstance(status, str) and status:
        return status.lower()
    return ""


def asset_status(value):
    status = value.get("status") if isinstance(value, dict) else None
    return status.lower() if isinstance(status, str) else ""


def video_id(value):
    for path in (
        ("id",),
        ("task_id",),
        ("output", "id"),
        ("output", "task_id"),
    ):
        current = nested_value(value, path)
        if isinstance(current, str) and current:
            return current
    return ""


def nested_value(value, path):
    current = value
    for key in path:
        if not isinstance(current, dict):
            return None
        current = current.get(key)
    return current


def video_urls(value):
    urls = []
    if isinstance(value, dict):
        for key, item in value.items():
            lower_key = key.lower()
            if isinstance(item, str) and lower_key in {"video_url", "url"}:
                if item.startswith("http://") or item.startswith("https://"):
                    urls.append(item)
            else:
                urls.extend(video_urls(item))
    elif isinstance(value, list):
        for item in value:
            urls.extend(video_urls(item))
    return urls


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


def create_image_asset(api, source_url, name):
    status, _headers, body = api.post_json(
        "/assets",
        {
            "model": VIDEO_MODEL,
            "type": "image",
            "url": source_url,
            "name": name,
        },
    )
    assert_success(status, body)
    value = parse_json_body(body)
    save_json(f"{name}_create", value)
    asset_id = value.get("id") if isinstance(value, dict) else None
    if not isinstance(asset_id, str) or not asset_id.startswith("asset_"):
        raise AssertionError(f"asset id is missing: {value}")
    return asset_id, value


def poll_asset_until_active(api, asset_id, initial_value):
    value = initial_value
    started = time.monotonic()
    deadline = time.monotonic() + ASSET_POLL_TIMEOUT_SECONDS
    terminal_statuses = {"active", "failed", "deleted", "expired"}
    print(f"asset {asset_id}: {asset_status(value)} (0s)", flush=True)
    while asset_status(value) not in terminal_statuses:
        if time.monotonic() >= deadline:
            raise AssertionError(
                f"asset {asset_id} stayed {asset_status(value)} for "
                f"{ASSET_POLL_TIMEOUT_SECONDS}s: {value}"
            )
        time.sleep(VIDEO_POLL_INTERVAL_SECONDS)
        status, _headers, value = api.get_json(f"/assets/{asset_id}")
        if not 200 <= status < 300:
            raise AssertionError(f"failed to poll asset {asset_id}: HTTP {status} {value}")
        elapsed = int(time.monotonic() - started)
        print(f"asset {asset_id}: {asset_status(value)} ({elapsed}s)", flush=True)
    save_json(f"{asset_id}_final", value)
    if asset_status(value) != "active":
        raise AssertionError(f"asset {asset_id} is not active: {value}")
    return value


def _test_videos_create_poll_and_download():
    api = client()
    status, _headers, body = api.post_json("/videos", video_request_payload())
    assert_success(status, body)
    value = parse_json_body(body)
    save_json("videos_create", value)

    video_id_value = video_id(value)
    if not video_id_value:
        raise AssertionError(f"video id is missing: {value}")

    final_value = poll_video_until_terminal(api, video_id_value, value)
    save_json("videos_final", final_value)
    if video_status(final_value) not in SUCCESS_STATUSES:
        raise AssertionError(final_value)

    urls = video_urls(final_value)
    if urls:
        save_video_url("videos_content", urls[0])
        return

    status, headers, content = api.get_bytes(f"/videos/{video_id_value}/content")
    assert_success(status, content)
    save_video("videos_content", headers, content)


def _test_videos_create_with_two_asset_references():
    if VIDEO_MODEL not in {"sd_2.0_discount", "sd_2.0_fast_discount"}:
        raise unittest.SkipTest(
            "set NEOGATE_VIDEO_MODEL to a GlobalAI OPC discount model for asset references"
        )

    api = client()
    asset_ids = []
    for source_url, name in [
        (ASSET_IMAGE_URL_1, "video_reference_1"),
        (ASSET_IMAGE_URL_2, "video_reference_2"),
    ]:
        asset_id, initial_value = create_image_asset(api, source_url, name)
        poll_asset_until_active(api, asset_id, initial_value)
        asset_ids.append(asset_id)

    payload = video_request_payload()
    payload["prompt"] = (
        "Create a short transition that combines the subjects and visual style "
        "from both reference images."
    )
    payload["content"] = [
        {
            "type": "image_url",
            "role": "reference_image",
            "image_url": {"url": f"asset://{asset_ids[0]}"},
        },
        {
            "type": "image_url",
            "role": "reference_image",
            "image_url": {"url": f"asset://{asset_ids[1]}"},
        },
    ]

    status, _headers, body = api.post_json("/videos", payload)
    assert_success(status, body)
    value = parse_json_body(body)
    save_json("videos_two_asset_refs_create", value)

    video_id_value = video_id(value)
    if not video_id_value:
        raise AssertionError(f"video id is missing: {value}")
    final_value = poll_video_until_terminal(api, video_id_value, value)
    save_json("videos_two_asset_refs_final", final_value)
    if video_status(final_value) not in SUCCESS_STATUSES:
        raise AssertionError(final_value)

    urls = video_urls(final_value)
    if urls:
        save_video_url("videos_two_asset_refs_content", urls[0])
        return
    status, headers, content = api.get_bytes(f"/videos/{video_id_value}/content")
    assert_success(status, content)
    save_video("videos_two_asset_refs_content", headers, content)


def make_test_case(test_func):
    return unittest.FunctionTestCase(test_func, description=test_func.__name__.removeprefix("_"))


def test_videos_create_poll_and_download():
    return make_test_case(_test_videos_create_poll_and_download)


def test_videos_create_with_two_asset_references():
    return make_test_case(_test_videos_create_with_two_asset_references)


def load_tests(loader, tests, pattern):
    suite = unittest.TestSuite()
    suite.addTest(test_videos_create_poll_and_download())
    suite.addTest(test_videos_create_with_two_asset_references())
    return suite


if __name__ == "__main__":
    unittest.main()
