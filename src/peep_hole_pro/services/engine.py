from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path

from peep_hole_pro.core.diff import diff_snapshots
from peep_hole_pro.core.events import EventBus, StatusMessage
from peep_hole_pro.core.labels import LabelStore
from peep_hole_pro.core.repository import ProjectRepository
from peep_hole_pro.services.scanner import FileSystemScanner
from peep_hole_pro.services.watcher import PollingFolderWatcher


@dataclass(slots=True)
class PeepHoleEngine:
    root: Path
    polling_interval: float = 1.0
    events: EventBus = field(default_factory=EventBus)
    repository: ProjectRepository = field(init=False)
    label_store: LabelStore = field(init=False)
    scanner: FileSystemScanner = field(init=False)
    watcher: PollingFolderWatcher = field(init=False)

    def __post_init__(self) -> None:
        self.repository = ProjectRepository(root=self.root)
        self.label_store = LabelStore(self.root)
        self.repository.attach_labels(self.label_store)
        self.scanner = FileSystemScanner(self.root)
        self.watcher = PollingFolderWatcher(root=self.root, interval=self.polling_interval, on_change=self.refresh)

    def refresh(self) -> None:
        snapshot = self.scanner.scan(labels=self.repository.labels)
        changes = diff_snapshots(self.repository.snapshot, snapshot)
        self.repository.set_snapshot(snapshot)
        self.events.publish("snapshot", snapshot)
        self.events.publish("changes", changes)
        self.events.publish("status", StatusMessage(text=f"Indexed {len(snapshot.nodes)} nodes, {len(changes)} changes"))

    def start(self) -> None:
        self.refresh()
        self.watcher.start()

    def stop(self) -> None:
        self.watcher.stop()

    def set_label(self, scope: str, tags: list[str]) -> None:
        self.repository.labels.set_rule(scope, tags)
        self.label_store.save(self.repository.labels)
        self.refresh()

