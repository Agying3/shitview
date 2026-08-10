from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path

from shitview.core.labels import LabelCatalog
from shitview.core.models import FileNode, FileTreeSnapshot, NodeKind, normalize_path

DEFAULT_IGNORED_NAMES = {
    ".git",
    "__pycache__",
    ".venv",
    "node_modules",
    ".shitview",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
}
DEFAULT_IGNORED_SUFFIXES = (".egg-info",)


@dataclass(slots=True)
class FileSystemScanner:
    root: Path
    ignored_names: set[str] | None = None
    max_nodes: int = 1600
    max_children_per_dir: int = 180

    def scan(self, labels: LabelCatalog | None = None) -> FileTreeSnapshot:
        root_path = self.root.resolve()
        ignored = self.ignored_names or set(DEFAULT_IGNORED_NAMES)
        nodes: dict[str, FileNode] = {}

        def walk(current: Path, depth: int) -> str:
            current_id = normalize_path(current)
            try:
                stat = current.stat()
            except OSError:
                stat = root_path.stat()
            if current.is_dir():
                remaining = max(0, self.max_nodes - len(nodes) - 1)
                children_paths = _list_children(current, ignored, min(self.max_children_per_dir, remaining))
                child_ids = tuple(walk(child, depth + 1) for child in children_paths)
                labels_for_node = labels.labels_for(current_id) if labels else ()
                node = FileNode(
                    path=current_id,
                    name=current.name or current_id,
                    kind=NodeKind.DIRECTORY,
                    depth=depth,
                    size=sum(nodes[child_id].size for child_id in child_ids),
                    mtime=stat.st_mtime,
                    children=child_ids,
                    labels=labels_for_node,
                )
            else:
                labels_for_node = labels.labels_for(current_id) if labels else ()
                node = FileNode(
                    path=current_id,
                    name=current.name,
                    kind=NodeKind.FILE,
                    depth=depth,
                    size=stat.st_size,
                    mtime=stat.st_mtime,
                    children=(),
                    labels=labels_for_node,
                )
            nodes[current_id] = node
            return current_id

        walk(root_path, 0)
        return FileTreeSnapshot(root=normalize_path(root_path), generated_at=datetime.now(timezone.utc), nodes=nodes)


def _is_ignored(path: Path, ignored_names: set[str]) -> bool:
    if path.name in ignored_names:
        return True
    if path.is_dir() and path.name.endswith(DEFAULT_IGNORED_SUFFIXES):
        return True
    return False


def _list_children(path: Path, ignored_names: set[str], limit: int) -> list[Path]:
    if limit <= 0:
        return []
    directories: list[Path] = []
    files: list[Path] = []
    try:
        children = path.iterdir()
        for child in children:
            if _is_ignored(child, ignored_names):
                continue
            target = directories if child.is_dir() else files
            target.append(child)
            if len(directories) + len(files) >= limit:
                break
    except OSError:
        return []
    directories.sort(key=lambda item: item.name.lower())
    files.sort(key=lambda item: item.name.lower())
    return directories + files


