# shitview

Dynamic project folder map. The long-term product path is Rust + Slint; the Python + PySide6 implementation remains the migration reference.

## Rust Phase 0

From `rust/` on Windows with MSYS2 MinGW:

```powershell
.\run_phase0.bat
```

The current Slint window is a synthetic 1,000/5,000/10,000-node renderer baseline. It does not yet index a real folder.

## Python Reference Run

```powershell
python -m shitview.main H:\some\project
```

Or after installing the package:

```powershell
shitview H:\some\project
```

## Reuse As A Plugin

```python
from shitview import analyze_folder, open_shitview, summarize_folder

summary = summarize_folder(r"H:\some\project")
print(summary)

analysis = analyze_folder(r"H:\some\project")
print(analysis.file_count, analysis.directory_count, analysis.leaf_count)

open_shitview(r"H:\some\project")
```

Layers:

- `core`: models, diffing, labeling, graph layout
- `services`: scanning, watching, orchestration
- `ui`: Qt widgets and rendering
- `plugin`: reusable entry points for scripts, tools, and other folders


