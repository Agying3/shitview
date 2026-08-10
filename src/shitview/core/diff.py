from __future__ import annotations

from shitview.core.models import ChangeKind, FileTreeSnapshot, TreeChange


def diff_snapshots(before: FileTreeSnapshot | None, after: FileTreeSnapshot) -> list[TreeChange]:
    if before is None:
        return [TreeChange(kind=ChangeKind.ADDED, path=path, after=node) for path, node in after.nodes.items()]

    changes: list[TreeChange] = []
    before_nodes = before.nodes
    after_nodes = after.nodes

    for path, node in after_nodes.items():
        prior = before_nodes.get(path)
        if prior is None:
            changes.append(TreeChange(kind=ChangeKind.ADDED, path=path, after=node))
        elif prior != node:
            change_kind = ChangeKind.LABEL_CHANGED if prior.labels != node.labels and _same_content(prior, node) else ChangeKind.MODIFIED
            changes.append(TreeChange(kind=change_kind, path=path, before=prior, after=node))

    for path, node in before_nodes.items():
        if path not in after_nodes:
            changes.append(TreeChange(kind=ChangeKind.REMOVED, path=path, before=node))

    return changes


def _same_content(before, after) -> bool:
    return before.kind == after.kind and before.size == after.size and before.mtime == after.mtime


