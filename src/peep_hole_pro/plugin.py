from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

from peep_hole_pro.core.graph_layout import GraphLayout, build_layered_graph
from peep_hole_pro.core.models import FileTreeSnapshot, NodeKind
from peep_hole_pro.services.engine import PeepHoleEngine
from peep_hole_pro.services.scanner import FileSystemScanner
from peep_hole_pro.ui.qt_runner import run_qt_app


@dataclass(slots=True, frozen=True)
class FolderAnalysis:
    root: Path
    snapshot: FileTreeSnapshot
    graph: GraphLayout
    file_count: int
    directory_count: int
    leaf_count: int


def analyze_folder(root: str | Path, max_nodes: int = 420) -> FolderAnalysis:
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


def open_peep_hole(root: str | Path = ".", polling_interval: float = 1.0) -> None:
    root_path = Path(root).expanduser().resolve()
    engine = PeepHoleEngine(root=root_path, polling_interval=polling_interval)
    run_qt_app(engine)


def summarize_folder(root: str | Path, max_nodes: int = 420) -> dict[str, object]:
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

