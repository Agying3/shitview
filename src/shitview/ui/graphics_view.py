from __future__ import annotations


class ZoomableGraphicsView:
    def __new__(cls):
        from PySide6.QtCore import Qt
        from PySide6.QtGui import QPainter
        from PySide6.QtWidgets import QGraphicsView

        class _ZoomableGraphicsView(QGraphicsView):
            def __init__(self) -> None:
                super().__init__()
                self.setRenderHint(QPainter.RenderHint.Antialiasing, True)
                self.setRenderHint(QPainter.RenderHint.TextAntialiasing, True)
                self.setCacheMode(QGraphicsView.CacheModeFlag.CacheBackground)
                self.setOptimizationFlag(QGraphicsView.OptimizationFlag.DontSavePainterState, True)
                self.setViewportUpdateMode(QGraphicsView.ViewportUpdateMode.BoundingRectViewportUpdate)
                self.setDragMode(QGraphicsView.DragMode.RubberBandDrag)
                self._panning = False
                self._pan_start = None
                self.setTransformationAnchor(QGraphicsView.ViewportAnchor.AnchorUnderMouse)
                self.setResizeAnchor(QGraphicsView.ViewportAnchor.AnchorViewCenter)
                self.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
                self.setVerticalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)

            def wheelEvent(self, event) -> None:
                factor = 1.12 if event.angleDelta().y() > 0 else 1 / 1.12
                self.scale(factor, factor)

            def mousePressEvent(self, event) -> None:
                item = self.itemAt(event.position().toPoint())
                interactive = self._interactive_item(item)
                if interactive is None and event.button() == Qt.MouseButton.LeftButton:
                    self._panning = True
                    self._pan_start = event.position().toPoint()
                    self.setCursor(Qt.CursorShape.ClosedHandCursor)
                    event.accept()
                    return
                super().mousePressEvent(event)

            def _interactive_item(self, item):
                current = item
                while current is not None:
                    if hasattr(current, "data") and current.data(0) is not None:
                        return current
                    if hasattr(current, "child_items"):
                        return current
                    current = current.parentItem()
                return None

            def mouseMoveEvent(self, event) -> None:
                if self._panning and self._pan_start is not None:
                    delta = event.position().toPoint() - self._pan_start
                    self._pan_start = event.position().toPoint()
                    self.horizontalScrollBar().setValue(self.horizontalScrollBar().value() - delta.x())
                    self.verticalScrollBar().setValue(self.verticalScrollBar().value() - delta.y())
                    event.accept()
                    return
                super().mouseMoveEvent(event)

            def mouseReleaseEvent(self, event) -> None:
                if self._panning and event.button() == Qt.MouseButton.LeftButton:
                    self._panning = False
                    self._pan_start = None
                    self.setCursor(Qt.CursorShape.ArrowCursor)
                    event.accept()
                    return
                super().mouseReleaseEvent(event)

        return _ZoomableGraphicsView()
