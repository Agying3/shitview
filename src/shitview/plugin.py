from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

from shitview.core.graph_layout import GraphLayout, build_layered_graph
from shitview.core.models import FileTreeSnapshot, NodeKind
from shitview.services.engine import ShitviewEngine
from shitview.services.scanner import FileSystemScanner
from shitview.ui.qt_runner import run_qt_app


@dataclass(slots=True, frozen=True)
class FolderAnalysis:
    root: Path
    snapshot: FileTreeSnapshot
    graph: GraphLayout
    file_count: int
    directory_count: int
    leaf_count: int


def analyze_folder(root: str | Path, max_nodes: int = 500) -> FolderAnalysis:
    root_path = Path(root).expanduser().resolve()
    snapshot = FileSystemScanner(root_path).scan()
    graph = build_layered_graph(snapshot, max_nodes=max_nodes)
    nodes = snapshot.nodes.values()
    file_count = sum(1 for node in nodes if node.kind is NodeKind.FILE)
    directory_count = sum(1 for node in snapshot.nodes.values() if node.kind is NodeKind.DIRECTORY)
    leaf_count = sum(1 for node in snapshot.nodes.values() if not node.children)
    return FolderAnalysis(
        root=root_path,
        snapshot=snapshot,
        graph=graph,
        file_count=file_count,
        directory_count=directory_count,
        leaf_count=leaf_count,
    )


def open_shitview(root: str | Path = ".", polling_interval: float = 1.0) -> None:
    root_path = Path(root).expanduser().resolve()
    engine = ShitviewEngine(root=root_path, polling_interval=polling_interval)
    run_qt_app(engine)


def summarize_folder(root: str | Path, max_nodes: int = 500) -> dict[str, object]:
    analysis = analyze_folder(root, max_nodes=max_nodes)
    return {
        "root": str(analysis.root),
        "files": analysis.file_count,
        "directories": analysis.directory_count,
        "leaf_nodes": analysis.leaf_count,
        "visible_nodes": len(analysis.graph.nodes),
        "groups": [group.name for group in analysis.graph.groups],
        "scene": {"width": analysis.graph.width, "height": analysis.graph.height},
    }


