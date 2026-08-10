from __future__ import annotations

from dataclasses import dataclass

from shitview.core.models import FileTreeSnapshot, NodeKind


@dataclass(slots=True, frozen=True)
class LayoutRect:
    path: str
    x: float
    y: float
    width: float
    height: float
    depth: int
    kind: NodeKind
    labels: tuple[str, ...]


def build_treemap(snapshot: FileTreeSnapshot, width: float, height: float) -> list[LayoutRect]:
    root = snapshot.get(snapshot.root)
    if root is None:
        return []

    weights = _subtree_weights(snapshot)
    rects: list[LayoutRect] = []
    _slice_and_dice(snapshot, root.path, 0.0, 0.0, width, height, True, weights, rects)
    return rects


def _subtree_weights(snapshot: FileTreeSnapshot) -> dict[str, float]:
    weights: dict[str, float] = {}

    def weight(path: str) -> float:
        node = snapshot.nodes[path]
        if node.kind is NodeKind.FILE:
            result = float(max(node.size, 1))
        else:
            result = float(sum(weight(child) for child in node.children) or 1.0)
        weights[path] = result
        return result

    weight(snapshot.root)
    return weights


def _slice_and_dice(
    snapshot: FileTreeSnapshot,
    path: str,
    x: float,
    y: float,
    width: float,
    height: float,
    horizontal: bool,
    weights: dict[str, float],
    rects: list[LayoutRect],
) -> None:
    node = snapshot.nodes[path]
    rects.append(LayoutRect(path=path, x=x, y=y, width=width, height=height, depth=node.depth, kind=node.kind, labels=node.labels))

    if node.kind is NodeKind.FILE or not node.children:
        return

    total = sum(weights[child] for child in node.children) or 1.0
    cursor = 0.0
    for child in node.children:
        share = weights[child] / total
        if horizontal:
            child_width = width * share
            _slice_and_dice(snapshot, child, x + cursor, y, child_width, height, not horizontal, weights, rects)
            cursor += child_width
        else:
            child_height = height * share
            _slice_and_dice(snapshot, child, x, y + cursor, width, child_height, not horizontal, weights, rects)
            cursor += child_height


