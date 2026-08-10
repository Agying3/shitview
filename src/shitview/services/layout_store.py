from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path


LAYOUT_VERSION = 3


@dataclass(slots=True)
class UserLayoutStore:
    root: Path

    @property
    def path(self) -> Path:
        return self.root / ".shitview" / "layout.json"

    def load_positions(self) -> dict[str, tuple[float, float]]:
        if not self.path.exists():
            return {}
        try:
            data = json.loads(self.path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            return {}
        if data.get("version") != LAYOUT_VERSION:
            return {}
        nodes = data.get("nodes", {})
        positions: dict[str, tuple[float, float]] = {}
        if not isinstance(nodes, dict):
            return positions
        for node_path, value in nodes.items():
            if (
                isinstance(node_path, str)
                and isinstance(value, list)
                and len(value) == 2
                and all(isinstance(item, int | float) for item in value)
            ):
                positions[node_path] = (float(value[0]), float(value[1]))
        return positions

    def save_position(self, node_path: str, x: float, y: float) -> None:
        data = self._load_raw()
        nodes = data.setdefault("nodes", {})
        if not isinstance(nodes, dict):
            nodes = {}
            data["nodes"] = nodes
        nodes[node_path] = [round(x, 2), round(y, 2)]
        data["version"] = LAYOUT_VERSION
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.path.write_text(json.dumps(data, ensure_ascii=False, indent=2), encoding="utf-8")

    def _load_raw(self) -> dict[str, object]:
        if not self.path.exists():
            return {"version": LAYOUT_VERSION, "nodes": {}}
        try:
            data = json.loads(self.path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            return {"version": LAYOUT_VERSION, "nodes": {}}
        if not isinstance(data, dict) or data.get("version") != LAYOUT_VERSION:
            return {"version": LAYOUT_VERSION, "nodes": {}}
        return data


