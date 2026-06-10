import base64
import http.client
import json
import mimetypes
import os
import tempfile
import time
import unittest
import uuid
from pathlib import Path
from urllib.parse import urlparse
from urllib.request import Request, urlopen


ENV_FILE = Path(__file__).parent / "../.env"
DEFAULT_BASE_URL = "http://127.0.0.1:8080/v1"
DEFAULT_IMAGE_MODEL = "gpt-image-2"
DEFAULT_RESPONSE_MODEL = "gpt-5.5"
DEFAULT_SIZE = "1024x1024"
REQUEST_TIMEOUT_SECONDS = 600
RESPONSE_POLL_TIMEOUT_SECONDS = 900
RESPONSE_POLL_INTERVAL_SECONDS = 5
DEFAULT_OUTPUT_DIR = Path(__file__).parent / "output"

# 1x1 transparent PNG.
PNG_BASE64 = (
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAF"
    "gwJ/l1JrNwAAAABJRU5ErkJggg=="
)


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


def config(name, default=None):
    value = os.environ.get(name)
    if value is not None and value.strip():
        return value.strip()
    value = ENV.get(name)
    if value is not None and value.strip():
        return value.strip()
    return default


def normalized_base_url():
    explicit = config("NEOGATE_BASE_URL")
    if explicit:
        return explicit.rstrip("/")

    public_base_url = config("PUBLIC_BASE_URL")
    if public_base_url:
        return f"{public_base_url.rstrip('/')}/v1"

    return DEFAULT_BASE_URL


BASE_URL = normalized_base_url()
API_KEY = config("NEOGATE_API_KEY")
IMAGE_MODEL = config("NEOGATE_IMAGE_MODEL", DEFAULT_IMAGE_MODEL)
VARIATION_MODEL = config("NEOGATE_VARIATION_MODEL", IMAGE_MODEL)
RESPONSE_MODEL = config("NEOGATE_RESPONSE_MODEL", DEFAULT_RESPONSE_MODEL)
OUTPUT_DIR = Path(config("NEOGATE_IMAGE_OUTPUT_DIR", str(DEFAULT_OUTPUT_DIR)))


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

    def post_multipart(self, path, fields, files):
        body, content_type = encode_multipart_form(fields, files)
        return self.request(
            "POST",
            path,
            body,
            {
                "Authorization": f"Bearer {self.api_key}",
                "Content-Type": content_type,
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

    def stream_json(self, path, payload):
        body = json.dumps(payload).encode("utf-8")
        conn = make_connection(self.parsed)
        request_path = f"{self.base_path}{path}"
        conn.request(
            "POST",
            request_path,
            body=body,
            headers={
                "Authorization": f"Bearer {self.api_key}",
                "Content-Type": "application/json",
                "Accept": "text/event-stream",
            },
        )
        return conn, conn.getresponse()


def encode_multipart_form(fields, files):
    boundary = f"----neogate-test-{uuid.uuid4().hex}"
    chunks = []

    for name, value in fields.items():
        chunks.extend(
            [
                f"--{boundary}\r\n".encode("utf-8"),
                f'Content-Disposition: form-data; name="{name}"\r\n\r\n'.encode("utf-8"),
                str(value).encode("utf-8"),
                b"\r\n",
            ]
        )

    for name, file_path in files.items():
        path = Path(file_path)
        content_type = mimetypes.guess_type(path.name)[0] or "application/octet-stream"
        chunks.extend(
            [
                f"--{boundary}\r\n".encode("utf-8"),
                (
                    f'Content-Disposition: form-data; name="{name}"; '
                    f'filename="{path.name}"\r\n'
                ).encode("utf-8"),
                f"Content-Type: {content_type}\r\n\r\n".encode("utf-8"),
                path.read_bytes(),
                b"\r\n",
            ]
        )

    chunks.append(f"--{boundary}--\r\n".encode("utf-8"))
    return b"".join(chunks), f"multipart/form-data; boundary={boundary}"


def parse_json_body(body):
    try:
        return json.loads(body.decode("utf-8"))
    except json.JSONDecodeError as exc:
        preview = body[:500].decode("utf-8", errors="replace")
        raise AssertionError(f"response body is not JSON: {preview}") from exc


def assert_success(status, body):
    if 200 <= status < 300:
        return
    preview = body[:1000].decode("utf-8", errors="replace")
    raise AssertionError(f"expected HTTP 2xx, got {status}: {preview}")


def image_payloads_from_images_response(value):
    data = value.get("data")
    if not isinstance(data, list) or not data:
        raise AssertionError(f"response missing data: {value}")
    payloads = []
    for index, item in enumerate(data):
        if not isinstance(item, dict):
            raise AssertionError(f"response data[{index}] is not an object: {item}")
        payload = item.get("b64_json") or item.get("url")
        if payload:
            payloads.append(payload)
    if not payloads:
        raise AssertionError(f"response did not include image payloads: {value}")
    return payloads


def image_bytes_from_payload(payload):
    if not isinstance(payload, str):
        raise AssertionError(f"image payload is not a string: {payload!r}")
    if not payload:
        raise AssertionError("image payload is empty")
    if payload.startswith("http://") or payload.startswith("https://"):
        request = Request(payload, headers={"User-Agent": "NeoGate image smoke test"})
        with urlopen(request, timeout=REQUEST_TIMEOUT_SECONDS) as response:
            content_type = response.headers.get("content-type", "")
            extension = image_extension(content_type)
            return response.read(), extension
    if payload.startswith("data:"):
        metadata, payload = payload.split(",", 1)
        extension = image_extension(metadata)
    else:
        extension = ".png"
    try:
        image_bytes = base64.b64decode(payload, validate=True)
    except Exception as exc:
        raise AssertionError("image payload is not valid base64") from exc
    if not image_bytes:
        raise AssertionError("decoded image payload is empty")
    return image_bytes, extension


def assert_image_payload(payload):
    image_bytes_from_payload(payload)


def save_image_payload(test_name, payload):
    image_bytes, extension = image_bytes_from_payload(payload)
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    file_name = f"{output_name(test_name)}_{uuid.uuid4().hex[:6]}{extension}"
    path = OUTPUT_DIR / file_name
    path.write_bytes(image_bytes)
    print(f"saved image: {path}")
    return path


def save_image_payloads(test_name, payloads):
    saved = []
    for index, payload in enumerate(payloads, start=1):
        saved.append(save_image_payload(f"{test_name}-{index}", payload))
    return saved


def output_name(test_name):
    name = test_name.removeprefix("test_").replace("-", "_")
    return "_".join(part for part in name.split("_") if part)


def image_extension(content_type):
    content_type = content_type.lower()
    if "jpeg" in content_type or "jpg" in content_type:
        return ".jpg"
    if "webp" in content_type:
        return ".webp"
    if "gif" in content_type:
        return ".gif"
    return ".png"


def response_image_payloads(value):
    payloads = []
    for output in value.get("output") or []:
        if not isinstance(output, dict):
            continue
        if output.get("type") == "image_generation_call" and output.get("result"):
            payloads.append(output["result"])
        for item in output.get("content") or []:
            if not isinstance(item, dict):
                continue
            if item.get("type") in {"output_image", "image"}:
                payload = item.get("image_url") or item.get("b64_json") or item.get("result")
                if payload:
                    payloads.append(payload)
    return payloads


def collect_image_payloads(value):
    payloads = []
    if isinstance(value, dict):
        for key, item in value.items():
            lower_key = key.lower()
            if isinstance(item, str) and (
                lower_key in {"b64_json", "image_url", "result", "url"}
                or "image_b64" in lower_key
                or "partial_image" in lower_key
            ):
                payloads.append(item)
            else:
                payloads.extend(collect_image_payloads(item))
    elif isinstance(value, list):
        for item in value:
            payloads.extend(collect_image_payloads(item))
    return payloads


def write_test_png(directory):
    path = Path(directory) / "input.png"
    path.write_bytes(base64.b64decode(PNG_BASE64))
    return path


def client():
    require_api_key()
    return NeoGateClient(BASE_URL, API_KEY)


def _test_images_generation_json():
    status, _headers, body = client().post_json(
        "/images/generations",
        {
            "model": IMAGE_MODEL,
            "prompt": "A compact glass teapot on a walnut table",
            "size": DEFAULT_SIZE,
        },
    )
    assert_success(status, body)
    value = parse_json_body(body)
    save_image_payloads("test_images_generation_json", image_payloads_from_images_response(value))


def _test_images_edit_multipart():
    with tempfile.TemporaryDirectory() as directory:
        image_path = write_test_png(directory)
        status, _headers, body = client().post_multipart(
            "/images/edits",
            {
                "model": IMAGE_MODEL,
                "prompt": "Add soft morning light through the window",
                "size": DEFAULT_SIZE,
            },
            {"image": image_path},
        )

    assert_success(status, body)
    value = parse_json_body(body)
    save_image_payloads("test_images_edit_multipart", image_payloads_from_images_response(value))


def _test_images_generation_stream():
    conn, response = client().stream_json(
        "/images/generations",
        {
            "model": IMAGE_MODEL,
            "prompt": "A compact glass teapot on a walnut table",
            "size": DEFAULT_SIZE,
            "stream": True,
            "partial_images": 2,
        },
    )
    try:
        body = response.read()
    finally:
        conn.close()

    assert_success(response.status, body)
    events = parse_sse_events(body)
    if not events:
        raise AssertionError("stream response did not contain SSE events")
    event_types = {event.get("type") or event.get("event") for event in events}
    has_expected_event = any(
        event_type
        and ("partial" in event_type or "completed" in event_type or "error" in event_type)
        for event_type in event_types
    )
    if not has_expected_event:
        raise AssertionError(
            f"stream response did not include partial/completed/error events: {events[:5]}"
        )
    for index, payload in enumerate(collect_image_payloads(events), start=1):
        save_image_payload(f"test_images_generation_stream-{index}", payload)


def _test_images_variation():
    with tempfile.TemporaryDirectory() as directory:
        image_path = write_test_png(directory)
        status, _headers, body = client().post_multipart(
            "/images/variations",
            {
                "model": VARIATION_MODEL,
                "size": DEFAULT_SIZE,
            },
            {"image": image_path},
        )

    assert_success(status, body)
    value = parse_json_body(body)
    save_image_payloads("test_images_variation", image_payloads_from_images_response(value))


def _test_responses_image_generation_background():
    status, _headers, body = client().post_json(
        "/responses",
        {
            "model": RESPONSE_MODEL,
            "input": "Generate an image of a compact glass teapot on a walnut table.",
            "tools": [{"type": "image_generation"}],
            "background": True,
            "store": True,
        },
    )
    assert_success(status, body)
    value = parse_json_body(body)
    response_id = value.get("id")
    if not isinstance(response_id, str) or not response_id:
        raise AssertionError(f"response id is missing: {value}")

    terminal_statuses = {"completed", "failed", "cancelled", "canceled", "incomplete"}
    deadline = time.monotonic() + RESPONSE_POLL_TIMEOUT_SECONDS
    while value.get("status") not in terminal_statuses:
        if time.monotonic() >= deadline:
            raise AssertionError(f"response {response_id} did not finish before timeout: {value}")
        time.sleep(RESPONSE_POLL_INTERVAL_SECONDS)
        status, _headers, value = client().get_json(f"/responses/{response_id}")
        if not 200 <= status < 300:
            raise AssertionError(f"failed to poll response {response_id}: HTTP {status} {value}")

    if value.get("status") != "completed":
        raise AssertionError(value)
    payloads = response_image_payloads(value)
    if not payloads:
        raise AssertionError(f"completed response did not include image output: {value}")
    save_image_payloads("test_responses_image_generation_background", payloads)


def make_test_case(test_func):
    return unittest.FunctionTestCase(test_func, description=test_func.__name__.removeprefix("_"))


def test_images_generation_json():
    return make_test_case(_test_images_generation_json)


def test_images_edit_multipart():
    return make_test_case(_test_images_edit_multipart)


def test_images_generation_stream():
    return make_test_case(_test_images_generation_stream)


def test_images_variation():
    return make_test_case(_test_images_variation)


def test_responses_image_generation_background():
    return make_test_case(_test_responses_image_generation_background)


def load_tests(loader, tests, pattern):
    suite = unittest.TestSuite()
    suite.addTests(
        [
            test_images_generation_json(),
            test_images_edit_multipart(),
            test_images_generation_stream(),
            test_images_variation(),
            test_responses_image_generation_background(),
        ]
    )
    return suite


def parse_sse_events(body):
    events = []
    current_event = None
    data_lines = []

    for raw_line in body.decode("utf-8", errors="replace").splitlines():
        line = raw_line.strip()
        if not line:
            append_sse_event(events, current_event, data_lines)
            current_event = None
            data_lines = []
            continue
        if line.startswith(":"):
            continue
        if line.startswith("event:"):
            current_event = line.partition(":")[2].strip()
            continue
        if line.startswith("data:"):
            data_lines.append(line.partition(":")[2].lstrip())

    append_sse_event(events, current_event, data_lines)
    return events


def append_sse_event(events, event_name, data_lines):
    if not data_lines:
        return
    data = "\n".join(data_lines)
    if data == "[DONE]":
        events.append({"event": event_name, "type": "done"})
        return
    try:
        event = json.loads(data)
    except json.JSONDecodeError:
        event = {"event": event_name, "data": data}
    if event_name and isinstance(event, dict) and "event" not in event:
        event["event"] = event_name
    events.append(event)


if __name__ == "__main__":
    unittest.main()
