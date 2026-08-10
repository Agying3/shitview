# shitview 当前进度记录

更新时间：2026-07-08

## 项目定位

- 项目名：`shitview`。
- 技术方向：Python + PySide6 桌面 GUI，不做 WebUI。
- 启动命令：`python -m shitview.main .`
- 可复用入口：`shitview` 包内提供 `summarize_folder`、`analyze_folder`、`open_shitview` 等插件式 API。
- 旧名称 `panorama`、`peep_hole_pro`、`PEEP-HOLE-PRO` 不再作为项目身份使用。

## 当前 UI 方向

- 主界面是暗色、亚克力/毛玻璃风格。
- 窗口是圆角无边框设计。
- 标题栏使用 `H:\shitview\resouce\shitview.jpg` 作为应用图标。
- 主画布已经从树形控件改为 `QGraphicsView + QGraphicsScene`。
- 画布上的每个文件/文件夹都是一个圆角节点方块。
- 文件夹/模块使用半透明大框分区，但目前策略是不使用嵌套大框。
- 大框标题使用更清晰的 UI 字体、高亮文字和固定尺寸标题底片。
- 节点文字使用 `Microsoft YaHei UI`，更适合中英文混排。

## 布局现状

- 当前布局使用“模块岛屿 + 子模块泳道 + 多列文件网格”。
- 根目录散文件会和模块区域拉开，避免压到第一个模块框。
- 当前 `H:\shitview` 可见分框为：`docx`、`resouce`、`core`、`services`、`ui`。
- 已去掉 `src -> shitview -> core/services/ui` 这种大框套娃结构。
- 旧的用户布局缓存已经通过版本号隔离，当前布局缓存版本为 `3`。
- 小/中项目全局视角使用紧凑短标签；完整文件名通过 hover tooltip 和右侧详情面板查看。

## 连线系统

- 连线是绿色正交折线，偏电路板风格。
- 连线会随节点拖动实时更新。
- 连线使用障碍物感知的多折路由，尽量绕开其它文件块。
- 当前 `H:\shitview` 验证结果：绿色线穿过其它文件块的数量为 `0`。

## 交互现状

- 鼠标拖动节点时，相关连线会更新。
- 拖动父节点时，子节点会跟随移动。
- 鼠标拖动空白区域可以平移视角。
- 鼠标悬停节点会轻微放大并高亮。
- 点击节点会在右侧详情面板显示路径、类型、大小、子节点数量等信息。
- 文件夹变化会通过监听/轮询自动刷新，不需要手动刷新按钮。

## 性能策略

- 大项目启动时跳过重型全局碰撞解算，避免 UI 卡死。
- 已移除每个节点的投影效果，因为 `QGraphicsDropShadowEffect` 对大量节点很耗性能。
- 大项目连线避障会使用预计算障碍物矩形，减少重复计算。
- 当前项目 `H:\shitview` 的离屏渲染验证约为 `0.117s - 0.2s` 量级。
- `H:\dmshoot` 体量测试约 500 个可见节点、36 个分框，渲染约 `1.3s - 2.0s`；主要慢点已经转移到文件系统扫描。

## 最近验证结果

当前 `H:\shitview` 的几何验证结果：

```text
nodes 41
groups 5
node_overlaps 0
separate_overlaps 0
edge_node_violations 0
```

说明：

- `node_overlaps 0`：文件/文件夹节点没有互相重叠。
- `separate_overlaps 0`：有效分框与外部节点/其它分框没有冲突。
- `edge_node_violations 0`：绿色连线没有穿过其它文件块。

## 关键文件

- `src/shitview/core/graph_layout.py`：图布局算法、模块岛屿、分框过滤、节点尺寸和坐标。
- `src/shitview/ui/canvas.py`：画布渲染、场景初始化、碰撞处理、连线创建、视角适配。
- `src/shitview/ui/graph_items.py`：节点、大框、绿色连线的具体绘制和动画。
- `src/shitview/services/layout_store.py`：用户拖拽布局记忆和布局缓存版本。
- `src/shitview/ui/main_window.py`：无边框主窗口、标题栏、图标、右侧活动面板。
- `docx/memory_1.md`：之前的英文/混合进度记录。
- `docx/progress_zh.md`：当前中文进度记录。

## 下一步建议

- 做扫描缓存和增量扫描，解决大项目导入时扫描慢的问题。
- 为 `docs`、`scripts` 这种密集目录做可折叠分组。
- 增加小地图/缩略概览，让大项目既能全局浏览又能快速定位。
- 增加“只显示重要分框 / 显示全部分框”的切换。
- 继续打磨节点文字：全局短标签、缩放后完整标签、hover 放大标签可以分层处理。
