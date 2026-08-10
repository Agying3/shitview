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


def build_layered_graph(snapshot: FileTreeSnapshot, max_nodes: int = 500) -> GraphLayout:
    root = snapshot.get(snapshot.root)
    if root is None:
        return GraphLayout(nodes=(), edges=(), groups=(), width=1200, height=800, leaf_count=0)

    visible_paths = _collect_visible_paths(snapshot, max_nodes=max_nodes)
    visible_set = set(visible_paths)
    child_map = {
        path: [child for child in snapshot.nodes[path].children if child in visible_set]
        for path in visible_paths
    }
    branch_roots = _choose_branch_roots(snapshot, snapshot.root, visible_set) or [snapshot.root]
    branch_order = {path: index for index, path in enumerate(branch_roots)}

    graph_nodes: list[GraphNode] = []
    graph_edges: list[GraphEdge] = []
    placed: set[str] = set()
    max_x = 0.0
    max_y = 0.0

    node_width = 380.0
    node_height = 132.0
    directory_width = 410.0
    directory_height = 144.0
    row_gap = 250.0
    column_gap = 560.0

    def add_node(path: str, x: float, y: float) -> None:
        nonlocal max_x, max_y
        if path in placed:
            return
        source = snapshot.nodes[path]
        width = directory_width if source.kind is NodeKind.DIRECTORY else node_width
        height = directory_height if source.kind is NodeKind.DIRECTORY else node_height
        branch_key = _nearest_visible_branch(path, snapshot.root, child_map, branch_order)
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
        max_x = max(max_x, x + width + 160.0)
        max_y = max(max_y, y + height + 160.0)

    positions: dict[str, tuple[float, float]] = {}
    positions[snapshot.root] = (110.0, 110.0)
    context_paths = [path for path in child_map.get(snapshot.root, []) if path not in branch_order]
    for index, path in enumerate(context_paths[:10]):
        positions[path] = (110.0 + (index % 2) * 560.0, 360.0 + (index // 2) * row_gap)

    island_columns = _island_columns(len(branch_roots))
    island_width = 3600.0
    start_x = 1500.0
    start_y = 140.0
    island_gap_y = 340.0
    column_heights = [start_y for _ in range(island_columns)]
    assigned = set(positions)
    for branch_index, branch in enumerate(branch_roots):
        island_col = min(range(island_columns), key=lambda index: column_heights[index])
        origin_x = start_x + island_col * island_width + (branch_index % 2) * 56.0
        origin_y = column_heights[island_col]
        branch_paths = _branch_visible_paths(branch, child_map, limit=120)
        used_height = _place_branch_island(
            branch_paths,
            child_map,
            positions,
            assigned,
            origin_x,
            origin_y,
            row_gap,
            column_gap,
        )
        column_heights[island_col] = origin_y + used_height + island_gap_y

    loose = [path for path in visible_paths if path not in positions]
    loose_start_y = max(column_heights) + 220.0
    for index, path in enumerate(loose):
        positions[path] = (1500.0 + (index % 5) * column_gap, loose_start_y + (index // 5) * row_gap)

    for path in visible_paths:
        x, y = positions.get(path, (120.0, max_y + row_gap))
        add_node(path, x, y)

    groups = _build_groups(snapshot, graph_nodes, branch_order)
    for path in visible_paths:
        node = snapshot.nodes[path]
        for child in child_map.get(path, []):
            if child in placed:
                graph_edges.append(GraphEdge(source=path, target=child))

    return GraphLayout(
        nodes=tuple(graph_nodes),
        edges=tuple(graph_edges),
        groups=tuple(groups),
        width=max(max_x, 1600.0),
        height=max(max_y, 900.0),
        leaf_count=sum(1 for path in visible_paths if not snapshot.nodes[path].children),
    )


def _branch_visible_paths(branch: str, child_map: dict[str, list[str]], limit: int) -> list[str]:
    found: list[str] = []
    queue = [branch]
    while queue and len(found) < limit:
        path = queue.pop(0)
        found.append(path)
        queue.extend(child_map.get(path, []))
    return found


def _island_columns(count: int) -> int:
    if count <= 2:
        return 1
    if count <= 6:
        return 2
    if count <= 11:
        return 3
    if count <= 15:
        return 4
    return 5


def _place_branch_island(
    branch_paths: list[str],
    child_map: dict[str, list[str]],
    positions: dict[str, tuple[float, float]],
    assigned: set[str],
    origin_x: float,
    origin_y: float,
    row_gap: float,
    column_gap: float,
) -> float:
    if not branch_paths:
        return 0.0
    branch_set = set(branch_paths)
    branch = branch_paths[0]
    positions[branch] = (origin_x + 44.0, origin_y + 40.0)
    assigned.add(branch)

    hub = branch
    hub_y = origin_y + 390.0
    direct_children = [child for child in child_map.get(branch, []) if child in branch_set]
    directory_children = [child for child in direct_children if child_map.get(child)]
    if len(directory_children) == 1 and len(direct_children) <= 3:
        hub = directory_children[0]
        if hub not in assigned:
            positions[hub] = (origin_x + 160.0, origin_y + 470.0)
            assigned.add(hub)
        module_roots = [child for child in child_map.get(hub, []) if child in branch_set]
        lane_top = origin_y + 820.0
    else:
        module_roots = direct_children
        lane_top = hub_y

    if not module_roots:
        return 520.0

    lane_width = 1080.0
    lane_gap = 180.0
    structured_roots = [module for module in module_roots if child_map.get(module)]
    flat_roots = [module for module in module_roots if not child_map.get(module)]
    lane_columns = min(3, max(1, len(structured_roots)))
    lane_heights = [lane_top for _ in range(lane_columns)]
    max_used_y = lane_top
    for module_index, module in enumerate(structured_roots):
        if module in assigned:
            continue
        lane_col = min(range(lane_columns), key=lambda index: lane_heights[index])
        lane_x = origin_x + 80.0 + lane_col * (lane_width + lane_gap) + (54.0 if module_index % 2 else 0.0)
        lane_y = lane_heights[lane_col]
        positions[module] = (lane_x, lane_y)
        assigned.add(module)

        subtree = [path for path in _subtree_paths(module, child_map, branch_set) if path not in assigned]
        inner_columns = 2 if len(subtree) >= 7 else 1
        for index, path in enumerate(subtree):
            col = index % inner_columns
            row = index // inner_columns
            positions[path] = (lane_x + col * column_gap, lane_y + 260.0 + row * row_gap)
            assigned.add(path)
        rows = (len(subtree) + inner_columns - 1) // inner_columns
        used_height = 260.0 + max(1, rows) * row_gap + 150.0
        lane_heights[lane_col] = lane_y + used_height + 190.0
        max_used_y = max(max_used_y, lane_heights[lane_col])

    flat_roots = [path for path in flat_roots if path not in assigned]
    if flat_roots:
        grid_y = max_used_y + (170.0 if structured_roots else 0.0)
        columns = _flat_columns(len(flat_roots))
        for index, path in enumerate(flat_roots):
            col = index % columns
            row = index // columns
            positions[path] = (origin_x + 80.0 + col * column_gap, grid_y + row * row_gap)
            assigned.add(path)
        rows = (len(flat_roots) + columns - 1) // columns
        max_used_y = max(max_used_y, grid_y + rows * row_gap + 150.0)

    unassigned = [path for path in branch_paths if path not in assigned]
    for index, path in enumerate(unassigned):
        positions[path] = (origin_x + 80.0 + (index % 4) * column_gap, max_used_y + (index // 4) * row_gap)
        assigned.add(path)
    if unassigned:
        max_used_y += ((len(unassigned) + 3) // 4) * row_gap
    return max(740.0, max_used_y - origin_y + 140.0)


def _flat_columns(count: int) -> int:
    if count <= 1:
        return 1
    if count <= 4:
        return count
    if count >= 20:
        return 5
    return 4


def _subtree_paths(root: str, child_map: dict[str, list[str]], branch_set: set[str]) -> list[str]:
    found: list[str] = []
    queue = [child for child in child_map.get(root, []) if child in branch_set]
    while queue:
        path = queue.pop(0)
        found.append(path)
        queue.extend(child for child in child_map.get(path, []) if child in branch_set)
    return found


def _nearest_visible_branch(
    path: str,
    root: str,
    child_map: dict[str, list[str]],
    branch_order: dict[str, int],
) -> str:
    if path in branch_order:
        return path
    if path == root:
        return root
    for branch in branch_order:
        queue = list(child_map.get(branch, []))
        while queue:
            child = queue.pop(0)
            if child == path:
                return branch
            queue.extend(child_map.get(child, []))
    return root


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
        if source.path == snapshot.root:
            continue
        if source.depth > 4:
            continue
        visible_direct_children = [child for child in source.children if child in by_path]
        if source.path not in branch_order and len(visible_direct_children) == 1:
            only_child = snapshot.nodes[visible_direct_children[0]]
            if only_child.kind is NodeKind.DIRECTORY:
                continue
        descendant_paths = [source.path] + _visible_descendants(snapshot, source.path, by_path)
        if source.path not in branch_order and len(descendant_paths) < 4:
            continue
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
                branch_key=graph_node.branch_key,
                branch_index=graph_node.branch_index,
            )
        )
    return _filter_nested_groups(groups)


def _filter_nested_groups(groups: list[GraphGroup]) -> list[GraphGroup]:
    leaf_groups: list[GraphGroup] = []
    normalized = [(group, group.path.replace("\\", "/")) for group in groups]
    for group, path in normalized:
        has_child_group = any(
            other is not group and other_path.startswith(path + "/")
            for other, other_path in normalized
        )
        if not has_child_group:
            leaf_groups.append(group)
    return leaf_groups


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
        meaningful_dirs = [path for path in directories if _visible_descendant_count(snapshot, path, visible_set) >= 2]
        if len(meaningful_dirs) >= 3:
            return meaningful_dirs
        if len(directories) != 1:
            break
        only_dir = directories[0]
        expanded = [child for child in snapshot.nodes[only_dir].children if child in visible_set]
        if not expanded:
            break
        candidates = expanded

    expanded_dirs: list[str] = []
    for path in candidates:
        node = snapshot.nodes[path]
        if node.kind is NodeKind.FILE:
            continue
        else:
            child_dirs = [
                child
                for child in node.children
                if child in visible_set
                and snapshot.nodes[child].kind is NodeKind.DIRECTORY
                and _visible_descendant_count(snapshot, child, visible_set) >= 2
            ]
            if len(child_dirs) >= 2:
                expanded_dirs.extend(child_dirs)
            else:
                expanded_dirs.append(path)
    if expanded_dirs:
        return expanded_dirs
    return [path for path in candidates if snapshot.nodes[path].kind is NodeKind.DIRECTORY]


def _visible_descendant_count(snapshot: FileTreeSnapshot, path: str, visible_set: set[str], limit: int = 3) -> int:
    count = 0
    queue = [child for child in snapshot.nodes[path].children if child in visible_set]
    while queue and count < limit:
        child = queue.pop(0)
        count += 1
        queue.extend(grandchild for grandchild in snapshot.nodes[child].children if grandchild in visible_set)
    return count
