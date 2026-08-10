from __future__ import annotations

from dataclasses import dataclass


@dataclass(slots=True)
class SelectionState:
    path: str | None = None
