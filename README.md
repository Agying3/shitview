# shitview

Dynamic project folder peep-hole map with a Python core and Qt GUI.

## Run

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


