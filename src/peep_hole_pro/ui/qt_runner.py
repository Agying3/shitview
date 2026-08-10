from __future__ import annotations

def run_qt_app(engine) -> None:
    try:
        from PySide6.QtWidgets import QApplication
    except Exception as exc:  # pragma: no cover
        raise RuntimeError(
            "PySide6 is required for the GUI. Install dependencies and run again."
        ) from exc

    from peep_hole_pro.ui.main_window import MainWindow

    app = QApplication([])
    window = MainWindow(engine)
    window.resize(1440, 900)
    window.show()
    window.start_engine()
    try:
        app.exec()
    finally:
        window.stop()

