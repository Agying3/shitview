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
                self.padding_x = 42.0
                self.padding_top = 82.0
                self.padding_bottom = 34.0
                self.branch_index = graph_group.branch_index
                self.setBrush(QColor(31, 35, 43, 62))
                border = QColor(_group_border(graph_group))
                border.setAlpha(138)
                pen = QPen(border, 1.15)
                pen.setStyle(Qt.PenStyle.SolidLine)
                self.setPen(pen)
                self.setZValue(-20 + graph_group.depth)

                self.title = QGraphicsSimpleTextItem(_shorten(graph_group.name, 28), self)
                title_font = QFont("Segoe UI", 9)
                title_font.setPixelSize(12)
                title_font.setBold(True)
                self.title.setFont(title_font)
                self.title.setBrush(QColor(_group_accent(graph_group)))
                self.title.setFlag(self.title.GraphicsItemFlag.ItemIgnoresTransformations, True)

                self.meta = QGraphicsSimpleTextItem("", self)
                meta_font = QFont("Segoe UI", 8)
                meta_font.setPixelSize(10)
                self.meta.setFont(meta_font)
                self.meta.setBrush(QColor("#a0aec0"))
                self.meta.setFlag(self.meta.GraphicsItemFlag.ItemIgnoresTransformations, True)
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
                path.addRoundedRect(rect, 18, 18)
                glass = QLinearGradient(rect.topLeft(), rect.bottomRight())
                glass.setColorAt(0.0, QColor(255, 255, 255, 16))
                glass.setColorAt(0.18, QColor(48, 56, 70, 72))
                glass.setColorAt(1.0, QColor(20, 24, 31, 54))
                self.setBrush(glass)
                self.current_rect = rect
                self.setPath(path)
                self.title.setPos(rect.x() + 18, rect.y() + 15)
                self.meta.setText(f"{self.group.child_count} items - modified {format_relative_time(self.group.mtime)}")
                self.meta.setPos(rect.x() + 18, rect.y() + 36)
                if notify and self.on_bounds_changed is not None:
                    self.on_bounds_changed(self)

        return _GraphGroupItem(group, child_items, on_bounds_changed)


class GraphEdgeItem:
    def __new__(cls, source, target):
        from PySide6.QtCore import Qt
        from PySide6.QtGui import QColor, QPainterPath, QPen
        from PySide6.QtWidgets import QGraphicsPathItem

        class _GraphEdgeItem(QGraphicsPathItem):
            def __init__(self, source_item, target_item) -> None:
                super().__init__()
                self.source_item = source_item
                self.target_item = target_item
                pen = QPen(QColor(93, 113, 142, 132), 1.65)
                pen.setCapStyle(Qt.PenCapStyle.RoundCap)
                pen.setJoinStyle(Qt.PenJoinStyle.RoundJoin)
                self.setPen(pen)
                self.setZValue(-30)
                self.update_path()

            def update_path(self) -> None:
                source = self.source_item.sceneBoundingRect()
                target = self.target_item.sceneBoundingRect()
                start_x = source.right()
                start_y = source.center().y()
                end_x = target.left()
                end_y = target.center().y()
                handle = max(48.0, abs(end_x - start_x) * 0.42)
                path = QPainterPath()
                path.moveTo(start_x, start_y)
                path.cubicTo(start_x + handle, start_y, end_x - handle, end_y, end_x, end_y)
                self.setPath(path)

        return _GraphEdgeItem(source, target)


class GraphNodeItem:
    def __new__(cls, node: GraphNode, on_select=None, on_move=None, on_position_changed=None):
        from PySide6.QtCore import QEasingCurve, QRectF, QVariantAnimation
        from PySide6.QtGui import QColor, QFont, QPainterPath, QPen
        from PySide6.QtWidgets import QGraphicsDropShadowEffect, QGraphicsItem, QGraphicsPathItem, QGraphicsSimpleTextItem

        class _GraphNodeItem(QGraphicsPathItem):
            def __init__(
                self,
                graph_node: GraphNode,
                select_callback=None,
                move_callback=None,
                position_changed_callback=None,
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
                self.shadow = QGraphicsDropShadowEffect()
                self.shadow.setBlurRadius(18)
                self.shadow.setOffset(0, 7)
                self.shadow.setColor(QColor(0, 0, 0, 92))
                self.setGraphicsEffect(self.shadow)
                self.hover_animation = QVariantAnimation()
                self.hover_animation.setDuration(150)
                self.hover_animation.setEasingCurve(QEasingCurve.Type.OutCubic)
                self.hover_animation.valueChanged.connect(self._set_animated_scale)
                self._add_contents()
                self.setTransformOriginPoint(graph_node.width / 2, graph_node.height / 2)

            def add_edge(self, edge) -> None:
                self.edges.append(edge)

            def add_group(self, group) -> None:
                self.groups.append(group)

            def update_relations(self, notify: bool = True) -> None:
                for edge in self.edges:
                    edge.update_path()
                for group in self.groups:
                    group.update_bounds(notify=notify)

            def itemChange(self, change, value):
                if change == QGraphicsItem.GraphicsItemChange.ItemPositionHasChanged:
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

            def hoverEnterEvent(self, event) -> None:
                self._animate_scale(1.055)
                self.setZValue(38)
                self.setPen(QPen(QColor("#60a5fa"), 1.8))
                self.shadow.setBlurRadius(30)
                self.shadow.setOffset(0, 10)
                glow = QColor(_accent(self.node))
                glow.setAlpha(96)
                self.shadow.setColor(glow)
                self.update_relations()
                super().hoverEnterEvent(event)

            def hoverLeaveEvent(self, event) -> None:
                self._animate_scale(1.0)
                self.setZValue(20)
                self.setPen(QPen(QColor(_node_border(self.node)), 1.2))
                self.shadow.setBlurRadius(18)
                self.shadow.setOffset(0, 7)
                self.shadow.setColor(QColor(0, 0, 0, 92))
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

                title = QGraphicsSimpleTextItem(_shorten(self.node.name, 32), self)
                title_font = QFont("Segoe UI", 9)
                title_font.setPixelSize(12)
                title_font.setBold(self.node.kind is NodeKind.DIRECTORY)
                title.setFont(title_font)
                title.setBrush(QColor("#e7eef9"))
                title.setFlag(title.GraphicsItemFlag.ItemIgnoresTransformations, True)
                title.setPos(22, 16)
                title.setZValue(22)

                meta_text = f"{_node_meta(self.node)} - {format_relative_time(self.node.mtime)}"
                meta = QGraphicsSimpleTextItem(meta_text, self)
                meta_font = QFont("Segoe UI", 8)
                meta_font.setPixelSize(10)
                meta.setFont(meta_font)
                meta.setBrush(QColor("#8ea2bd"))
                meta.setFlag(meta.GraphicsItemFlag.ItemIgnoresTransformations, True)
                meta.setPos(22, self.node.height - 34)
                meta.setZValue(22)

                if self.node.kind is NodeKind.DIRECTORY:
                    badge = QGraphicsSimpleTextItem("DIR", self)
                    badge_font = QFont("Segoe UI", 7)
                    badge_font.setPixelSize(9)
                    badge.setFont(badge_font)
                    badge.setBrush(QColor(_accent(self.node)))
                    badge.setFlag(badge.GraphicsItemFlag.ItemIgnoresTransformations, True)
                    badge.setPos(self.node.width - 38, 14)
                    badge.setZValue(22)

        return _GraphNodeItem(node, on_select, on_move, on_position_changed)


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


def _group_border(group: GraphGroup) -> str:
    palette = ["#4c8cff", "#38bdf8", "#a855f7", "#fb923c", "#34d399", "#f472b6"]
    return palette[group.branch_index % len(palette)]


def _group_accent(group: GraphGroup) -> str:
    palette = ["#93c5fd", "#67e8f9", "#d8b4fe", "#fdba74", "#6ee7b7", "#f9a8d4"]
    return palette[group.branch_index % len(palette)]


