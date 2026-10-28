import contextlib
import base64
import importlib
import io
import json
import math
import os
import struct
import sys
import traceback

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
        _native_image_observation = "macos_dyld_snapshot"
    else:
        _snapshot_native_images = None
        _native_images_seen = set()
        _native_image_observation = "unsupported"
except BaseException:
    _snapshot_native_images = None
    _native_images_seen = set()
    _native_image_observation = "unsupported"

MAX_FRAME = 24 * 1024 * 1024
MAX_NUMPY_ELEMENTS = MAX_FRAME // 256
PROTOCOL_OUT = os.fdopen(os.dup(sys.stdout.fileno()), "wb", buffering=0)
# Keep native extension writes on fd 1/2 from corrupting the framed channel.
# Python-level writes are captured with redirect_stdout/redirect_stderr below.
devnull = os.open(os.devnull, os.O_WRONLY)
os.dup2(devnull, 1)
os.dup2(devnull, 2)
os.close(devnull)


def read_exact(count):
    chunks = bytearray()
    while len(chunks) < count:
        chunk = sys.stdin.buffer.read(count - len(chunks))
        if not chunk:
            if not chunks:
                return None
            raise EOFError("truncated frame")
        chunks.extend(chunk)
    return chunks


def decode(value, depth=0):
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
    if tag == "bytes" and isinstance(item, str):
        return base64.b64decode(item, validate=True)
    if tag == "list" and isinstance(item, list):
        return [decode(child, depth + 1) for child in item]
    if tag == "map" and isinstance(item, dict) and all(isinstance(key, str) for key in item):
        return {key: decode(child, depth + 1) for key, child in item.items()}
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


def write_frame(value):
    encoded = json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    if len(encoded) > MAX_FRAME:
        raise ValueError("bridge response exceeds the maximum frame size")
    PROTOCOL_OUT.write(struct.pack(">I", len(encoded)))
    PROTOCOL_OUT.write(encoded)
    PROTOCOL_OUT.flush()


while True:
    header = read_exact(4)
    if header is None:
        break
    size = struct.unpack(">I", header)[0]
    if size > MAX_FRAME:
        break
    frame = read_exact(size)
    if frame is None:
        break
    request_id = None
    stdout_capture = io.StringIO()
    stderr_capture = io.StringIO()
    try:
        request = json.loads(frame)
        request_id = request["id"]
        if request.get("protocol") != 1 or request.get("kind") != "call":
            raise ValueError("unsupported bridge protocol request")
        with contextlib.redirect_stdout(stdout_capture), contextlib.redirect_stderr(stderr_capture):
            module = importlib.import_module(request["module"])
            function = getattr(module, request["function"])
            args = [decode(item) for item in request.get("args", [])]
            kwargs = {name: decode(item) for name, item in request.get("kwargs", {}).items()}
            result = function(*args, **kwargs)
        if request.get("numpy_list_result") is True:
            result = normalize_numpy_list(result)
        if request.get("json_result") is True:
            encoded_result = {"t": "json", "v": encode_json(result)}
        else:
            encoded_result = encode(result)
        response = {"protocol": 1, "id": request_id, "ok": True, "value": encoded_result}
    except BaseException as error:
        response = {
            "protocol": 1,
            "id": request_id,
            "ok": False,
            "error": {
                "type": type(error).__name__,
                "message": str(error)[:8192],
                "traceback": traceback.format_exc()[-32768:],
            },
        }
    response["stdout"] = stdout_capture.getvalue()[:65536]
    response["stderr"] = stderr_capture.getvalue()[:65536]
    response["native_image_observation"] = _native_image_observation
    response["native_images"] = []
    if _snapshot_native_images is not None:
        try:
            current_images = _snapshot_native_images()
            response["native_images"] = sorted(current_images - _native_images_seen)
            _native_images_seen.update(current_images)
        except BaseException:
            response["native_image_observation"] = "unsupported"
            _native_image_observation = "unsupported"
            _snapshot_native_images = None
    try:
        write_frame(response)
    except BaseException:
        break
