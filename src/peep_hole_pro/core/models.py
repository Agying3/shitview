from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime
from enum import Enum
from pathlib import Path


class NodeKind(str, Enum):
    DIRECTORY = "directory"
    FILE = "file"


class ChangeKind(str, Enum):
    ADDED = "added"
    REMOVED = "removed"
    MODIFIED = "modified"
    LABEL_CHANGED = "label_changed"


@dataclass(slots=True, frozen=True)
class FileNode:
    path: str
    name: str
    kind: NodeKind
    depth: int
    size: int = 0
    mtime: float = 0.0
    children: tuple[str, ...] = ()
    labels: tuple[str, ...] = ()

    @property
    def is_directory(self) -> bool:
        return self.kind is NodeKind.DIRECTORY


@dataclass(slots=True)
class FileTreeSnapshot:
    root: str
    generated_at: datetime
    nodes: dict[str, FileNode] = field(default_factory=dict)

    def get(self, path: str) -> FileNode | None:
        return self.nodes.get(path)


@dataclass(slots=True, frozen=True)
class TreeChange:
    kind: ChangeKind
    path: str
    before: FileNode | None = None
    after: FileNode | None = None


@dataclass(slots=True, frozen=True)
class LabelRule:
    scope: str
    tags: tuple[str, ...]

    @staticmethod
    def from_strings(scope: str, tags: list[str]) -> "LabelRule":
        return LabelRule(scope=scope, tags=tuple(tag for tag in tags if tag))


def normalize_path(path: Path) -> str:
    return path.resolve().as_posix()
