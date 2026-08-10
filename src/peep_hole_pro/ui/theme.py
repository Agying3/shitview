from __future__ import annotations


APP_STYLESHEET = """
QMainWindow {
  background: transparent;
}

QWidget {
  color: #c9d1e3;
  font-family: "Segoe UI", "Microsoft YaHei UI", sans-serif;
}

QWidget#WindowRoot {
  background: transparent;
}

QFrame#AppShell {
  background: rgba(27, 30, 35, 0.96);
  border: 1px solid rgba(124, 144, 178, 0.22);
  border-radius: 12px;
}

QWidget#TitleBar {
  background: rgba(30, 34, 41, 0.78);
  border-top-left-radius: 12px;
  border-top-right-radius: 12px;
}

QWidget#ContentPanel {
  background: transparent;
}

QLabel#Title {
  color: #edf2fb;
  font-size: 18px;
  font-weight: 800;
  letter-spacing: 0px;
}

QLabel#Subtitle {
  color: #78879d;
  font-size: 11px;
}

QLabel#PanelTitle {
  color: #7f8da4;
  font-size: 11px;
  font-weight: 800;
  letter-spacing: 0px;
  padding: 0 0 7px 2px;
}

QFrame#Panel {
  background: rgba(33, 37, 45, 0.72);
  border: 1px solid rgba(121, 137, 165, 0.16);
  border-radius: 8px;
}

QFrame#Panel:hover {
  border: 1px solid rgba(86, 138, 242, 0.28);
}

QPushButton#OpenButton,
QPushButton#WindowButton,
QPushButton#CloseButton {
  background: rgba(52, 59, 72, 0.72);
  border: 1px solid rgba(130, 146, 174, 0.16);
  border-radius: 8px;
  color: #aeb8c9;
  font-size: 12px;
  font-weight: 700;
}

QPushButton#OpenButton {
  color: #d7e3ff;
  background: rgba(86, 138, 242, 0.16);
  border: 1px solid rgba(86, 138, 242, 0.34);
}

QPushButton#OpenButton:hover {
  background: rgba(86, 138, 242, 0.28);
  border: 1px solid rgba(117, 164, 255, 0.56);
  color: #ffffff;
}

QPushButton#WindowButton:hover {
  background: rgba(67, 76, 91, 0.92);
  color: #f1f5ff;
}

QPushButton#CloseButton:hover {
  background: rgba(224, 67, 86, 0.88);
  border: 1px solid rgba(255, 137, 152, 0.55);
  color: #ffffff;
}

QTreeWidget {
  background: transparent;
  border: 0;
  color: #b8c2d4;
  font-size: 12px;
  outline: 0;
  show-decoration-selected: 1;
}

QTreeWidget::item {
  min-height: 28px;
  border-radius: 7px;
  padding: 3px 6px;
  margin: 1px 0;
}

QTreeWidget::item:hover {
  background: rgba(86, 138, 242, 0.11);
  color: #eaf1ff;
}

QTreeWidget::item:selected {
  color: #ffffff;
  background: rgba(86, 138, 242, 0.32);
}

QHeaderView::section {
  background: transparent;
  border: 0;
  color: #69788f;
  font-weight: 800;
  font-size: 11px;
  padding: 4px 6px 8px 6px;
}

QGraphicsView {
  background: #1b1e23;
  border: 0;
  border-radius: 5px;
}

QPlainTextEdit {
  background: transparent;
  border: 0;
  color: #a9b6ca;
  font-family: Consolas, "Cascadia Mono", monospace;
  font-size: 11px;
  selection-background-color: rgba(86, 138, 242, 0.44);
}

QGraphicsView,
QPlainTextEdit,
QTreeWidget {
  selection-background-color: rgba(86, 138, 242, 0.38);
}

QSplitter::handle {
  background: rgba(124, 144, 178, 0.08);
  border-radius: 2px;
}

QSplitter::handle:horizontal {
  width: 6px;
  margin: 6px 0;
}

QLabel#StatusLabel {
  color: #8e9bb0;
  background: rgba(33, 37, 45, 0.46);
  border: 1px solid rgba(121, 137, 165, 0.11);
  border-radius: 8px;
  padding: 5px 10px;
}

QScrollBar:vertical,
QScrollBar:horizontal {
  background: transparent;
  width: 8px;
  height: 8px;
}

QScrollBar::handle:vertical,
QScrollBar::handle:horizontal {
  background: rgba(124, 144, 178, 0.24);
  border-radius: 4px;
}

QScrollBar::handle:vertical:hover,
QScrollBar::handle:horizontal:hover {
  background: rgba(124, 144, 178, 0.40);
}

QScrollBar::add-line,
QScrollBar::sub-line,
QScrollBar::add-page,
QScrollBar::sub-page {
  background: transparent;
  border: 0;
}
"""
