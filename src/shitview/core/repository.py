from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path

from shitview.core.labels import LabelCatalog, LabelStore
from shitview.core.models import FileTreeSnapshot


@dataclass(slots=True)
class ProjectRepository:
    root: Path
    labels: LabelCatalog = field(default_factory=LabelCatalog)
    snapshot: FileTreeSnapshot | None = None

    def attach_labels(self, store: LabelStore) -> None:
        self.labels = store.load()

    def set_snapshot(self, snapshot: FileTreeSnapshot) -> None:
        self.snapshot = snapshot


