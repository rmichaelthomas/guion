import base64
import importlib
import io
import json
import math
import os
import struct
import sys
import traceback

_IMPORT_MODULE = importlib.import_module
_image_count = None
try:
    if sys.platform == "darwin":
        import ctypes

        _dyld = ctypes.CDLL(None)
        _image_count = _dyld._dyld_image_count
        _image_count.restype = ctypes.c_uint32
        _image_name = _dyld._dyld_get_image_name
        _image_name.argtypes = [ctypes.c_uint32]
        _image_name.restype = ctypes.c_char_p

        def _snapshot_native_images():
            images = set()
            for index in range(_image_count()):
                image = _image_name(index)
                if image:
                    images.add(os.fsdecode(image))
            return images

        _native_images_seen = _snapshot_native_images()
        _native_image_count_seen = _image_count()
        _native_image_observation = "macos_dyld_snapshot"
        _native_images_dirty = False
    else:
        _snapshot_native_images = None
        _native_images_seen = set()
        _native_image_count_seen = None
        _native_image_observation = "unsupported"
        _native_images_dirty = False
except BaseException:
    _snapshot_native_images = None
    _native_images_seen = set()
    _native_image_count_seen = None
    _native_image_observation = "unsupported"
    _native_images_dirty = False


if _snapshot_native_images is not None:
    def _mark_native_images_dirty(event, _args):
        global _native_images_dirty
        if event in ("import", "ctypes.dlopen"):
            _native_images_dirty = True

    sys.addaudithook(_mark_native_images_dirty)

MAX_FRAME = 24 * 1024 * 1024
MAX_NUMPY_ELEMENTS = MAX_FRAME // 256
BINARY_FRAME_MAGIC = b"GBIN"
PROFILE_ENABLED = os.environ.get("GUION_PYTHON_BRIDGE_PROFILE") == "1"
if PROFILE_ENABLED:
    from time import perf_counter_ns

    PROFILE_PHASES_NS = {
        "first_call": {
            "request_frame_read": 0,
            "request_setup": 0,
            "request_parse": 0,
            "request_validation_setup": 0,
            "request_value_decode": 0,
            "module_import_fallback": 0,
            "module_dispatch": 0,
            "function_call": 0,
            "stream_restore": 0,
            "result_encode": 0,
            "response_object_setup": 0,
            "output_capture": 0,
            "response_native_prepare": 0,
            "native_image_check": 0,
            "native_image_snapshot": 0,
            "response_envelope": 0,
            "profile_payload_build": 0,
            "response_json_encode": 0,
            "request_to_response_first_write": 0,
            "response_write": 0,
        },
        "repeated_calls": {
            "request_frame_read": 0,
            "request_setup": 0,
            "request_parse": 0,
            "request_validation_setup": 0,
            "request_value_decode": 0,
            "module_import_fallback": 0,
            "module_dispatch": 0,
            "function_call": 0,
            "stream_restore": 0,
            "result_encode": 0,
            "response_object_setup": 0,
            "output_capture": 0,
            "response_native_prepare": 0,
            "native_image_check": 0,
            "native_image_snapshot": 0,
            "response_envelope": 0,
            "profile_payload_build": 0,
            "response_json_encode": 0,
            "request_to_response_first_write": 0,
            "response_write": 0,
        },
    }
    PROFILE_ACTIVE_BUCKET = "first_call"
else:
    PROFILE_PHASES_NS = None
    PROFILE_ACTIVE_BUCKET = None
PROFILE_CALLS = 0
REQUEST_ORJSON_LOADS = None
LAST_REQUEST_JSON_PARSER = "stdlib"
RESPONSE_ORJSON_DUMPS = None
PROTOCOL_OUT = os.fdopen(os.dup(sys.stdout.fileno()), "wb", buffering=0)
# Keep native extension writes on fd 1/2 from corrupting the framed channel.
# Python-level writes are captured with redirect_stdout/redirect_stderr below.
devnull = os.open(os.devnull, os.O_WRONLY)
os.dup2(devnull, 1)
os.dup2(devnull, 2)
os.close(devnull)


def read_exact(count):
    if count <= 0:
        return bytearray()
    chunks = bytearray(count)
    view = memoryview(chunks)
    offset = 0
    while offset < count:
        read = sys.stdin.buffer.readinto(view[offset:])
        if not read:
            if offset == 0:
                return None
            raise EOFError("truncated frame")
        offset += read
    return chunks


def parse_request_frame(frame):
    global LAST_REQUEST_JSON_PARSER
    if REQUEST_ORJSON_LOADS is None:
        parse_json = json.loads
        LAST_REQUEST_JSON_PARSER = "stdlib"
    else:
        parse_json = REQUEST_ORJSON_LOADS
        LAST_REQUEST_JSON_PARSER = "orjson"
    if not frame.startswith(BINARY_FRAME_MAGIC):
        return parse_json(frame), []
    if len(frame) < 12:
        raise ValueError("truncated binary request frame header")
    metadata_length = struct.unpack(">I", frame[4:8])[0]
    metadata_end = 8 + metadata_length
    if metadata_end + 4 > len(frame):
        raise ValueError("truncated binary request metadata")
    request = parse_json(frame[8:metadata_end])
    attachment_count = struct.unpack(">I", frame[metadata_end:metadata_end + 4])[0]
    offset = metadata_end + 4
    attachments = []
    frame_view = memoryview(frame)
    for _ in range(attachment_count):
        if offset + 4 > len(frame):
            raise ValueError("truncated binary request attachment length")
        attachment_length = struct.unpack(">I", frame[offset:offset + 4])[0]
        offset += 4
        attachment_end = offset + attachment_length
        if attachment_end > len(frame):
            raise ValueError("truncated binary request attachment")
        attachments.append(bytes(frame_view[offset:attachment_end]))
        offset = attachment_end
    if offset != len(frame):
        raise ValueError("binary request frame has trailing bytes")
    return request, attachments


def decode(value, attachments, depth=0):
    if depth > 64:
        raise ValueError("value nesting exceeds 64 levels")
    tag = value.get("t")
    item = value.get("v")
    if tag == "unit":
        return None
    if tag == "bool" and type(item) is bool:
        return item
    if tag == "int" and isinstance(item, str):
        return int(item, 10)
    if tag == "f64" and isinstance(item, str):
        result = float(item)
        if math.isfinite(result):
            return result
    if tag == "text" and isinstance(item, str):
        return item
    if tag == "bytes" and type(item) is int and 0 <= item < len(attachments):
        return attachments[item]
    if tag == "list" and isinstance(item, list):
        return [decode(child, attachments, depth + 1) for child in item]
    if tag == "map" and isinstance(item, dict) and all(isinstance(key, str) for key in item):
        return {key: decode(child, attachments, depth + 1) for key, child in item.items()}
    if tag == "json":
        return encode_json(item)
    raise ValueError("unsupported or malformed Guion value tag")


def encode(value, depth=0):
    if depth > 64:
        raise ValueError("return value nesting exceeds 64 levels")
    if value is None:
        return {"t": "unit"}
    if type(value) is bool:
        return {"t": "bool", "v": value}
    if type(value) is int:
        return {"t": "int", "v": str(value)}
    if type(value) is float and math.isfinite(value):
        return {"t": "f64", "v": repr(value)}
    if type(value) is str:
        return {"t": "text", "v": value}
    if type(value) is bytes:
        return {"t": "bytes", "v": base64.b64encode(value).decode("ascii")}
    if type(value) is list:
        return {"t": "list", "v": [encode(child, depth + 1) for child in value]}
    if type(value) is dict and all(isinstance(key, str) for key in value):
        return {"t": "map", "v": {key: encode(child, depth + 1) for key, child in value.items()}}
    raise TypeError("Python return type is not supported by the Guion bridge")


def encode_json(value, depth=0, ancestors=None):
    """Encode only finite, acyclic values representable by JSON."""
    if depth > 64:
        raise ValueError("JSON return value nesting exceeds 64 levels")
    if value is None or type(value) in (bool, int, str):
        return value
    if type(value) is float:
        if not math.isfinite(value):
            raise ValueError("JSON return values cannot contain NaN or infinity")
        return value
    if type(value) not in (list, dict):
        raise TypeError("Python return value is not JSON-compatible")
    if ancestors is None:
        ancestors = set()
    identity = id(value)
    if identity in ancestors:
        raise ValueError("JSON return values cannot contain cycles")
    ancestors.add(identity)
    try:
        if type(value) is list:
            return [encode_json(child, depth + 1, ancestors) for child in value]
        if any(type(key) is not str for key in value):
            raise TypeError("JSON object keys must be strings")
        return {
            key: encode_json(child, depth + 1, ancestors)
            for key, child in value.items()
        }
    finally:
        ancestors.remove(identity)


def normalize_numpy_list(value):
    value_type = type(value)
    if value_type.__module__ == "numpy" and value_type.__name__ == "ndarray":
        if value.dtype.hasobject or value.dtype.kind not in "biuf":
            raise TypeError("NumPy array dtype must be a non-object boolean or numeric type")
        if value.dtype.kind in "iu" and value.dtype.itemsize > 8:
            raise TypeError("NumPy integer array elements wider than 64 bits are unsupported")
        if value.dtype.kind == "f" and value.dtype.itemsize > 8:
            raise TypeError("NumPy floating-point array elements wider than 64 bits are unsupported")
        if value.size > MAX_NUMPY_ELEMENTS:
            raise ValueError("NumPy array is too large to materialize safely for the Python bridge")
        return value.tolist()
    return value


def write_frame(value, request_ready_at=None):
    started = perf_counter_ns() if PROFILE_ENABLED else 0
    if RESPONSE_ORJSON_DUMPS is None:
        encoded = json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    else:
        try:
            encoded = RESPONSE_ORJSON_DUMPS(value)
        except (OverflowError, TypeError):
            if PROFILE_ENABLED and isinstance(value, dict):
                profile = value.get("_bridge_profile")
                if isinstance(profile, dict):
                    profile["response_json_encoder"] = "stdlib_fallback"
            encoded = json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode(
                "utf-8"
            )
    if PROFILE_ENABLED:
        PROFILE_PHASES_NS[PROFILE_ACTIVE_BUCKET]["response_json_encode"] += (
            perf_counter_ns() - started
        )
    if len(encoded) > MAX_FRAME:
        raise ValueError("bridge response exceeds the maximum frame size")
    if PROFILE_ENABLED and request_ready_at is not None:
        PROFILE_PHASES_NS[PROFILE_ACTIVE_BUCKET][
            "request_to_response_first_write"
        ] += perf_counter_ns() - request_ready_at
    started = perf_counter_ns() if PROFILE_ENABLED else 0
    PROTOCOL_OUT.write(struct.pack(">I", len(encoded)))
    PROTOCOL_OUT.write(encoded)
    PROTOCOL_OUT.flush()
    if PROFILE_ENABLED:
        PROFILE_PHASES_NS[PROFILE_ACTIVE_BUCKET]["response_write"] += (
            perf_counter_ns() - started
        )


while True:
    if PROFILE_ENABLED:
        PROFILE_ACTIVE_BUCKET = "first_call" if PROFILE_CALLS == 0 else "repeated_calls"
    started = perf_counter_ns() if PROFILE_ENABLED else 0
    header = read_exact(4)
    if header is None:
        break
    size = struct.unpack(">I", header)[0]
    if size > MAX_FRAME:
        break
    frame = read_exact(size)
    if frame is None:
        break
    request_ready_at = perf_counter_ns() if PROFILE_ENABLED else None
    if PROFILE_ENABLED:
        PROFILE_PHASES_NS[PROFILE_ACTIVE_BUCKET]["request_frame_read"] += (
            perf_counter_ns() - started
        )
    if PROFILE_ENABLED:
        PROFILE_CALLS += 1
    request_setup_started = perf_counter_ns() if PROFILE_ENABLED else 0
    request_id = None
    stdout_capture = io.StringIO()
    stderr_capture = io.StringIO()
    if PROFILE_ENABLED:
        PROFILE_PHASES_NS[PROFILE_ACTIVE_BUCKET]["request_setup"] += (
            perf_counter_ns() - request_setup_started
        )
    try:
        started = perf_counter_ns() if PROFILE_ENABLED else 0
        request, attachments = parse_request_frame(frame)
        if PROFILE_ENABLED:
            PROFILE_PHASES_NS[PROFILE_ACTIVE_BUCKET]["request_parse"] += (
                perf_counter_ns() - started
            )
        validation_setup_started = perf_counter_ns() if PROFILE_ENABLED else 0
        request_id = request["id"]
        if request.get("protocol") != 2 or request.get("kind") != "call":
            raise ValueError("unsupported bridge protocol request")
        previous_stdout, previous_stderr = sys.stdout, sys.stderr
        sys.stdout, sys.stderr = stdout_capture, stderr_capture
        if PROFILE_ENABLED:
            PROFILE_PHASES_NS[PROFILE_ACTIVE_BUCKET]["request_validation_setup"] += (
                perf_counter_ns() - validation_setup_started
            )
        try:
            started = perf_counter_ns() if PROFILE_ENABLED else 0
            module_name = request["module"]
            import_module = importlib.import_module
            module_cache_hit = import_module is _IMPORT_MODULE
            module = sys.modules.get(module_name) if module_cache_hit else None
            module_cache_hit = module is not None and module_cache_hit
            if module is None:
                module = import_module(module_name)
            if PROFILE_ENABLED and not module_cache_hit:
                PROFILE_PHASES_NS[PROFILE_ACTIVE_BUCKET]["module_import_fallback"] += (
                    perf_counter_ns() - started
                )
            function = getattr(module, request["function"])
            if RESPONSE_ORJSON_DUMPS is None:
                fast_json = sys.modules.get("orjson")
                candidate = getattr(fast_json, "dumps", None)
                if callable(candidate):
                    RESPONSE_ORJSON_DUMPS = candidate
            if REQUEST_ORJSON_LOADS is None:
                fast_json = sys.modules.get("orjson")
                candidate = getattr(fast_json, "loads", None)
                if callable(candidate):
                    REQUEST_ORJSON_LOADS = candidate
            if PROFILE_ENABLED:
                PROFILE_PHASES_NS[PROFILE_ACTIVE_BUCKET]["module_dispatch"] += (
                    perf_counter_ns() - started
                )
            started = perf_counter_ns() if PROFILE_ENABLED else 0
            args = [decode(item, attachments) for item in request.get("args", [])]
            kwargs = {
                name: decode(item, attachments)
                for name, item in request.get("kwargs", {}).items()
            }
            if PROFILE_ENABLED:
                PROFILE_PHASES_NS[PROFILE_ACTIVE_BUCKET]["request_value_decode"] += (
                    perf_counter_ns() - started
                )
            started = perf_counter_ns() if PROFILE_ENABLED else 0
            result = function(*args, **kwargs)
            if PROFILE_ENABLED:
                PROFILE_PHASES_NS[PROFILE_ACTIVE_BUCKET]["function_call"] += (
                    perf_counter_ns() - started
                )
        finally:
            restore_started = perf_counter_ns() if PROFILE_ENABLED else 0
            sys.stdout, sys.stderr = previous_stdout, previous_stderr
            if PROFILE_ENABLED:
                PROFILE_PHASES_NS[PROFILE_ACTIVE_BUCKET]["stream_restore"] += (
                    perf_counter_ns() - restore_started
                )
        started = perf_counter_ns() if PROFILE_ENABLED else 0
        if request.get("numpy_list_result") is True:
            result = normalize_numpy_list(result)
        if request.get("json_result") is True:
            encoded_result = {"t": "json", "v": encode_json(result)}
        else:
            encoded_result = encode(result)
        if PROFILE_ENABLED:
            PROFILE_PHASES_NS[PROFILE_ACTIVE_BUCKET]["result_encode"] += (
                perf_counter_ns() - started
            )
        response_setup_started = perf_counter_ns() if PROFILE_ENABLED else 0
        response = {"protocol": 2, "id": request_id, "ok": True, "value": encoded_result}
        if PROFILE_ENABLED:
            PROFILE_PHASES_NS[PROFILE_ACTIVE_BUCKET]["response_object_setup"] += (
                perf_counter_ns() - response_setup_started
            )
    except BaseException as error:
        response_setup_started = perf_counter_ns() if PROFILE_ENABLED else 0
        response = {
            "protocol": 2,
            "id": request_id,
            "ok": False,
            "error": {
                "type": type(error).__name__,
                "message": str(error)[:8192],
                "traceback": traceback.format_exc()[-32768:],
            },
        }
        if PROFILE_ENABLED:
            PROFILE_PHASES_NS[PROFILE_ACTIVE_BUCKET]["response_object_setup"] += (
                perf_counter_ns() - response_setup_started
            )
    started = perf_counter_ns() if PROFILE_ENABLED else 0
    response["stdout"] = stdout_capture.getvalue()[:65536]
    response["stderr"] = stderr_capture.getvalue()[:65536]
    if PROFILE_ENABLED:
        PROFILE_PHASES_NS[PROFILE_ACTIVE_BUCKET]["output_capture"] += (
            perf_counter_ns() - started
        )
    native_prepare_started = perf_counter_ns() if PROFILE_ENABLED else 0
    response["native_images"] = []
    native_image_count_changed = False
    if PROFILE_ENABLED:
        PROFILE_PHASES_NS[PROFILE_ACTIVE_BUCKET]["response_native_prepare"] += (
            perf_counter_ns() - native_prepare_started
        )
    if _image_count is not None:
        started = perf_counter_ns() if PROFILE_ENABLED else 0
        try:
            native_image_count_changed = _image_count() != _native_image_count_seen
        except BaseException:
            _image_count = None
            _native_image_observation = "unsupported"
        if PROFILE_ENABLED:
            PROFILE_PHASES_NS[PROFILE_ACTIVE_BUCKET]["native_image_check"] += (
                perf_counter_ns() - started
            )
    if _snapshot_native_images is not None and (
        _native_images_dirty or native_image_count_changed
    ):
        started = perf_counter_ns() if PROFILE_ENABLED else 0
        try:
            current_images = _snapshot_native_images()
            response["native_images"] = sorted(current_images - _native_images_seen)
            _native_images_seen.update(current_images)
            _native_image_count_seen = _image_count()
            _native_images_dirty = False
        except BaseException:
            response["native_image_observation"] = "unsupported"
            _native_image_observation = "unsupported"
            _snapshot_native_images = None
            _image_count = None
        if PROFILE_ENABLED:
            PROFILE_PHASES_NS[PROFILE_ACTIVE_BUCKET]["native_image_snapshot"] += (
                perf_counter_ns() - started
            )
    started = perf_counter_ns() if PROFILE_ENABLED else 0
    response["native_image_observation"] = _native_image_observation
    if PROFILE_ENABLED:
        PROFILE_PHASES_NS[PROFILE_ACTIVE_BUCKET]["response_envelope"] += (
            perf_counter_ns() - started
        )
        profile_payload_started = perf_counter_ns()
        bridge_profile = {
            "calls": PROFILE_CALLS,
            "request_json_parser": LAST_REQUEST_JSON_PARSER,
            "response_json_encoder": (
                "orjson" if RESPONSE_ORJSON_DUMPS is not None else "stdlib"
            ),
            "first_call_ns": dict(PROFILE_PHASES_NS["first_call"]),
            "repeated_calls_ns": dict(PROFILE_PHASES_NS["repeated_calls"]),
        }
        PROFILE_PHASES_NS[PROFILE_ACTIVE_BUCKET]["profile_payload_build"] += (
            perf_counter_ns() - profile_payload_started
        )
        response["_bridge_profile"] = bridge_profile
    try:
        write_frame(response, request_ready_at=request_ready_at)
    except BaseException:
        break
