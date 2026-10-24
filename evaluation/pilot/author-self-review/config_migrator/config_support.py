"""TOML parsing and comment-preserving transformations for the Veritas pilot."""
import tomlkit


def inspect_and_migrate(source: str, operation: str) -> dict:
    try:
        document = tomlkit.parse(source)
        version = document.get("version")
        if isinstance(version, bool) or not isinstance(version, int):
            return {"ok": False, "error": "version must be an integer"}
        if version > 3:
            return {"ok": False, "error": f"future schema version {version} is not supported"}
        if version < 1:
            return {"ok": False, "error": f"unsupported schema version {version}"}

        name = document.get("name")
        if not isinstance(name, str) or not name:
            return {"ok": False, "error": "name must be non-empty text"}

        sync = document.get("sync")
        if version >= 2:
            if not isinstance(sync, dict):
                return {"ok": False, "error": "sync section is required"}
            timeout = sync.get("timeout_seconds")
            if isinstance(timeout, bool) or not isinstance(timeout, int) or timeout <= 0:
                return {"ok": False, "error": "sync.timeout_seconds must be a positive integer"}
        if version >= 3:
            parallelism = sync.get("parallelism")
            if isinstance(parallelism, bool) or not isinstance(parallelism, int) or parallelism <= 0:
                return {"ok": False, "error": "sync.parallelism must be a positive integer"}

        if operation == "check":
            if version != 3:
                return {"ok": False, "error": f"configuration version {version} is not current"}
            return {"ok": True, "version": int(version), "updated": source}

        if version == 1:
            if "sync" in document:
                if not isinstance(sync, dict):
                    return {"ok": False, "error": "sync must be a table"}
                if "timeout_seconds" in sync:
                    return {"ok": False, "error": "schema 1 cannot define sync.timeout_seconds"}
            elif "sync" in document:
                return {"ok": False, "error": "sync must be a table"}
            document["version"] = 2
            if "sync" not in document:
                document["sync"] = {"timeout_seconds": 30}
            else:
                document["sync"]["timeout_seconds"] = 30
        elif version == 2:
            if "parallelism" in sync:
                # Preserve an explicit value, but reject invalid explicit values.
                value = sync["parallelism"]
                if isinstance(value, bool) or not isinstance(value, int) or value <= 0:
                    return {"ok": False, "error": "sync.parallelism must be a positive integer"}
            else:
                sync["parallelism"] = 4
            document["version"] = 3
        return {"ok": True, "version": int(version) + 1, "updated": tomlkit.dumps(document)}
    except Exception as exc:
        return {"ok": False, "error": f"invalid TOML: {exc}"}
