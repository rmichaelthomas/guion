"""Create a host-side, source-free delivery tree without executing candidate code."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import sys

from case_runner import _prepare_delivery


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("candidate_root", type=Path)
    parser.add_argument("base_candidate_config", type=Path)
    parser.add_argument("artifact_root", type=Path)
    parser.add_argument("delivery_candidate_config", type=Path)
    args = parser.parse_args()
    source = args.candidate_root.resolve()
    artifact_root = args.artifact_root.resolve()
    if artifact_root == source or source in artifact_root.parents:
        raise SystemExit("delivery staging directory must be outside the original candidate project")
    base = json.loads(args.base_candidate_config.read_text(encoding="utf-8"))
    if not isinstance(base, dict):
        raise SystemExit("base candidate configuration must be an object")
    _, delivery = _prepare_delivery(source, artifact_root)
    staged = dict(base)
    staged["command"] = delivery["command"]
    staged["metadata"] = delivery["metadata"]
    args.delivery_candidate_config.write_text(json.dumps(staged, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"artifact_root": str(artifact_root), "artifact_sha256": delivery["metadata"]["artifact_sha256"], "artifact_bytes": delivery["metadata"]["artifact_bytes"], "source_paths_excluded": delivery["metadata"]["source_paths"]}, ensure_ascii=False, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    raise SystemExit(main())
