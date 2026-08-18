import http.client
import json
import mimetypes
import os
import unittest
import uuid
from pathlib import Path
from urllib.parse import urlparse


TESTS_DIR = Path(__file__).resolve().parents[1]
BACKEND_DIR = TESTS_DIR.parent
ENV_FILE = BACKEND_DIR / ".env"
DEFAULT_AUDIO_FILE = TESTS_DIR / "fixtures" / "audio.wav"
DEFAULT_BASE_URL = "http://127.0.0.1:8080/v1"
DEFAULT_AUDIO_MODEL = "fun-asr-flash-2026-06-15"
REQUEST_TIMEOUT_SECONDS = 900


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


def audio_file_path():
    configured = env_value("NEOGATE_AUDIO_FILE")
    if configured:
        path = Path(configured).expanduser()
        if not path.is_absolute():
            path = BACKEND_DIR / path
    else:
        path = DEFAULT_AUDIO_FILE
    if not path.is_file():
        raise unittest.SkipTest(f"audio fixture does not exist: {path}")
    return path


def require_api_key():
    api_key = env_value("NEOGATE_API_KEY")
    if not api_key:
        raise unittest.SkipTest(
            f"NEOGATE_API_KEY is not set in the environment or {ENV_FILE}"
        )
    return api_key


def encode_multipart_form(fields, file_path):
    boundary = f"----neogate-audio-test-{uuid.uuid4().hex}"
    chunks = []
    for name, value in fields.items():
        chunks.extend(
            [
                f"--{boundary}\r\n".encode(),
                f'Content-Disposition: form-data; name="{name}"\r\n\r\n'.encode(),
                str(value).encode(),
                b"\r\n",
            ]
        )

    content_type = mimetypes.guess_type(file_path.name)[0] or "application/octet-stream"
    suffix = file_path.suffix.lower()
    filename = f"audio{suffix}" if suffix else "audio"
    chunks.extend(
        [
            f"--{boundary}\r\n".encode(),
            (
                f'Content-Disposition: form-data; name="file"; filename="{filename}"\r\n'
            ).encode(),
            f"Content-Type: {content_type}\r\n\r\n".encode(),
            file_path.read_bytes(),
            b"\r\n",
            f"--{boundary}--\r\n".encode(),
        ]
    )
    return b"".join(chunks), f"multipart/form-data; boundary={boundary}"


def post_transcription(base_url, api_key, fields, file_path):
    parsed = urlparse(base_url)
    if not parsed.scheme or not parsed.netloc:
        raise ValueError(f"invalid NEOGATE_BASE_URL: {base_url}")
    connection_type = (
        http.client.HTTPSConnection if parsed.scheme == "https" else http.client.HTTPConnection
    )
    if parsed.scheme not in {"http", "https"}:
        raise ValueError(f"unsupported URL scheme: {parsed.scheme}")

    body, content_type = encode_multipart_form(fields, file_path)
    connection = connection_type(
        parsed.hostname,
        parsed.port,
        timeout=REQUEST_TIMEOUT_SECONDS,
    )
    try:
        connection.request(
            "POST",
            f"{parsed.path.rstrip('/')}/audio/transcriptions",
            body=body,
            headers={
                "Authorization": f"Bearer {api_key}",
                "Content-Type": content_type,
            },
        )
        response = connection.getresponse()
        response_body = response.read()
        response_headers = {key.lower(): value for key, value in response.getheaders()}
        return response.status, response_headers, response_body
    finally:
        connection.close()


class OpenAiAudioTranscriptionSmokeTest(unittest.TestCase):
    def test_audio_transcription_json(self):
        fields = {
            "model": env_value("NEOGATE_AUDIO_MODEL") or DEFAULT_AUDIO_MODEL,
            "response_format": "json",
        }
        language = env_value("NEOGATE_AUDIO_LANGUAGE")
        if language:
            fields["language"] = language

        status, headers, body = post_transcription(
            normalized_base_url(),
            require_api_key(),
            fields,
            audio_file_path(),
        )
        if not 200 <= status < 300:
            preview = body[:1000].decode("utf-8", errors="replace")
            self.fail(f"expected HTTP 2xx, got {status}: {preview}")

        self.assertIn("application/json", headers.get("content-type", "").lower())
        try:
            value = json.loads(body.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError) as exc:
            self.fail(f"response body is not valid JSON: {exc}")
        text = value.get("text") if isinstance(value, dict) else None
        self.assertIsInstance(text, str)
        self.assertTrue(text.strip(), "transcription text is empty")

        expected = env_value("NEOGATE_AUDIO_EXPECTED_TEXT")
        if expected:
            self.assertIn(expected.casefold(), text.casefold())

    def test_audio_transcription_text(self):
        fields = {
            "model": env_value("NEOGATE_AUDIO_MODEL") or DEFAULT_AUDIO_MODEL,
            "response_format": "text",
        }
        language = env_value("NEOGATE_AUDIO_LANGUAGE")
        if language:
            fields["language"] = language

        status, headers, body = post_transcription(
            normalized_base_url(),
            require_api_key(),
            fields,
            audio_file_path(),
        )
        if not 200 <= status < 300:
            preview = body[:1000].decode("utf-8", errors="replace")
            self.fail(f"expected HTTP 2xx, got {status}: {preview}")

        self.assertIn("text/plain", headers.get("content-type", "").lower())
        try:
            text = body.decode("utf-8")
        except UnicodeDecodeError as exc:
            self.fail(f"response body is not valid UTF-8: {exc}")
        self.assertTrue(text.strip(), "transcription text is empty")

        expected = env_value("NEOGATE_AUDIO_EXPECTED_TEXT")
        if expected:
            self.assertIn(expected.casefold(), text.casefold())


if __name__ == "__main__":
    unittest.main()
