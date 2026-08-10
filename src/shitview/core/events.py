from __future__ import annotations

from collections import defaultdict
from dataclasses import dataclass
from typing import Any, Callable


Handler = Callable[[Any], None]


class EventBus:
    def __init__(self) -> None:
        self._handlers: dict[str, list[Handler]] = defaultdict(list)

    def subscribe(self, topic: str, handler: Handler) -> None:
        self._handlers[topic].append(handler)

    def publish(self, topic: str, payload: Any = None) -> None:
        for handler in list(self._handlers.get(topic, [])):
            handler(payload)


@dataclass(slots=True, frozen=True)
class StatusMessage:
    text: str
