from __future__ import annotations

import hashlib
import os
import threading
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable

DEFAULT_IGNORED_NAMES = {".git", "__pycache__", ".venv", "node_modules", ".peep-hole-pro"}


@dataclass(slots=True)
class PollingFolderWatcher:
    root: Path
    interval: float
    on_change: Callable[[], None]
    ignored_names: set[str] | None = None
    _stop_event: threading.Event = field(init=False)
    _thread: threading.Thread | None = field(default=None, init=False)

    def __post_init__(self) -> None:
        self._stop_event = threading.Event()

    def start(self) -> None:
        if self._thread and self._thread.is_alive():
            return
        self._thread = threading.Thread(target=self._run, name="peep-hole-pro-watcher", daemon=True)
        self._thread.start()

    def stop(self) -> None:
        self._stop_event.set()
        if self._thread and self._thread.is_alive():
            self._thread.join(timeout=self.interval * 2)

    def _run(self) -> None:
        previous = self._fingerprint()
        while not self._stop_event.wait(self.interval):
            current = self._fingerprint()
            if current != previous:
                previous = current
                self.on_change()

    def _fingerprint(self) -> str:
        ignored = self.ignored_names or DEFAULT_IGNORED_NAMES
        digest = hashlib.sha1()
        for dirpath, dirnames, filenames in os.walk(self.root):
            dirnames[:] = [name for name in dirnames if name not in ignored]
            filenames = [name for name in filenames if name not in ignored]
            rel_dir = Path(dirpath).relative_to(self.root).as_posix()
            digest.update(rel_dir.encode("utf-8"))
            for name in sorted(dirnames):
                digest.update(f"D:{name}".encode("utf-8"))
            for name in sorted(filenames):
                file_path = Path(dirpath) / name
                stat = file_path.stat()
                digest.update(f"F:{name}:{stat.st_size}:{stat.st_mtime_ns}".encode("utf-8"))
        return digest.hexdigest()

