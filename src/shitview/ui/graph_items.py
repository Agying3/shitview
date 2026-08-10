from __future__ import annotations

from shitview.core.graph_layout import GraphGroup, GraphNode
from shitview.core.models import NodeKind
from shitview.core.time_format import format_relative_time


class GraphGroupItem:
    def __new__(cls, group: GraphGroup, child_items: list[object], on_bounds_changed=None):
        from PySide6.QtCore import QRectF, Qt
        from PySide6.QtGui import QColor, QFont, QLinearGradient, QPainterPath, QPen
        from PySide6.QtWidgets import QGraphicsPathItem, QGraphicsSimpleTextItem

        class _GraphGroupItem(QGraphicsPathItem):
            def __init__(self, graph_group: GraphGroup, children: list[object], bounds_callback=None) -> None:
                super().__init__()
                self.group = graph_group
                self.child_items = children
                self.on_bounds_changed = bounds_callback
                self.current_rect = QRectF(graph_group.x, graph_group.y, graph_group.width, graph_group.height)
                self.padding_x = 58.0
                self.padding_top = 104.0
                self.padding_bottom = 48.0
                self.branch_index = graph_group.branch_index
                self.setBrush(QColor(31, 35, 43, 96))
                border = QColor(_group_border(graph_group))
                border.setAlpha(188)
                pen = QPen(border, 1.8)
                pen.setStyle(Qt.PenStyle.SolidLine)
                self.setPen(pen)
                self.setZValue(-20 + graph_group.depth)

                self.header = QGraphicsPathItem(self)
                self.header.setZValue(-18 + graph_group.depth)
                self.header.setPen(QPen(QColor(255, 255, 255, 0), 0))
                self.header.setFlag(self.header.GraphicsItemFlag.ItemIgnoresTransformations, True)

                self.rail = QGraphicsPathItem(self)
                self.rail.setZValue(-17 + graph_group.depth)
                self.rail.setPen(QPen(QColor(_group_accent(graph_group)), 2.2))

                self.title = QGraphicsSimpleTextItem(_shorten(graph_group.name, 10), self)
                title_font = QFont("Microsoft YaHei UI", 9)
                title_font.setPixelSize(18)
                title_font.setBold(True)
                self.title.setFont(title_font)
                self.title.setBrush(QColor("#f4f8ff"))
                self.title.setFlag(self.title.GraphicsItemFlag.ItemIgnoresTransformations, True)

                self.meta = QGraphicsSimpleTextItem("", self)
                meta_font = QFont("Microsoft YaHei UI", 8)
                meta_font.setPixelSize(10)
                self.meta.setFont(meta_font)
                self.meta.setBrush(QColor("#9fb2cc"))
                self.update_bounds()

            def update_bounds(self, notify: bool = True) -> None:
                if not self.child_items:
                    rect = QRectF(self.group.x, self.group.y, self.group.width, self.group.height)
                else:
                    child_rect = self.child_items[0].sceneBoundingRect()
                    for child in self.child_items[1:]:
                        child_rect = child_rect.united(child.sceneBoundingRect())
                    rect = QRectF(
                        child_rect.left() - self.padding_x,
                        child_rect.top() - self.padding_top,
                        child_rect.width() + self.padding_x * 2,
                        child_rect.height() + self.padding_top + self.padding_bottom,
                    )

                path = QPainterPath()
                path.addRoundedRect(rect, 22, 22)
                glass = QLinearGradient(rect.topLeft(), rect.bottomRight())
                glass.setColorAt(0.0, QColor(255, 255, 255, 28))
                glass.setColorAt(0.20, QColor(48, 56, 70, 116))
                glass.setColorAt(1.0, QColor(20, 24, 31, 84))
                self.setBrush(glass)
                self.current_rect = rect
                self.setPath(path)

                header_rect = QRectF(0, 0, min(280.0, rect.width() - 28), 54.0)
                header_path = QPainterPath()
                header_path.addRoundedRect(header_rect, 14, 14)
                header_gradient = QLinearGradient(header_rect.topLeft(), header_rect.bottomRight())
                accent = QColor(_group_accent(self.group))
                header_gradient.setColorAt(0.0, QColor(255, 255, 255, 28))
                accent.setAlpha(56)
                header_gradient.setColorAt(0.36, accent)
                header_gradient.setColorAt(1.0, QColor(12, 18, 28, 126))
                self.header.setPath(header_path)
                self.header.setBrush(header_gradient)
                self.header.setPos(rect.x() + 14, rect.y() + 13)

                rail_path = QPainterPath()
                rail_path.moveTo(rect.x() + 25, rect.y() + 68)
                rail_path.lineTo(min(rect.right() - 26, rect.x() + 392), rect.y() + 68)
                self.rail.setPath(rail_path)
                self.title.setPos(rect.x() + 24, rect.y() + 18)
                self.meta.setText(f"{self.group.child_count} items - modified {format_relative_time(self.group.mtime)}")
                self.meta.setPos(rect.x() + 24, rect.y() + 46)
                if notify and self.on_bounds_changed is not None:
                    self.on_bounds_changed(self)

        return _GraphGroupItem(group, child_items, on_bounds_changed)


class GraphEdgeItem:
    def __new__(cls, source, target, obstacle_items=None, lane_index: int = 0):
        from PySide6.QtCore import QRectF, Qt
        from PySide6.QtGui import QColor, QPainterPath, QPen
        from PySide6.QtWidgets import QGraphicsPathItem

        class _GraphEdgeItem(QGraphicsPathItem):
            def __init__(self, source_item, target_item, obstacles, edge_lane: int) -> None:
                super().__init__()
                self.source_item = source_item
                self.target_item = target_item
                self.obstacle_items = obstacles or []
                self.lane_index = edge_lane
                pen = QPen(QColor(70, 244, 162, 172), 1.9)
                pen.setCapStyle(Qt.PenCapStyle.RoundCap)
                pen.setJoinStyle(Qt.PenJoinStyle.RoundJoin)
                self.setPen(pen)
                self.setZValue(-30)
                self.update_path()

            def update_path(self) -> None:
                source = self.source_item.sceneBoundingRect()
                target = self.target_item.sceneBoundingRect()
                self._current_obstacle_rects = self._obstacle_rects()
                start_x = source.right()
                start_y = source.center().y()
                end_x = target.center().x()
                end_y = target.top()
                lane_offset = (self.lane_index % 18) * 18.0
                top_lane = min(source.top(), target.top(), self._obstacle_top()) - 72.0 - lane_offset
                pre_end_y = end_y - 30.0 - (self.lane_index % 4) * 10.0
                exit_x = self._free_exit_x(source, start_y, top_lane)
                enter_x = self._free_enter_x(target, top_lane, pre_end_y)
                path = QPainterPath()
                path.moveTo(start_x, start_y)
                path.lineTo(exit_x, start_y)
                path.lineTo(exit_x, top_lane)
                path.lineTo(enter_x, top_lane)
                path.lineTo(enter_x, pre_end_y)
                path.lineTo(end_x, pre_end_y)
                path.lineTo(end_x, end_y)
                self.setPath(path)
                self._current_obstacle_rects = None

            def _obstacle_top(self) -> float:
                rects = getattr(self, "_current_obstacle_rects", None) or self._obstacle_rects()
                if not rects:
                    return min(self.source_item.sceneBoundingRect().top(), self.target_item.sceneBoundingRect().top())
                return min(rect.top() for rect in rects)

            def _free_exit_x(self, source, start_y: float, top_lane: float) -> float:
                for step in range(12):
                    candidate = source.right() + 28.0 + step * 34.0
                    if self._clear_segment(source.right(), start_y, candidate, start_y) and self._clear_segment(candidate, start_y, candidate, top_lane):
                        return candidate
                return source.right() + 120.0 + (self.lane_index % 8) * 24.0

            def _free_enter_x(self, target, top_lane: float, pre_end_y: float) -> float:
                offsets = [0.0, -54.0, 54.0, -108.0, 108.0, -162.0, 162.0, -240.0, 240.0, -330.0, 330.0, -450.0, 450.0, -600.0, 600.0]
                for offset in offsets:
                    candidate = target.center().x() + offset
                    if self._clear_segment(candidate, top_lane, candidate, pre_end_y) and self._clear_segment(candidate, pre_end_y, target.center().x(), pre_end_y):
                        return candidate
                return target.center().x()

            def _clear_segment(self, x1: float, y1: float, x2: float, y2: float) -> bool:
                left = min(x1, x2) - 5.0
                top = min(y1, y2) - 5.0
                width = abs(x2 - x1) + 10.0
                height = abs(y2 - y1) + 10.0
                segment = QRectF(left, top, max(width, 10.0), max(height, 10.0))
                rects = getattr(self, "_current_obstacle_rects", None) or self._obstacle_rects()
                return not any(segment.intersects(rect) for rect in rects)

            def _obstacle_rects(self) -> list:
                rects = []
                for entry in self.obstacle_items:
                    if isinstance(entry, tuple) and len(entry) == 2:
                        item, rect = entry
                        if item is self.source_item or item is self.target_item:
                            continue
                        rects.append(rect)
                        continue
                    item = entry
                    if item is self.source_item or item is self.target_item:
                        continue
                    rects.append(item.sceneBoundingRect().adjusted(-14.0, -14.0, 14.0, 14.0))
                return rects

        return _GraphEdgeItem(source, target, obstacle_items, lane_index)


class GraphNodeItem:
    def __new__(cls, node: GraphNode, on_select=None, on_move=None, on_position_changed=None, keep_title_readable: bool = False):
        from PySide6.QtCore import QEasingCurve, QPointF, QRectF, QVariantAnimation
        from PySide6.QtGui import QColor, QFont, QPainterPath, QPen
        from PySide6.QtWidgets import QGraphicsItem, QGraphicsPathItem, QGraphicsSimpleTextItem

        class _GraphNodeItem(QGraphicsPathItem):
            def __init__(
                self,
                graph_node: GraphNode,
                select_callback=None,
                move_callback=None,
                position_changed_callback=None,
                readable_title: bool = False,
            ) -> None:
                path = QPainterPath()
                path.addRoundedRect(QRectF(0, 0, graph_node.width, graph_node.height), 12, 12)
                super().__init__(path)
                self.node = graph_node
                self.edges = []
                self.groups = []
                self.on_select = select_callback
                self.on_move = move_callback
                self.on_position_changed = position_changed_callback
                self.keep_title_readable = readable_title
                self.child_items = []
                self._last_pos = None
                self._syncing_tree = False
                self.setPos(graph_node.x, graph_node.y)
                self.setBrush(QColor(_node_fill(graph_node)))
                self.setPen(QPen(QColor(_node_border(graph_node)), 1.0 if graph_node.kind is NodeKind.FILE else 1.3))
                self.setAcceptHoverEvents(True)
                self.setFlag(QGraphicsItem.GraphicsItemFlag.ItemIsMovable, True)
                self.setFlag(QGraphicsItem.GraphicsItemFlag.ItemIsSelectable, True)
                self.setFlag(QGraphicsItem.GraphicsItemFlag.ItemSendsGeometryChanges, True)
                self.setTransformOriginPoint(graph_node.width / 2, graph_node.height / 2)
                self.setToolTip(f"{graph_node.path}\nLast modified {format_relative_time(graph_node.mtime)}")
                self.setData(0, graph_node.path)
                self.setZValue(20)
                self.hover_animation = QVariantAnimation()
                self.hover_animation.setDuration(150)
                self.hover_animation.setEasingCurve(QEasingCurve.Type.OutCubic)
                self.hover_animation.valueChanged.connect(self._set_animated_scale)
                self.move_animation = QVariantAnimation()
                self.move_animation.setDuration(150)
                self.move_animation.setEasingCurve(QEasingCurve.Type.OutCubic)
                self.move_animation.valueChanged.connect(self._set_animated_pos)
                self._add_contents()
                self.setTransformOriginPoint(graph_node.width / 2, graph_node.height / 2)

            def add_edge(self, edge) -> None:
                self.edges.append(edge)

            def add_group(self, group) -> None:
                self.groups.append(group)

            def add_child_node(self, child) -> None:
                self.child_items.append(child)

            def animate_to(self, x: float, y: float) -> None:
                self.move_animation.stop()
                self.move_animation.setStartValue(self.pos())
                self.move_animation.setEndValue(QPointF(x, y))
                self.move_animation.start()

            def update_relations(self, notify: bool = True) -> None:
                for edge in self.edges:
                    edge.update_path()
                for group in self.groups:
                    group.update_bounds(notify=notify)

            def itemChange(self, change, value):
                if change == QGraphicsItem.GraphicsItemChange.ItemPositionHasChanged:
                    old_pos = self._last_pos
                    self._last_pos = self.pos()
                    if old_pos is not None and not self._syncing_tree:
                        delta = self.pos() - old_pos
                        if abs(delta.x()) > 0.01 or abs(delta.y()) > 0.01:
                            self._move_descendants(delta)
                    self.update_relations()
                    if self.on_position_changed is not None:
                        self.on_position_changed(self)
                if change == QGraphicsItem.GraphicsItemChange.ItemSelectedHasChanged and value and self.on_select:
                    self.on_select(self.node.path)
                return super().itemChange(change, value)

            def mouseReleaseEvent(self, event) -> None:
                super().mouseReleaseEvent(event)
                if self.on_move is not None:
                    self.on_move(self.node.path, self.pos().x(), self.pos().y())
                    self._save_descendant_positions()

            def _move_descendants(self, delta) -> None:
                for child in self.child_items:
                    child._syncing_tree = True
                    child.setPos(child.pos() + delta)
                    child._syncing_tree = False
                    child.update_relations()
                    child._move_descendants(delta)

            def _save_descendant_positions(self) -> None:
                for child in self.child_items:
                    if child.on_move is not None:
                        child.on_move(child.node.path, child.pos().x(), child.pos().y())
                    child._save_descendant_positions()

            def hoverEnterEvent(self, event) -> None:
                self._animate_scale(1.055)
                self.setZValue(38)
                self.setPen(QPen(QColor("#4af0a4"), 2.0))
                self.update_relations()
                super().hoverEnterEvent(event)

            def hoverLeaveEvent(self, event) -> None:
                self._animate_scale(1.0)
                self.setZValue(20)
                self.setPen(QPen(QColor(_node_border(self.node)), 1.2))
                self.update_relations()
                super().hoverLeaveEvent(event)

            def _animate_scale(self, target: float) -> None:
                self.hover_animation.stop()
                self.hover_animation.setStartValue(float(self.scale()))
                self.hover_animation.setEndValue(target)
                self.hover_animation.start()

            def _set_animated_scale(self, value) -> None:
                self.setScale(float(value))
                self.update_relations(notify=False)

            def _set_animated_pos(self, value) -> None:
                self._syncing_tree = True
                self.setPos(value)
                self._syncing_tree = False
                self.update_relations(notify=False)

            def _add_contents(self) -> None:
                accent_path = QPainterPath()
                accent_path.addRoundedRect(QRectF(0, 0, 7, self.node.height), 3.5, 3.5)
                accent = QGraphicsPathItem(accent_path, self)
                accent.setBrush(QColor(_accent(self.node)))
                accent.setPen(QPen(QColor(_accent(self.node)), 0))
                accent.setZValue(21)

                shine_path = QPainterPath()
                shine_path.addRoundedRect(QRectF(14, 7, self.node.width - 28, 1.4), 0.7, 0.7)
                shine = QGraphicsPathItem(shine_path, self)
                shine.setBrush(QColor(255, 255, 255, 46))
                shine.setPen(QPen(QColor(255, 255, 255, 0), 0))
                shine.setZValue(21)

                title = QGraphicsSimpleTextItem(_node_title(self.node, self.keep_title_readable), self)
                title_font = QFont("Microsoft YaHei UI", 9)
                title_font.setPixelSize(16 if self.keep_title_readable else 16)
                title_font.setBold(self.node.kind is NodeKind.DIRECTORY)
                title.setFont(title_font)
                title.setBrush(QColor("#e7eef9"))
                if self.keep_title_readable:
                    title.setFlag(title.GraphicsItemFlag.ItemIgnoresTransformations, True)
                title.setPos(24, 18)
                title.setZValue(22)

                meta_text = f"{_node_meta(self.node)} - {format_relative_time(self.node.mtime)}"
                meta = QGraphicsSimpleTextItem(meta_text, self)
                meta_font = QFont("Microsoft YaHei UI", 8)
                meta_font.setPixelSize(12)
                meta.setFont(meta_font)
                meta.setBrush(QColor("#8ea2bd"))
                meta.setPos(24, self.node.height - 36)
                meta.setZValue(22)

                if self.node.kind is NodeKind.DIRECTORY:
                    badge = QGraphicsSimpleTextItem("DIR", self)
                    badge_font = QFont("Microsoft YaHei UI", 7)
                    badge_font.setPixelSize(9)
                    badge.setFont(badge_font)
                    badge.setBrush(QColor(_accent(self.node)))
                    badge.setPos(self.node.width - 38, 14)
                    badge.setZValue(22)

        return _GraphNodeItem(node, on_select, on_move, on_position_changed, keep_title_readable)


def _node_fill(node: GraphNode) -> str:
    if node.kind is NodeKind.FILE:
        palette = ["#121a28", "#162131", "#151c29", "#171f30", "#132033", "#182234"]
        return palette[node.branch_index % len(palette)]
    palette = ["#182235", "#1b2833", "#132334", "#201827", "#1d2130", "#1b2438"]
    return palette[node.branch_index % len(palette)]


def _node_border(node: GraphNode) -> str:
    if node.kind is NodeKind.FILE:
        palette = ["#355a82", "#346c7b", "#5a4f8a", "#8a5d3d", "#4b7b52", "#8f4f71"]
        return palette[node.branch_index % len(palette)]
    palette = ["#76b4ff", "#5bd8ff", "#9f7cff", "#ff9d55", "#7ee081", "#f47dbf"]
    return palette[node.branch_index % len(palette)]


def _accent(node: GraphNode) -> str:
    if node.kind is NodeKind.FILE:
        palette = ["#7aa7d9", "#6ed5e5", "#b29df8", "#f9b36f", "#86efac", "#f0a2cb"]
        return palette[node.branch_index % len(palette)]
    palette = ["#3b82f6", "#22d3ee", "#8b5cf6", "#fb923c", "#22c55e", "#d946ef"]
    return palette[node.branch_index % len(palette)]


def _node_meta(node: GraphNode) -> str:
    if node.kind is NodeKind.DIRECTORY:
        return f"{node.child_count} items"
    if node.labels:
        return ", ".join(node.labels[:2])
    if node.size >= 1024:
        return f"{node.size / 1024:.1f} KB"
    return f"{node.size} B"


def _shorten(text: str, limit: int) -> str:
    if len(text) <= limit:
        return text
    return text[: limit - 1] + "..."


def _node_title(node: GraphNode, compact: bool) -> str:
    if not compact:
        return _shorten(node.name, 32)
    if node.kind is NodeKind.DIRECTORY:
        return _shorten(node.name, 8)
    name = node.name
    if "." in name:
        stem, suffix = name.rsplit(".", 1)
        if len(suffix) <= 5:
            return _shorten(stem, 6) + "." + _shorten(suffix, 3)
    return _shorten(name, 9)


def _group_border(group: GraphGroup) -> str:
    palette = ["#4c8cff", "#38bdf8", "#a855f7", "#fb923c", "#34d399", "#f472b6"]
    return palette[group.branch_index % len(palette)]


def _group_accent(group: GraphGroup) -> str:
    palette = ["#93c5fd", "#67e8f9", "#d8b4fe", "#fdba74", "#6ee7b7", "#f9a8d4"]
    return palette[group.branch_index % len(palette)]


