from __future__ import annotations

from dataclasses import dataclass

from shitview.core.models import FileTreeSnapshot, NodeKind


@dataclass(slots=True, frozen=True)
class GraphNode:
    path: str
    name: str
    x: float
    y: float
    width: float
    height: float
    depth: int
    kind: NodeKind
    labels: tuple[str, ...]
    child_count: int
    size: int
    mtime: float
    branch_key: str
    branch_index: int


@dataclass(slots=True, frozen=True)
class GraphEdge:
    source: str
    target: str


@dataclass(slots=True, frozen=True)
class GraphGroup:
    path: str
    name: str
    x: float
    y: float
    width: float
    height: float
    depth: int
    child_count: int
    mtime: float
    child_paths: tuple[str, ...]
    branch_key: str
    branch_index: int


@dataclass(slots=True, frozen=True)
class GraphLayout:
    nodes: tuple[GraphNode, ...]
    edges: tuple[GraphEdge, ...]
    groups: tuple[GraphGroup, ...]
    width: float
    height: float
    leaf_count: int


def build_layered_graph(snapshot: FileTreeSnapshot, max_nodes: int = 420) -> GraphLayout:
    root = snapshot.get(snapshot.root)
    if root is None:
        return GraphLayout(nodes=(), edges=(), groups=(), width=1200, height=800, leaf_count=0)

    visible_paths = _collect_visible_paths(snapshot, max_nodes=max_nodes)
    visible_set = set(visible_paths)
    parent_map = _build_parent_map(snapshot)
    branch_roots = _choose_branch_roots(snapshot, snapshot.root, visible_set)
    branch_order = {path: index for index, path in enumerate(branch_roots)}
    branch_key_map = {snapshot.root: snapshot.root}
    for path in visible_paths:
        branch_key_map[path] = _nearest_branch(path, parent_map, branch_order, snapshot.root)

    graph_nodes: list[GraphNode] = []
    graph_edges: list[GraphEdge] = []
    placed: set[str] = set()
    max_x = 1600.0
    max_y = 900.0

    def add_node(path: str, x: float, y: float) -> None:
        nonlocal max_x, max_y
        if path in placed:
            return
        source = snapshot.nodes[path]
        width = 360.0 if source.kind is NodeKind.DIRECTORY else 340.0
        height = 124.0 if source.kind is NodeKind.DIRECTORY else 116.0
        branch_key = branch_key_map.get(path, snapshot.root)
        branch_index = branch_order.get(branch_key, 0)
        graph_nodes.append(
            GraphNode(
                path=path,
                name=source.name,
                x=x,
                y=y,
                width=width,
                height=height,
                depth=source.depth,
                kind=source.kind,
                labels=source.labels,
                child_count=len(source.children),
                size=source.size,
                mtime=source.mtime,
                branch_key=branch_key,
                branch_index=branch_index,
            )
        )
        placed.add(path)
        max_x = max(max_x, x + width + 120.0)
        max_y = max(max_y, y + height + 120.0)

    def descendants(path: str) -> list[str]:
        found: list[str] = []
        queue = [child for child in snapshot.nodes[path].children if child in visible_set]
        while queue and len(found) < 80:
            child = queue.pop(0)
            found.append(child)
            queue.extend(grandchild for grandchild in snapshot.nodes[child].children if grandchild in visible_set)
        return found

    add_node(snapshot.root, 690.0, 70.0)
    _place_context_nodes(snapshot, visible_set, branch_roots, parent_map, add_node)

    for index, path in enumerate(branch_roots):
        source = snapshot.nodes[path]
        child_paths = descendants(path) if source.kind is NodeKind.DIRECTORY else []
        columns = 2 if len(child_paths) < 12 else 3
        column_gap = 370.0
        row_gap = 150.0
        rows = max(1, (len(child_paths) + columns - 1) // columns)
        section_height = 260.0 + rows * row_gap if child_paths else 260.0
        section_width = 220.0 + columns * column_gap
        slot_columns = 2 if len(branch_roots) <= 4 else 3
        slot_gap_x = max(section_width - 200.0, 760.0)
        slot_gap_y = max(section_height + 180.0, 650.0)
        slot_x = index % slot_columns
        slot_y = index // slot_columns
        origin_x = 110.0 + slot_x * slot_gap_x + (slot_y % 2) * 72.0
        origin_y = 460.0 + slot_y * slot_gap_y

        add_node(path, origin_x + 54.0, origin_y + 62.0)
        for child_index, child in enumerate(child_paths):
            child_column = child_index % columns
            child_row = child_index // columns
            child_x = origin_x + 58.0 + child_column * column_gap
            child_y = origin_y + 206.0 + child_row * row_gap
            add_node(child, child_x, child_y)

        max_x = max(max_x, origin_x + section_width)
        max_y = max(max_y, origin_y + section_height)

    unplaced = [path for path in visible_paths if path not in placed]
    loose_start_y = max_y + 120.0
    for index, path in enumerate(unplaced):
        add_node(path, 160.0 + (index % 3) * 430.0, loose_start_y + (index // 3) * 144.0)

    groups = _build_groups(snapshot, graph_nodes, branch_order)
    for path in visible_paths:
        node = snapshot.nodes[path]
        for child in node.children:
            if child in visible_set:
                graph_edges.append(GraphEdge(source=path, target=child))

    return GraphLayout(
        nodes=tuple(graph_nodes),
        edges=tuple(graph_edges),
        groups=tuple(groups),
        width=max(max_x, 1200.0),
        height=max(max_y, 760.0),
        leaf_count=sum(1 for path in visible_paths if not snapshot.nodes[path].children),
    )


def _build_groups(
    snapshot: FileTreeSnapshot,
    graph_nodes: list[GraphNode],
    branch_order: dict[str, int],
) -> list[GraphGroup]:
    by_path = {node.path: node for node in graph_nodes}
    groups: list[GraphGroup] = []

    for graph_node in graph_nodes:
        source = snapshot.nodes[graph_node.path]
        if source.kind is not NodeKind.DIRECTORY:
            continue
        if source.path not in branch_order:
            continue
        descendant_paths = _visible_descendants(snapshot, source.path, by_path)
        visible_children = [by_path[path] for path in descendant_paths]
        if len(visible_children) < 2:
            continue

        min_x = min(child.x for child in visible_children)
        min_y = min(child.y for child in visible_children)
        max_x = max(child.x + child.width for child in visible_children)
        max_y = max(child.y + child.height for child in visible_children)
        padding_x = 42.0
        padding_top = 82.0
        padding_bottom = 34.0
        groups.append(
            GraphGroup(
                path=source.path,
                name=source.name,
                x=min_x - padding_x,
                y=min_y - padding_top,
                width=(max_x - min_x) + padding_x * 2,
                height=(max_y - min_y) + padding_top + padding_bottom,
                depth=source.depth,
                child_count=len(source.children),
                mtime=source.mtime,
                child_paths=tuple(child.path for child in visible_children),
                branch_key=source.path,
                branch_index=branch_order.get(source.path, 0),
            )
        )
    return groups


def _visible_descendants(snapshot: FileTreeSnapshot, path: str, by_path: dict[str, GraphNode]) -> list[str]:
    found: list[str] = []
    queue = [child for child in snapshot.nodes[path].children if child in by_path]
    while queue:
        child = queue.pop(0)
        found.append(child)
        queue.extend(grandchild for grandchild in snapshot.nodes[child].children if grandchild in by_path)
    return found


def _collect_visible_paths(snapshot: FileTreeSnapshot, max_nodes: int) -> list[str]:
    visible: list[str] = []
    queue = [snapshot.root]
    while queue and len(visible) < max_nodes:
        path = queue.pop(0)
        visible.append(path)
        node = snapshot.nodes[path]
        directories = [child for child in node.children if snapshot.nodes[child].kind is NodeKind.DIRECTORY]
        files = [child for child in node.children if snapshot.nodes[child].kind is NodeKind.FILE]
        queue.extend(directories + files)
    return visible


def _choose_branch_roots(snapshot: FileTreeSnapshot, root: str, visible_set: set[str]) -> list[str]:
    candidates = [child for child in snapshot.nodes[root].children if child in visible_set]

    for _ in range(4):
        directories = [path for path in candidates if snapshot.nodes[path].kind is NodeKind.DIRECTORY]
        files = [path for path in candidates if snapshot.nodes[path].kind is NodeKind.FILE]
        meaningful_dirs = [path for path in directories if len(snapshot.nodes[path].children) >= 2]
        if len(meaningful_dirs) >= 3:
            return meaningful_dirs + files[:4]
        if len(directories) != 1:
            break
        only_dir = directories[0]
        expanded = [child for child in snapshot.nodes[only_dir].children if child in visible_set]
        if not expanded:
            break
        candidates = expanded + files

    expanded_dirs: list[str] = []
    files: list[str] = []
    for path in candidates:
        node = snapshot.nodes[path]
        if node.kind is NodeKind.FILE:
            files.append(path)
        else:
            child_dirs = [child for child in node.children if child in visible_set and snapshot.nodes[child].kind is NodeKind.DIRECTORY]
            if len(child_dirs) >= 2:
                expanded_dirs.extend(child_dirs)
            else:
                expanded_dirs.append(path)
    if expanded_dirs:
        return expanded_dirs + files[:4]
    return candidates


def _place_context_nodes(
    snapshot: FileTreeSnapshot,
    visible_set: set[str],
    branch_roots: list[str],
    parent_map: dict[str, str],
    add_node,
) -> None:
    context: list[str] = []
    branch_set = set(branch_roots)
    for branch in branch_roots:
        parts: list[str] = []
        current = branch
        while current in snapshot.nodes and current != snapshot.root:
            parent = parent_map.get(current)
            if parent is None or parent == snapshot.root:
                break
            if parent not in branch_set:
                parts.append(parent)
            current = parent
        context.extend(reversed(parts))

    unique_context: list[str] = []
    for path in context:
        if path in visible_set and path not in unique_context:
            unique_context.append(path)

    ribbon = unique_context[:6]
    root_files = [
        child
        for child in snapshot.nodes[snapshot.root].children
        if child in visible_set and snapshot.nodes[child].kind is NodeKind.FILE
    ][:6]

    ribbon.extend(root_files)
    if not ribbon:
        return

    columns = min(4, max(2, len(ribbon)))
    start_x = 130.0
    step_x = 370.0
    first_row_y = 210.0
    second_row_y = 350.0
    for index, path in enumerate(ribbon):
        row = index // columns
        col = index % columns
        y = first_row_y if row == 0 else second_row_y
        add_node(path, start_x + col * step_x, y)


def _build_parent_map(snapshot: FileTreeSnapshot) -> dict[str, str]:
    parent_map: dict[str, str] = {}
    for path, node in snapshot.nodes.items():
        for child in node.children:
            parent_map[child] = path
    return parent_map


def _nearest_branch(path: str, parent_map: dict[str, str], branch_order: dict[str, int], root: str) -> str:
    current = path
    while current != root:
        if current in branch_order:
            return current
        parent = parent_map.get(current)
        if parent is None:
            break
        current = parent
    return root


