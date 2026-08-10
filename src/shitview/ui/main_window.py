from __future__ import annotations

import threading
from pathlib import Path

from PySide6.QtCore import QEasingCurve, QObject, QPropertyAnimation, Qt, Signal
from PySide6.QtGui import QColor, QIcon, QPixmap
from PySide6.QtWidgets import (
    QFrame,
    QGraphicsDropShadowEffect,
    QGraphicsOpacityEffect,
    QHBoxLayout,
    QLabel,
    QMainWindow,
    QPlainTextEdit,
    QPushButton,
    QFileDialog,
    QSplitter,
    QVBoxLayout,
    QWidget,
)

from shitview.services.engine import ShitviewEngine
from shitview.services.layout_store import UserLayoutStore
from shitview.ui.graphics_view import ZoomableGraphicsView
from shitview.ui.canvas import MapCanvas
from shitview.ui.theme import APP_STYLESHEET


class TitleBar(QWidget):
    open_clicked = Signal()
    minimize_clicked = Signal()
    maximize_clicked = Signal()
    close_clicked = Signal()

    def __init__(self, parent=None) -> None:
        super().__init__(parent)
        self.setObjectName('TitleBar')
        self.setFixedHeight(38)
        self._drag_pos = None

        layout = QHBoxLayout(self)
        layout.setContentsMargins(14, 0, 8, 0)
        layout.setSpacing(10)

        self.icon_label = QLabel('')
        self.icon_label.setObjectName('TitleIcon')
        self.icon_label.setFixedSize(24, 24)
        self.icon_label.setScaledContents(True)

        self.title = QLabel('shitview')
        self.title.setObjectName('Title')
        self.subtitle = QLabel('')
        self.subtitle.setObjectName('Subtitle')

        title_box = QVBoxLayout()
        title_box.setSpacing(0)
        title_box.addWidget(self.title)
        title_box.addWidget(self.subtitle)
        layout.addWidget(self.icon_label)
        layout.addLayout(title_box, 1)

        self.btn_open = self._make_button('Open', 'OpenButton', width=58)
        self.btn_min = self._make_button('-', 'WindowButton')
        self.btn_max = self._make_button('[]', 'WindowButton')
        self.btn_close = self._make_button('X', 'CloseButton')

        self.btn_open.clicked.connect(self.open_clicked.emit)
        self.btn_min.clicked.connect(self.minimize_clicked.emit)
        self.btn_max.clicked.connect(self.maximize_clicked.emit)
        self.btn_close.clicked.connect(self.close_clicked.emit)

        layout.addWidget(self.btn_open)
        layout.addWidget(self.btn_min)
        layout.addWidget(self.btn_max)
        layout.addWidget(self.btn_close)

    def _make_button(self, text: str, name: str, width: int = 30) -> QPushButton:
        button = QPushButton(text)
        button.setObjectName(name)
        button.setFixedSize(width, 30)
        button.setFocusPolicy(Qt.FocusPolicy.NoFocus)
        return button

    def set_subtitle(self, text: str) -> None:
        self.subtitle.setText(text)

    def set_icon(self, icon_path: Path) -> None:
        pixmap = QPixmap(str(icon_path))
        if not pixmap.isNull():
            self.icon_label.setPixmap(pixmap)

    def mousePressEvent(self, event) -> None:
        if event.button() == Qt.MouseButton.LeftButton:
            self._drag_pos = event.globalPosition().toPoint()
            event.accept()
            return
        super().mousePressEvent(event)

    def mouseMoveEvent(self, event) -> None:
        if self._drag_pos is not None and event.buttons() & Qt.MouseButton.LeftButton:
            self.window().move(self.window().pos() + event.globalPosition().toPoint() - self._drag_pos)
            self._drag_pos = event.globalPosition().toPoint()
            event.accept()
            return
        super().mouseMoveEvent(event)

    def mouseReleaseEvent(self, event) -> None:
        self._drag_pos = None
        super().mouseReleaseEvent(event)

    def mouseDoubleClickEvent(self, event) -> None:
        if event.button() == Qt.MouseButton.LeftButton:
            self.maximize_clicked.emit()
            event.accept()
            return
        super().mouseDoubleClickEvent(event)


class ShadowContainer(QFrame):
    def __init__(self, inner: QWidget, parent=None) -> None:
        super().__init__(parent)
        self.setObjectName('ShadowContainer')
        shadow = QGraphicsDropShadowEffect(self)
        shadow.setBlurRadius(36)
        shadow.setOffset(0, 12)
        shadow.setColor(QColor(0, 0, 0, 145))
        self.setGraphicsEffect(shadow)

        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(0)
        layout.addWidget(inner)


class MainWindow:
    def __init__(self, engine) -> None:
        self.engine = engine
        self.window = QMainWindow()
        self.window.setWindowTitle('shitview')
        self.icon_path = _app_icon_path()
        if self.icon_path is not None:
            self.window.setWindowIcon(QIcon(str(self.icon_path)))
        self.window.setWindowFlags(Qt.WindowType.FramelessWindowHint | Qt.WindowType.Window)
        self.window.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground, True)
        self.window.setStyleSheet(APP_STYLESHEET)
        self.window.setWindowOpacity(0.0)

        root = QWidget()
        root.setObjectName('WindowRoot')
        root_layout = QVBoxLayout(root)
        root_layout.setContentsMargins(6, 6, 6, 6)
        root_layout.setSpacing(0)

        shell = QFrame()
        shell.setObjectName('AppShell')
        root_layout.addWidget(shell)

        shell_layout = QVBoxLayout(shell)
        shell_layout.setContentsMargins(0, 0, 0, 0)
        shell_layout.setSpacing(0)

        self.title_bar = TitleBar(shell)
        if self.icon_path is not None:
            self.title_bar.set_icon(self.icon_path)
        self.title_bar.set_subtitle(str(self.engine.root))
        self.title_bar.open_clicked.connect(self._open_folder)
        self.title_bar.minimize_clicked.connect(self.window.showMinimized)
        self.title_bar.maximize_clicked.connect(self._toggle_maximized)
        self.title_bar.close_clicked.connect(self.window.close)
        shell_layout.addWidget(self.title_bar)

        body = QWidget()
        body.setObjectName('ContentPanel')
        body_layout = QVBoxLayout(body)
        body_layout.setContentsMargins(8, 0, 8, 8)
        body_layout.setSpacing(8)

        splitter = QSplitter(Qt.Orientation.Horizontal)
        splitter.setChildrenCollapsible(False)
        body_layout.addWidget(splitter, 1)

        self.canvas = ZoomableGraphicsView()
        self.inspector = QPlainTextEdit()
        self.inspector.setReadOnly(True)

        center_panel = self._make_panel('GRAPH', self.canvas)
        right_panel = self._make_panel('ACTIVITY', self.inspector)

        splitter.addWidget(center_panel)
        splitter.addWidget(right_panel)
        splitter.setStretchFactor(0, 5)
        splitter.setStretchFactor(1, 1)
        splitter.setSizes([1240, 260])

        self.status_label = QLabel('Ready')
        self.status_label.setObjectName('StatusLabel')
        body_layout.addWidget(self.status_label)
        self._status_opacity = QGraphicsOpacityEffect(self.status_label)
        self.status_label.setGraphicsEffect(self._status_opacity)
        self._status_animation = QPropertyAnimation(self._status_opacity, b'opacity')
        self._status_animation.setDuration(460)
        self._status_animation.setStartValue(0.58)
        self._status_animation.setKeyValueAt(0.45, 1.0)
        self._status_animation.setEndValue(0.84)
        self._status_animation.setEasingCurve(QEasingCurve.Type.OutCubic)

        shell_layout.addWidget(body)
        self.window.setCentralWidget(root)
        self.window.resize(1620, 920)

        self.current_snapshot = None
        self.canvas_widget = MapCanvas(
            self.canvas,
            on_select=self._on_node_selected,
            layout_store=UserLayoutStore(self.engine.root),
        )

        class _Bridge(QObject):
            snapshot = Signal(object)
            changes = Signal(object)
            status = Signal(object)

        self.bridge = _Bridge()
        self.bridge.snapshot.connect(self._on_snapshot)
        self.bridge.changes.connect(self._on_changes)
        self.bridge.status.connect(self._on_status)

        self._bind_engine(self.engine)
        self._startup_animation = QPropertyAnimation(self.window, b'windowOpacity')
        self._startup_animation.setDuration(260)
        self._startup_animation.setStartValue(0.0)
        self._startup_animation.setEndValue(1.0)
        self._startup_animation.setEasingCurve(QEasingCurve.Type.OutCubic)

    def _bind_engine(self, engine) -> None:
        engine.events.subscribe('snapshot', self.bridge.snapshot.emit)
        engine.events.subscribe('changes', self.bridge.changes.emit)
        engine.events.subscribe('status', self.bridge.status.emit)

    def _open_folder(self) -> None:
        folder = QFileDialog.getExistingDirectory(self.window, 'Open project folder', str(self.engine.root))
        if not folder:
            return
        self.engine.stop()
        self.engine = ShitviewEngine(root=Path(folder).expanduser().resolve(), polling_interval=self.engine.polling_interval)
        self.canvas_widget.layout_store = UserLayoutStore(self.engine.root)
        self.title_bar.set_subtitle(str(self.engine.root))
        self.current_snapshot = None
        self._bind_engine(self.engine)
        self.start_engine()

    def _toggle_maximized(self) -> None:
        if self.window.isMaximized():
            self.window.showNormal()
            self.title_bar.btn_max.setText('[]')
        else:
            self.window.showMaximized()
            self.title_bar.btn_max.setText('[ ]')

    def _make_panel(self, title: str, content) -> QWidget:
        panel = QFrame()
        panel.setObjectName('Panel')
        shadow = QGraphicsDropShadowEffect(panel)
        shadow.setBlurRadius(24)
        shadow.setOffset(0, 10)
        shadow.setColor(QColor(0, 0, 0, 82))
        panel.setGraphicsEffect(shadow)
        layout = QVBoxLayout(panel)
        layout.setContentsMargins(12, 10, 12, 12)
        layout.setSpacing(6)
        label = QLabel(title)
        label.setObjectName('PanelTitle')
        layout.addWidget(label)
        layout.addWidget(content, 1)
        return panel

    def show(self) -> None:
        self.window.show()
        self._startup_animation.stop()
        self._startup_animation.start()

    def start_engine(self) -> None:
        self.status_label.setText('Indexing project...')
        self._pulse_status()
        self._engine_start_thread = threading.Thread(target=self.engine.start, daemon=True)
        self._engine_start_thread.start()

    def stop(self) -> None:
        self.engine.stop()

    def resize(self, width: int, height: int) -> None:
        self.window.resize(width, height)

    def _on_snapshot(self, snapshot) -> None:
        self.current_snapshot = snapshot
        self.canvas_widget.render_snapshot(snapshot)
        self.title_bar.set_subtitle(str(self.engine.root))
        self.status_label.setText(f'Indexed {len(snapshot.nodes)} nodes')
        self._pulse_status()
        self.inspector.setPlainText(f'Root\n{snapshot.root}\n\nNodes\n{len(snapshot.nodes)}')

    def _on_changes(self, changes) -> None:
        if not changes:
            return
        self.status_label.setText(f'Changes {len(changes)}')
        self._pulse_status()
        lines = [f'{change.kind.value}: {change.path}' for change in changes[-40:]]
        self.inspector.appendPlainText('\n\nLatest changes\n' + '\n'.join(lines))

    def _on_status(self, status) -> None:
        self.status_label.setText(status.text)
        self._pulse_status()

    def _pulse_status(self) -> None:
        self._status_animation.stop()
        self._status_animation.start()

    def _on_node_selected(self, path: str) -> None:
        if self.current_snapshot is None:
            return
        node = self.current_snapshot.nodes.get(path)
        if node is None:
            return
        details = [
            node.name,
            '',
            f'Path\n{node.path}',
            '',
            f'Type\n{node.kind.value}',
            '',
            f'Size\n{node.size} bytes',
            '',
            f'Children\n{len(node.children)}',
        ]
        if node.labels:
            details.extend(['', f"Labels\n{', '.join(node.labels)}"])
        self.inspector.setPlainText('\n'.join(details))


def _app_icon_path() -> Path | None:
    candidates = [
        Path.cwd() / 'resouce' / 'shitview.jpg',
        Path(__file__).resolve().parents[3] / 'resouce' / 'shitview.jpg',
    ]
    for candidate in candidates:
        if candidate.exists():
            return candidate
    return None


