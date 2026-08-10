from __future__ import annotations

from dataclasses import dataclass
from collections import defaultdict
from typing import Callable

from shitview.core.graph_layout import GraphGroup, GraphLayout, build_layered_graph
from shitview.core.models import FileTreeSnapshot
from shitview.ui.graph_items import GraphEdgeItem, GraphGroupItem, GraphNodeItem


@dataclass
class MapCanvas:
    view: object
    on_select: Callable[[str], None] | None = None
    layout_store: object | None = None

    def render_snapshot(self, snapshot: FileTreeSnapshot) -> None:
        from PySide6.QtCore import Qt

        layout = build_layered_graph(snapshot)
        self.group_items = []
        self.node_items = {}
        self._resolving_layout = False
        self._position_change_counter = 0
        scene = self._make_scene(layout)
        self._draw_background(scene, layout)
        node_items = self._draw_nodes(scene, layout)
        self._bind_tree_relations(layout, node_items)
        self._draw_groups(scene, layout, node_items)
        self._draw_edges(scene, layout, node_items)
        self.view.setScene(scene)
        scene.setSceneRect(scene.itemsBoundingRect().adjusted(-120.0, -120.0, 120.0, 120.0))
        self.view.resetTransform()
        focus_rect = scene.itemsBoundingRect().adjusted(-90.0, -90.0, 90.0, 90.0)
        self.view.fitInView(focus_rect, Qt.AspectRatioMode.KeepAspectRatio)
        self.view.centerOn(focus_rect.center())

    def _make_scene(self, layout: GraphLayout):
        from PySide6.QtGui import QColor
        from PySide6.QtWidgets import QGraphicsScene

        scene = QGraphicsScene()
        scene.setSceneRect(0, 0, layout.width, layout.height)
        scene.setBackgroundBrush(QColor("#1b1e23"))
        return scene

    def _draw_background(self, scene, layout: GraphLayout) -> None:
        from PySide6.QtCore import QRectF
        from PySide6.QtGui import QColor, QLinearGradient, QPainterPath, QPen
        from PySide6.QtWidgets import QGraphicsPathItem

        panel_path = QPainterPath()
        panel_rect = QRectF(18, 18, layout.width - 36, layout.height - 36)
        panel_path.addRoundedRect(panel_rect, 14, 14)
        panel = QGraphicsPathItem(panel_path)
        panel_gradient = QLinearGradient(panel_rect.topLeft(), panel_rect.bottomRight())
        panel_gradient.setColorAt(0.0, QColor(52, 59, 72, 96))
        panel_gradient.setColorAt(0.42, QColor(33, 37, 45, 80))
        panel_gradient.setColorAt(1.0, QColor(22, 25, 31, 104))
        panel.setBrush(panel_gradient)
        panel.setPen(QPen(QColor(121, 137, 165, 48), 1.0))
        panel.setZValue(-120)
        scene.addItem(panel)

        top_line = QPainterPath()
        top_line.moveTo(panel_rect.left() + 22, panel_rect.top() + 18)
        top_line.lineTo(panel_rect.right() - 22, panel_rect.top() + 18)
        top_item = QGraphicsPathItem(top_line)
        top_item.setPen(QPen(QColor(255, 255, 255, 34), 1.0))
        top_item.setZValue(-118)
        scene.addItem(top_item)

        band_pen = QPen(QColor(121, 137, 165, 10), 1.0)
        for index in range(4):
            y = panel_rect.top() + 180 + index * 260
            if y >= panel_rect.bottom() - 80:
                break
            band = QPainterPath()
            band.moveTo(panel_rect.left() + 34, y)
            band.cubicTo(
                panel_rect.left() + panel_rect.width() * 0.32,
                y - 34,
                panel_rect.left() + panel_rect.width() * 0.66,
                y + 34,
                panel_rect.right() - 34,
                y,
            )
            band_item = QGraphicsPathItem(band)
            band_item.setPen(band_pen)
            band_item.setZValue(-116)
            scene.addItem(band_item)

    def _draw_groups(self, scene, layout: GraphLayout, node_items: dict[str, object]) -> None:
        for group in sorted(layout.groups, key=lambda item: item.depth):
            child_items = [node_items[path] for path in group.child_paths if path in node_items]
            group_item = GraphGroupItem(group, child_items, on_bounds_changed=self._on_group_bounds_changed)
            group_item.on_bounds_changed = self._on_group_bounds_changed
            self.group_items.append(group_item)
            for child_item in child_items:
                child_item.add_group(group_item)
            scene.addItem(group_item)
        if len(node_items) <= 180:
            self._resolve_layout_conflicts(max_iterations=4, max_push=74.0, include_groups=False)

    def _draw_nodes(self, scene, layout: GraphLayout) -> dict[str, object]:
        items = {}
        saved_positions = self.layout_store.load_positions() if self.layout_store is not None else {}
        for node in layout.nodes:
            item = GraphNodeItem(
                node,
                on_select=self.on_select,
                on_move=self._on_node_moved,
                on_position_changed=self._on_node_position_changed,
                keep_title_readable=len(layout.nodes) <= 120,
            )
            if node.path in saved_positions:
                x, y = saved_positions[node.path]
                item.setPos(x, y)
            scene.addItem(item)
            items[node.path] = item
        self.node_items = items
        return items

    def _bind_tree_relations(self, layout: GraphLayout, node_items: dict[str, object]) -> None:
        for edge in layout.edges:
            source = node_items.get(edge.source)
            target = node_items.get(edge.target)
            if source is not None and target is not None:
                source.add_child_node(target)

    def _on_node_moved(self, node_path: str, x: float, y: float) -> None:
        if self.layout_store is not None:
            self.layout_store.save_position(node_path, x, y)

    def _on_node_position_changed(self, item) -> None:
        if self._resolving_layout:
            return
        if getattr(item, "_syncing_tree", False):
            return
        self._position_change_counter = (getattr(self, "_position_change_counter", 0) + 1) % 3
        if self._position_change_counter != 0:
            return
        self._resolve_layout_conflicts(changed_item=item, max_iterations=5, max_push=42.0, include_groups=True)

    def _draw_edges(self, scene, layout: GraphLayout, node_items: dict[str, object]) -> None:
        obstacle_items = list(node_items.values())
        edge_obstacles = (
            [(item, item.sceneBoundingRect().adjusted(-14.0, -14.0, 14.0, 14.0)) for item in obstacle_items]
            if len(obstacle_items) > 180
            else obstacle_items
        )
        for edge_index, edge in enumerate(layout.edges):
            source = node_items.get(edge.source)
            target = node_items.get(edge.target)
            if source is None or target is None:
                continue
            edge_item = GraphEdgeItem(source, target, obstacle_items=edge_obstacles, lane_index=edge_index)
            source.add_edge(edge_item)
            target.add_edge(edge_item)
            scene.addItem(edge_item)

    def _viewport_rect(self, layout: GraphLayout):
        from PySide6.QtCore import QRectF

        items = [QRectF(node.x, node.y, node.width, node.height) for node in layout.nodes]
        items.extend(QRectF(group.x, group.y, group.width, group.height) for group in layout.groups)
        if not items:
            return QRectF(24.0, 24.0, layout.width - 48.0, layout.height - 48.0)
        rect = items[0]
        for other in items[1:]:
            rect = rect.united(other)
        leaf_factor = max(1.0, min(4.0, layout.leaf_count / 8.0))
        pad_x = 56.0 + leaf_factor * 18.0
        pad_y = 56.0 + leaf_factor * 18.0
        return rect.adjusted(-pad_x, -pad_y, pad_x, pad_y)

    def _readable_focus_rect(self, bounds, node_count: int):
        from PySide6.QtCore import QRectF

        if node_count <= 80:
            max_width = 3200.0
            max_height = 1900.0
        elif node_count <= 220:
            max_width = 5000.0
            max_height = 3100.0
        else:
            max_width = 7000.0
            max_height = 4300.0

        width = min(bounds.width() + 180.0, max_width)
        height = min(bounds.height() + 180.0, max_height)
        return QRectF(bounds.left() - 90.0, bounds.top() - 90.0, width, height)

    def _on_group_bounds_changed(self, changed_group) -> None:
        if self._resolving_layout:
            return
        self._resolve_layout_conflicts(changed_item=changed_group, include_groups=True)

    def _resolve_layout_conflicts(
        self,
        changed_item=None,
        max_iterations: int = 8,
        max_push: float = 84.0,
        include_groups: bool = True,
    ) -> None:
        if not getattr(self, "group_items", None) and not getattr(self, "node_items", None):
            return
        colliders = self._collision_items(include_groups=include_groups)
        if not colliders:
            return
        if changed_item is not None:
            max_iterations = min(max_iterations, 4)
        elif len(colliders) > 300:
            max_iterations = min(max_iterations, 4)
        elif len(colliders) > 180:
            max_iterations = min(max_iterations, 4)
        self._resolving_layout = True
        try:
            if changed_item is not None:
                self._resolve_local_conflicts(colliders, changed_item, max_iterations, max_push, animated=True)
            else:
                self._resolve_global_conflicts(colliders, max_iterations, max_push)
        finally:
            self._resolving_layout = False

    def _resolve_global_conflicts(self, colliders: list[object], max_iterations: int, max_push: float) -> None:
        for _ in range(max_iterations):
            moved = False
            spatial_index = _SpatialIndex(colliders)
            for current, other in _candidate_pairs(colliders, spatial_index):
                if self._separate_pair(current, other, max_push=max_push):
                    moved = True
            if not moved:
                break

    def _resolve_local_conflicts(
        self,
        colliders: list[object],
        changed_item,
        max_iterations: int,
        max_push: float,
        animated: bool = False,
    ) -> None:
        affected = {changed_item}
        for _ in range(max_iterations):
            if not affected:
                break
            moved_items: set[object] = set()
            spatial_index = _SpatialIndex(colliders)
            seen: set[tuple[int, int]] = set()
            for current in list(affected):
                for other in spatial_index.nearby(current):
                    key = _pair_key(current, other)
                    if key in seen:
                        continue
                    seen.add(key)
                    moved = self._separate_pair(current, other, max_push=max_push, preferred=current, animated=animated)
                    moved_items.update(moved)
            affected = moved_items

    def _separate_pair(self, current, other, max_push: float, preferred=None, animated: bool = False) -> set[object]:
        if not _should_separate(current, other):
            return set()
        current_rect = _item_rect(current)
        other_rect = _item_rect(other)
        overlap = current_rect.intersected(other_rect)
        if overlap.isNull() or overlap.width() <= 0 or overlap.height() <= 0:
            return set()
        dx, dy = _separation_delta(current_rect, other_rect, max_push=max_push)
        return self._push_items(current, other, dx, dy, preferred=preferred, animated=animated)

    def _collision_items(self, include_groups: bool = True) -> list[object]:
        items = list(getattr(self, "node_items", {}).values())
        if include_groups:
            items.extend(getattr(self, "group_items", []))
        return items

    def _push_items(self, current, other, dx: float, dy: float, preferred=None, animated: bool = False) -> set[object]:
        current_is_group = _is_group_item(current)
        other_is_group = _is_group_item(other)
        if current_is_group != other_is_group:
            if preferred is current:
                target = other
                move_dx, move_dy = -dx, -dy
            elif preferred is other:
                target = current
                move_dx, move_dy = dx, dy
            else:
                target = other if current_is_group else current
                move_dx = -dx if current_is_group else dx
                move_dy = -dy if current_is_group else dy
            self._apply_item_motion(target, move_dx, move_dy, animated=animated)
            return {target}
        if preferred is current:
            self._apply_item_motion(other, -dx, -dy, animated=animated)
            return {other}
        if preferred is other:
            self._apply_item_motion(current, dx, dy, animated=animated)
            return {current}
        if not current_is_group and not other_is_group:
            self._apply_item_motion(current, dx * 0.5, dy * 0.5, animated=animated)
            self._apply_item_motion(other, -dx * 0.5, -dy * 0.5, animated=animated)
            return {current, other}
        if current_is_group and other_is_group:
            self._apply_item_motion(current, dx * 0.5, dy * 0.5, animated=animated)
            self._apply_item_motion(other, -dx * 0.5, -dy * 0.5, animated=animated)
            return {current, other}
        return set()

    def _apply_item_motion(self, item, dx: float, dy: float, animated: bool = False) -> None:
        if abs(dx) < 0.35 and abs(dy) < 0.35:
            return
        if _is_group_item(item):
            for child in item.child_items:
                if animated and hasattr(child, "animate_to"):
                    child.animate_to(child.pos().x() + dx, child.pos().y() + dy)
                else:
                    if hasattr(child, "_syncing_tree"):
                        child._syncing_tree = True
                    child.setPos(child.pos().x() + dx, child.pos().y() + dy)
                    if hasattr(child, "_syncing_tree"):
                        child._syncing_tree = False
            item.update_bounds(notify=False)
            for child in item.child_items:
                child.update_relations(notify=False)
            return
        if hasattr(item, "_syncing_tree"):
            item._syncing_tree = True
        if animated and hasattr(item, "animate_to"):
            item.animate_to(item.pos().x() + dx, item.pos().y() + dy)
        else:
            item.setPos(item.pos().x() + dx, item.pos().y() + dy)
        if hasattr(item, "_syncing_tree"):
            item._syncing_tree = False
        item.update_relations(notify=False)


def _item_rect(item):
    if hasattr(item, "current_rect"):
        return item.current_rect
    rect = item.sceneBoundingRect()
    if hasattr(item, "node"):
        return rect.adjusted(-22.0, -18.0, 22.0, 18.0)
    return rect


def _is_group_item(item) -> bool:
    group_attr = getattr(item, "group", None)
    return group_attr is not None and not callable(group_attr) and hasattr(item, "current_rect")


class _SpatialIndex:
    def __init__(self, items: list[object], cell_size: float = 520.0) -> None:
        self.cell_size = cell_size
        self.cells: dict[tuple[int, int], list[object]] = defaultdict(list)
        self.item_cells: dict[int, set[tuple[int, int]]] = {}
        for item in items:
            cells = set(self._cells_for_rect(_item_rect(item)))
            self.item_cells[id(item)] = cells
            for cell in cells:
                self.cells[cell].append(item)

    def nearby(self, item) -> list[object]:
        found: dict[int, object] = {}
        for cell_x, cell_y in self.item_cells.get(id(item), set()):
            for x in range(cell_x - 1, cell_x + 2):
                for y in range(cell_y - 1, cell_y + 2):
                    for other in self.cells.get((x, y), []):
                        if other is not item:
                            found[id(other)] = other
        return list(found.values())

    def _cells_for_rect(self, rect):
        left = int(rect.left() // self.cell_size)
        right = int(rect.right() // self.cell_size)
        top = int(rect.top() // self.cell_size)
        bottom = int(rect.bottom() // self.cell_size)
        for x in range(left, right + 1):
            for y in range(top, bottom + 1):
                yield (x, y)


def _candidate_pairs(items: list[object], spatial_index: _SpatialIndex):
    seen: set[tuple[int, int]] = set()
    for current in items:
        current_id = id(current)
        for other in spatial_index.nearby(current):
            key = _pair_key(current, other)
            if key in seen:
                continue
            seen.add(key)
            yield current, other


def _pair_key(first, second) -> tuple[int, int]:
    first_id = id(first)
    second_id = id(second)
    return (first_id, second_id) if first_id < second_id else (second_id, first_id)


def _nearby_pairs(changed_item, spatial_index: _SpatialIndex):
    if changed_item is None:
        return
    for other in spatial_index.nearby(changed_item):
        yield changed_item, other


def _share_children(first, second) -> bool:
    if not _is_group_item(first) or not _is_group_item(second):
        return False
    return bool(set(first.child_items).intersection(second.child_items))


def _nested_groups(first, second) -> bool:
    if not _is_group_item(first) or not _is_group_item(second):
        return False
    first_path = getattr(first.group, "path", "").replace("\\", "/")
    second_path = getattr(second.group, "path", "").replace("\\", "/")
    if not first_path or not second_path or first_path == second_path:
        return False
    return second_path.startswith(first_path + "/") or first_path.startswith(second_path + "/")


def _should_separate(first, second) -> bool:
    first_is_group = _is_group_item(first)
    second_is_group = _is_group_item(second)
    if first_is_group and second_is_group:
        return not (_share_children(first, second) or _nested_groups(first, second))
    if first_is_group:
        return second not in first.child_items
    if second_is_group:
        return first not in second.child_items
    return True


def _separation_delta(target_rect, obstacle_rect, max_push: float) -> tuple[float, float]:
    padding = 46.0
    center_delta_x = target_rect.center().x() - obstacle_rect.center().x()
    center_delta_y = target_rect.center().y() - obstacle_rect.center().y()
    overlap = target_rect.intersected(obstacle_rect)
    horizontal_push = overlap.width() + padding
    vertical_push = overlap.height() + padding
    if horizontal_push <= vertical_push:
        direction = 1.0 if center_delta_x >= 0 else -1.0
        return min(max(horizontal_push, 0.0), max_push) * direction, 0.0
    direction = 1.0 if center_delta_y >= 0 else -1.0
    return 0.0, min(max(vertical_push, 0.0), max_push) * direction


