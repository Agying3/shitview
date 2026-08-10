from __future__ import annotations

import json
from dataclasses import dataclass, field
from pathlib import Path

from shitview.core.models import LabelRule


@dataclass(slots=True)
class LabelCatalog:
    rules: list[LabelRule] = field(default_factory=list)

    def labels_for(self, path: str) -> tuple[str, ...]:
        collected: list[str] = []
        normalized = path.replace("\\", "/")
        for rule in self.rules:
            scope = rule.scope.replace("\\", "/").rstrip("/")
            if normalized == scope or normalized.startswith(scope + "/"):
                for tag in rule.tags:
                    if tag not in collected:
                        collected.append(tag)
        return tuple(collected)

    def set_rule(self, scope: str, tags: list[str]) -> None:
        updated = LabelRule.from_strings(scope, tags)
        for index, rule in enumerate(self.rules):
            if rule.scope == updated.scope:
                self.rules[index] = updated
                return
        self.rules.append(updated)


class LabelStore:
    def __init__(self, root: Path) -> None:
        self._root = root
        self._path = root / ".shitview" / "labels.json"

    @property
    def path(self) -> Path:
        return self._path

    def load(self) -> LabelCatalog:
        if not self._path.exists():
            return LabelCatalog()
        data = json.loads(self._path.read_text(encoding="utf-8"))
        rules = [LabelRule.from_strings(item["scope"], item["tags"]) for item in data.get("rules", [])]
        return LabelCatalog(rules=rules)

    def save(self, catalog: LabelCatalog) -> None:
        self._path.parent.mkdir(parents=True, exist_ok=True)
        payload = {
            "rules": [{"scope": rule.scope, "tags": list(rule.tags)} for rule in catalog.rules]
        }
        self._path.write_text(json.dumps(payload, ensure_ascii=False, indent=2), encoding="utf-8")


