# PEEP-HOLE-PRO

Dynamic project folder peep-hole map with a Python core and Qt GUI.

## Run

```powershell
python -m peep_hole_pro.main H:\some\project
```

Or after installing the package:

```powershell
peep-hole-pro H:\some\project
```

## Reuse As A Plugin

```python
from peep_hole_pro import analyze_folder, open_peep_hole, summarize_folder

summary = summarize_folder(r"H:\some\project")
print(summary)

analysis = analyze_folder(r"H:\some\project")
print(analysis.file_count, analysis.directory_count, analysis.leaf_count)

open_peep_hole(r"H:\some\project")
```

Layers:

- `core`: models, diffing, labeling, graph layout
- `services`: scanning, watching, orchestration
- `ui`: Qt widgets and rendering
- `plugin`: reusable entry points for scripts, tools, and other folders

