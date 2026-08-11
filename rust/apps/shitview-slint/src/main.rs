#![cfg_attr(windows, windows_subsystem = "windows")]

use slint::{
    Color, Image, ModelRc, RenderingState, Rgba8Pixel, SharedPixelBuffer, SharedString, Timer,
    TimerMode, VecModel,
};
use shitview_core::NodeKind;
use shitview_indexer::{
    default_database_path, IndexEvent, IndexHandle, IndexOptions, IndexPhase, IndexProgress,
};
use shitview_storage::StoredNode;
mod layout;
use layout::{LayoutEntry, LayoutStore};
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

slint::include_modules!();

const MODULE_COUNT: usize = 12;
const MODULE_COLUMNS: usize = 4;

struct SyntheticScene {
    nodes: Vec<SceneRect>,
    modules: Vec<SceneRect>,
    segments: Vec<SceneSegment>,
    labels: Vec<SceneLabel>,
    hit_targets: Vec<HitTarget>,
    width: f32,
    height: f32,
    elapsed: Duration,
}

struct PreparedScene {
    pixels: SharedPixelBuffer<Rgba8Pixel>,
    labels: Vec<SceneLabel>,
    width: f32,
    height: f32,
    node_count: usize,
    primitive_count: usize,
    elapsed: Duration,
    spatial_index: SpatialIndex,
}

#[derive(Debug, Clone)]
struct HitTarget {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    path: String,
    openable: bool,
    stable_id: Option<Vec<u8>>,
    pinned: bool,
}

#[derive(Debug, Clone)]
struct DragState {
    path: String,
    stable_id: Option<Vec<u8>>,
    origin_x: f32,
    origin_y: f32,
    start_pointer_x: f32,
    start_pointer_y: f32,
}

#[derive(Debug, Default)]
struct SpatialIndex {
    cell_size: f32,
    targets: Vec<HitTarget>,
    cells: HashMap<(i32, i32), Vec<usize>>,
}

impl SpatialIndex {
    fn new(targets: Vec<HitTarget>) -> Self {
        let cell_size = 128.0;
        let mut cells = HashMap::<(i32, i32), Vec<usize>>::new();
        for (index, target) in targets.iter().enumerate() {
            let min_x = (target.x / cell_size).floor() as i32;
            let max_x = ((target.x + target.width) / cell_size).floor() as i32;
            let min_y = (target.y / cell_size).floor() as i32;
            let max_y = ((target.y + target.height) / cell_size).floor() as i32;
            for cell_y in min_y..=max_y {
                for cell_x in min_x..=max_x {
                    cells.entry((cell_x, cell_y)).or_default().push(index);
                }
            }
        }
        Self {
            cell_size,
            targets,
            cells,
        }
    }

    fn hit(&self, x: f32, y: f32) -> Option<&HitTarget> {
        if self.cell_size <= 0.0 {
            return None;
        }
        let cell = (
            (x / self.cell_size).floor() as i32,
            (y / self.cell_size).floor() as i32,
        );
        self.cells.get(&cell)?.iter().rev().find_map(|index| {
            let target = &self.targets[*index];
            (x >= target.x
                && x <= target.x + target.width
                && y >= target.y
                && y <= target.y + target.height)
                .then_some(target)
        })
    }

    fn find_path(&self, path: &str) -> Option<&HitTarget> {
        self.targets.iter().find(|target| target.path == path)
    }
}

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    let spatial_index = Arc::new(Mutex::new(SpatialIndex::default()));
    let selected_target = Rc::new(RefCell::new(None::<HitTarget>));
    let drag_state = Rc::new(RefCell::new(None::<DragState>));
    let layout_store = Arc::new(Mutex::new(LayoutStore::default()));
    let current_nodes = Arc::new(Mutex::new(Vec::<StoredNode>::new()));
    let rendered_frames = Rc::new(Cell::new(0_u32));
    let notifier_frames = Rc::clone(&rendered_frames);
    let _ = ui.window().set_rendering_notifier(move |state, _| {
        if matches!(state, RenderingState::AfterRendering) {
            notifier_frames.set(notifier_frames.get().saturating_add(1));
        }
    });
    apply_scene(&ui, 1_000, &spatial_index);
    if let Ok(current_directory) = std::env::current_dir() {
        ui.set_project_path(SharedString::from(
            current_directory.to_string_lossy().into_owned(),
        ));
    }

    let index_handle = Rc::new(RefCell::new(None::<IndexHandle>));
    let density_request = Arc::new(AtomicU64::new(0));

    let weak = ui.as_weak();
    let handle = Rc::clone(&index_handle);
    let scene_request = Arc::clone(&density_request);
    let interaction = Arc::clone(&spatial_index);
    let layout_store_for_open = Arc::clone(&layout_store);
    let current_nodes_for_open = Arc::clone(&current_nodes);
    ui.on_open_project(move |path| {
        scene_request.fetch_add(1, Ordering::Relaxed);
        if let Some(ui) = weak.upgrade() {
            start_project_index(
                &ui,
                &handle,
                &interaction,
                &layout_store_for_open,
                &current_nodes_for_open,
                PathBuf::from(path.as_str()),
            );
        }
    });

    let weak = ui.as_weak();
    let handle = Rc::clone(&index_handle);
    let scene_request = Arc::clone(&density_request);
    let interaction = Arc::clone(&spatial_index);
    let layout_store_for_browse = Arc::clone(&layout_store);
    let current_nodes_for_browse = Arc::clone(&current_nodes);
    ui.on_browse_project(move || {
        let Some(path) = choose_project_folder() else {
            return;
        };
        scene_request.fetch_add(1, Ordering::Relaxed);
        if let Some(ui) = weak.upgrade() {
            ui.set_project_path(SharedString::from(path.to_string_lossy().into_owned()));
            start_project_index(
                &ui,
                &handle,
                &interaction,
                &layout_store_for_browse,
                &current_nodes_for_browse,
                path,
            );
        }
    });

    let handle = Rc::clone(&index_handle);
    ui.on_pause_index(move || {
        if let Some(index) = handle.borrow().as_ref() {
            let _ = index.pause();
        }
    });

    let handle = Rc::clone(&index_handle);
    ui.on_resume_index(move || {
        if let Some(index) = handle.borrow().as_ref() {
            let _ = index.resume();
        }
    });

    let handle = Rc::clone(&index_handle);
    ui.on_cancel_index(move || {
        if let Some(index) = handle.borrow().as_ref() {
            let _ = index.cancel();
        }
    });

    let weak = ui.as_weak();
    let active_request = Arc::clone(&density_request);
    let interaction = Arc::clone(&spatial_index);
    let current_nodes_for_density = Arc::clone(&current_nodes);
    ui.on_select_density(move |count| {
        let count = count.clamp(1_000, 10_000) as usize;
        if let Ok(mut nodes) = current_nodes_for_density.lock() {
            nodes.clear();
        }
        let request = active_request.fetch_add(1, Ordering::Relaxed) + 1;
        let active_request = Arc::clone(&active_request);
        let interaction = Arc::clone(&interaction);
        let weak = weak.clone();
        thread::spawn(move || {
            let prepared = prepare_scene(build_scene(count), count);
            let _ = slint::invoke_from_event_loop(move || {
                if active_request.load(Ordering::Relaxed) != request {
                    return;
                }
                if let Some(ui) = weak.upgrade() {
                    apply_prepared_scene(&ui, prepared, "", &interaction);
                    ui.set_status_title(SharedString::from("Synthetic scene ready"));
                }
            });
        });
    });

    let weak = ui.as_weak();
    let interaction = Arc::clone(&spatial_index);
    let selected = Rc::clone(&selected_target);
    ui.on_canvas_clicked(move |x, y| {
        let hit = interaction
            .lock()
            .ok()
            .and_then(|index| index.hit(x, y).cloned());
        *selected.borrow_mut() = hit.clone();
        if let Some(ui) = weak.upgrade() {
            apply_selection(&ui, hit.as_ref());
        }
    });

    let weak = ui.as_weak();
    let interaction = Arc::clone(&spatial_index);
    let selected = Rc::clone(&selected_target);
    let drag = Rc::clone(&drag_state);
    ui.on_canvas_pressed(move |x, y| {
        let hit = interaction
            .lock()
            .ok()
            .and_then(|index| index.hit(x, y).cloned());
        *selected.borrow_mut() = hit.clone();
        *drag.borrow_mut() = hit.as_ref().map(|target| DragState {
            path: target.path.clone(),
            stable_id: target.stable_id.clone(),
            origin_x: target.x,
            origin_y: target.y,
            start_pointer_x: x,
            start_pointer_y: y,
        });
        if let Some(ui) = weak.upgrade() {
            ui.set_canvas_pan_enabled(hit.is_none());
            apply_selection(&ui, hit.as_ref());
        }
    });

    let weak = ui.as_weak();
    let selected = Rc::clone(&selected_target);
    let drag = Rc::clone(&drag_state);
    ui.on_canvas_moved(move |x, y| {
        let Some(state) = drag.borrow().as_ref().cloned() else {
            return;
        };
        if let Some(ui) = weak.upgrade() {
            if let Some(target) = selected.borrow_mut().as_mut() {
                target.x = state.origin_x + x - state.start_pointer_x;
                target.y = state.origin_y + y - state.start_pointer_y;
                ui.set_selected_x(target.x);
                ui.set_selected_y(target.y);
            }
        }
    });

    let weak = ui.as_weak();
    let selected = Rc::clone(&selected_target);
    let drag = Rc::clone(&drag_state);
    let layout_store_for_drag = Arc::clone(&layout_store);
    let current_nodes_for_drag = Arc::clone(&current_nodes);
    let interaction_for_drag = Arc::clone(&spatial_index);
    ui.on_canvas_released(move |x, y| {
        let Some(state) = drag.borrow_mut().take() else {
            return;
        };
        let new_x = (state.origin_x + x - state.start_pointer_x).max(60.0);
        let new_y = (state.origin_y + y - state.start_pointer_y).max(60.0);
        let Some(ui) = weak.upgrade() else {
            return;
        };
        ui.set_canvas_pan_enabled(true);
        if let Ok(mut layout) = layout_store_for_drag.lock() {
            layout.set(
                state.stable_id.as_deref(),
                &state.path,
                LayoutEntry { x: new_x, y: new_y, pinned: true },
            );
            if let Err(error) = layout.save() {
                ui.set_status_detail(SharedString::from(format!("Cannot save layout: {error}")));
            }
        }
        if let Some(target) = selected.borrow_mut().as_mut() {
            target.x = new_x;
            target.y = new_y;
            target.pinned = true;
        }
        let nodes = current_nodes_for_drag.lock().ok().map(|nodes| nodes.clone());
        let layout = layout_store_for_drag.lock().ok().map(|layout| layout.clone());
        if let (Some(nodes), Some(layout)) = (nodes, layout) {
            rebuild_project_scene(
                &ui,
                nodes,
                layout,
                &interaction_for_drag,
                Some(state.path),
            );
        }
    });

    let weak = ui.as_weak();
    let selected = Rc::clone(&selected_target);
    let layout_store_for_pin = Arc::clone(&layout_store);
    let current_nodes_for_pin = Arc::clone(&current_nodes);
    let interaction_for_pin = Arc::clone(&spatial_index);
    ui.on_pin_selected(move || {
        let Some(target) = selected.borrow().clone() else {
            return;
        };
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if let Ok(mut layout) = layout_store_for_pin.lock() {
            if target.pinned {
                layout.remove(target.stable_id.as_deref(), &target.path);
            } else {
                layout.set(
                    target.stable_id.as_deref(),
                    &target.path,
                    LayoutEntry { x: target.x, y: target.y, pinned: true },
                );
            }
            if let Err(error) = layout.save() {
                ui.set_status_detail(SharedString::from(format!("Cannot save layout: {error}")));
            }
        }
        ui.set_selection_pinned(!target.pinned);
        let nodes = current_nodes_for_pin.lock().ok().map(|nodes| nodes.clone());
        let layout = layout_store_for_pin.lock().ok().map(|layout| layout.clone());
        if let (Some(nodes), Some(layout)) = (nodes, layout) {
            rebuild_project_scene(&ui, nodes, layout, &interaction_for_pin, Some(target.path));
        }
    });

    let weak = ui.as_weak();
    let selected = Rc::clone(&selected_target);
    let layout_store_for_reset = Arc::clone(&layout_store);
    let current_nodes_for_reset = Arc::clone(&current_nodes);
    let interaction_for_reset = Arc::clone(&spatial_index);
    ui.on_reset_layout(move || {
        if let Ok(mut layout) = layout_store_for_reset.lock() {
            layout.remove_all();
            if let Err(error) = layout.save() {
                if let Some(ui) = weak.upgrade() {
                    ui.set_status_detail(SharedString::from(format!("Cannot save layout: {error}")));
                }
                return;
            }
        }
        let nodes = current_nodes_for_reset.lock().ok().map(|nodes| nodes.clone());
        let layout = layout_store_for_reset.lock().ok().map(|layout| layout.clone());
        if let (Some(nodes), Some(layout), Some(ui)) = (nodes, layout, weak.upgrade()) {
            let selected_path = selected.borrow().as_ref().map(|target| target.path.clone());
            rebuild_project_scene(&ui, nodes, layout, &interaction_for_reset, selected_path);
        }
    });

    let selected = Rc::clone(&selected_target);
    ui.on_open_selected(move || {
        let Some(target) = selected.borrow().clone() else {
            return;
        };
        if target.openable {
            let _ = open_external_editor(PathBuf::from(target.path));
        }
    });

    let weak = ui.as_weak();
    ui.on_zoom_in(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_zoom((ui.get_zoom() + 0.10).min(1.60));
        }
    });

    let weak = ui.as_weak();
    ui.on_zoom_out(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_zoom((ui.get_zoom() - 0.10).max(0.28));
        }
    });

    let pulse_started = Instant::now();
    let pulse_timer = Timer::default();
    let weak = ui.as_weak();
    pulse_timer.start(TimerMode::Repeated, Duration::from_millis(16), move || {
        if let Some(ui) = weak.upgrade() {
            let phase = (pulse_started.elapsed().as_secs_f32() * 1.8).sin();
            ui.set_pulse((phase + 1.0) * 0.5);
        }
    });

    let fps_timer = Timer::default();
    let weak = ui.as_weak();
    fps_timer.start(TimerMode::Repeated, Duration::from_secs(1), move || {
        let frames = rendered_frames.replace(0);
        if let Some(ui) = weak.upgrade() {
            ui.set_frame_metric(SharedString::from(if ui.get_benchmark_active() {
                frames.to_string()
            } else {
                "idle".to_owned()
            }));
        }
    });

    if let Some(root) = std::env::args_os().nth(1) {
        start_project_index(
            &ui,
            &index_handle,
            &spatial_index,
            &layout_store,
            &current_nodes,
            PathBuf::from(root),
        );
    }

    ui.run()
}

fn apply_scene(ui: &AppWindow, count: usize, spatial_index: &Arc<Mutex<SpatialIndex>>) {
    let prepared = prepare_scene(build_scene(count), count);
    apply_prepared_scene(ui, prepared, "", spatial_index);
    ui.set_status_title(SharedString::from("Synthetic scene ready"));
    ui.set_status_detail(SharedString::from("Choose a project folder to build its index"));
}

fn prepare_scene(scene: SyntheticScene, count: usize) -> PreparedScene {
    let primitive_count = scene.nodes.len() + scene.modules.len() + scene.segments.len();
    let scene_elapsed = scene.elapsed;
    let raster_started = Instant::now();
    let pixels = rasterize_scene(&scene);
    let total_elapsed = scene_elapsed + raster_started.elapsed();
    let spatial_index = SpatialIndex::new(scene.hit_targets);
    PreparedScene {
        pixels,
        labels: scene.labels,
        width: scene.width,
        height: scene.height,
        node_count: count,
        primitive_count,
        elapsed: total_elapsed,
        spatial_index,
    }
}

fn apply_prepared_scene(
    ui: &AppWindow,
    scene: PreparedScene,
    generation: &str,
    spatial_index: &Arc<Mutex<SpatialIndex>>,
) {
    if let Ok(mut current) = spatial_index.lock() {
        *current = scene.spatial_index;
    }
    ui.set_selection_available(false);
    ui.set_selection_openable(false);
    ui.set_selected_path(SharedString::default());
    ui.set_scene_width(scene.width);
    ui.set_scene_height(scene.height);
    ui.set_graph_image(Image::from_rgba8(scene.pixels));
    ui.set_graph_labels(ModelRc::from(Rc::new(VecModel::from(scene.labels))));
    ui.set_node_metric(SharedString::from(format_number(scene.node_count)));
    ui.set_primitive_metric(SharedString::from(format_number(scene.primitive_count)));
    ui.set_benchmark_active(generation.is_empty());
    ui.set_generation_metric(SharedString::from(if generation.is_empty() {
        format!("{:.0} ms", scene.elapsed.as_secs_f64() * 1_000.0)
    } else {
        generation.to_owned()
    }));
}

fn rasterize_scene(scene: &SyntheticScene) -> SharedPixelBuffer<Rgba8Pixel> {
    let (scale, width, height) = raster_dimensions(scene);
    let mut pixels = SharedPixelBuffer::<Rgba8Pixel>::new(width, height);
    pixels.make_mut_slice().fill(Rgba8Pixel {
        r: 0x0d,
        g: 0x19,
        b: 0x15,
        a: 255,
    });

    for segment in &scene.segments {
        draw_rect(&mut pixels, segment.x, segment.y, segment.width, segment.height, segment.color, scale);
    }
    for module in &scene.modules {
        draw_rect(&mut pixels, module.x, module.y, module.width, module.height, module.fill, scale);
        draw_border(&mut pixels, module.x, module.y, module.width, module.height, module.border, scale);
    }
    for node in &scene.nodes {
        draw_rect(&mut pixels, node.x, node.y, node.width, node.height, node.fill, scale);
        draw_border(&mut pixels, node.x, node.y, node.width, node.height, node.border, scale);
    }
    pixels
}

fn raster_dimensions(scene: &SyntheticScene) -> (f32, u32, u32) {
    const MAX_BITMAP_DIMENSION: f32 = 4_096.0;
    let scale = (0.5_f32)
        .min(MAX_BITMAP_DIMENSION / scene.width.max(1.0))
        .min(MAX_BITMAP_DIMENSION / scene.height.max(1.0))
        .max(0.08);
    let width = (scene.width * scale).ceil().max(1.0) as u32;
    let height = (scene.height * scale).ceil().max(1.0) as u32;
    (scale, width, height)
}

fn draw_border(
    pixels: &mut SharedPixelBuffer<Rgba8Pixel>,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: Color,
    scale: f32,
) {
    let thickness = (scale * 1.5).ceil().max(1.0) / scale;
    draw_rect(pixels, x, y, width, thickness, color, scale);
    draw_rect(pixels, x, y + height - thickness, width, thickness, color, scale);
    draw_rect(pixels, x, y, thickness, height, color, scale);
    draw_rect(pixels, x + width - thickness, y, thickness, height, color, scale);
}

fn push_module_frame(
    modules: &mut Vec<SceneRect>,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    accent: (u8, u8, u8),
) {
    let (red, green, blue) = accent;
    modules.push(SceneRect {
        x,
        y,
        width,
        height,
        fill: Color::from_argb_u8(105, red / 3 + 5, green / 3 + 8, blue / 3 + 7),
        border: Color::from_argb_u8(210, red, green, blue),
    });
    modules.push(SceneRect {
        x: x + 8.0,
        y: y + 8.0,
        width: (width - 16.0).max(1.0),
        height: (height - 16.0).max(1.0),
        fill: Color::from_argb_u8(35, red, green, blue),
        border: Color::from_argb_u8(75, red, green, blue),
    });
    modules.push(SceneRect {
        x: x + 14.0,
        y: y + 12.0,
        width: (width - 28.0).min(320.0).max(1.0),
        height: 30.0,
        fill: Color::from_argb_u8(160, 0x13, 0x27, 0x20),
        border: Color::from_argb_u8(120, red, green, blue),
    });
    let pad_color = Color::from_argb_u8(230, 0xd0, 0xa8, 0x58);
    for (pad_x, pad_y) in [
        (x + 12.0, y + height - 20.0),
        (x + width - 20.0, y + 12.0),
        (x + width - 20.0, y + height - 20.0),
    ] {
        modules.push(SceneRect {
            x: pad_x,
            y: pad_y,
            width: 8.0,
            height: 8.0,
            fill: pad_color,
            border: Color::from_argb_u8(255, 0xf0, 0xc9, 0x72),
        });
    }
}

fn draw_rect(
    pixels: &mut SharedPixelBuffer<Rgba8Pixel>,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: Color,
    scale: f32,
) {
    let x0 = (x * scale).floor().max(0.0) as u32;
    let y0 = (y * scale).floor().max(0.0) as u32;
    let x1 = ((x + width) * scale).ceil().max(0.0) as u32;
    let y1 = ((y + height) * scale).ceil().max(0.0) as u32;
    let width_limit = pixels.width();
    let height_limit = pixels.height();
    let x1 = x1.min(width_limit);
    let y1 = y1.min(height_limit);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let source = Rgba8Pixel {
        r: color.red(),
        g: color.green(),
        b: color.blue(),
        a: color.alpha(),
    };
    let row_width = width_limit as usize;
    let data = pixels.make_mut_slice();
    for row in y0..y1 {
        for column in x0..x1 {
            let index = row as usize * row_width + column as usize;
            data[index] = blend(data[index], source);
        }
    }
}

fn blend(background: Rgba8Pixel, foreground: Rgba8Pixel) -> Rgba8Pixel {
    if foreground.a == 255 {
        return foreground;
    }
    let alpha = u16::from(foreground.a);
    let inverse = 255_u16.saturating_sub(alpha);
    Rgba8Pixel {
        r: ((u16::from(foreground.r) * alpha + u16::from(background.r) * inverse) / 255) as u8,
        g: ((u16::from(foreground.g) * alpha + u16::from(background.g) * inverse) / 255) as u8,
        b: ((u16::from(foreground.b) * alpha + u16::from(background.b) * inverse) / 255) as u8,
        a: 255,
    }
}

fn start_project_index(
    ui: &AppWindow,
    active_handle: &Rc<RefCell<Option<IndexHandle>>>,
    spatial_index: &Arc<Mutex<SpatialIndex>>,
    layout_store: &Arc<Mutex<LayoutStore>>,
    current_nodes: &Arc<Mutex<Vec<StoredNode>>>,
    root: PathBuf,
) {
    let root = match root.canonicalize() {
        Ok(root) if root.is_dir() => root,
        Ok(_) => {
            set_failed_status(ui, "The selected path is not a folder");
            return;
        }
        Err(error) => {
            set_failed_status(ui, &format!("Cannot open project folder: {error}"));
            return;
        }
    };
    let root_display = display_path_for_ui(&root);
    let (loaded_layout, layout_warning) = LayoutStore::load(&root);
    if let Ok(mut layout) = layout_store.lock() {
        *layout = loaded_layout;
    }
    if let Some(warning) = layout_warning {
        ui.set_status_detail(SharedString::from(warning));
    }
    if let Ok(mut nodes) = current_nodes.lock() {
        nodes.clear();
    }
    ui.set_project_path(SharedString::from(&root_display));
    ui.set_status_title(SharedString::from("Starting index"));
    ui.set_status_detail(SharedString::from("Preparing SQLite/WAL and file watcher"));
    ui.set_indexing(true);
    ui.set_paused(false);
    ui.set_benchmark_active(false);

    active_handle.borrow_mut().take();
    let database_path = default_database_path(&root);
    let handle = match IndexHandle::start(&root, database_path, IndexOptions::default()) {
        Ok(handle) => handle,
        Err(error) => {
            set_failed_status(ui, &error.to_string());
            return;
        }
    };
    let receiver = handle.events();
    *active_handle.borrow_mut() = Some(handle);
    let weak = ui.as_weak();
    let spatial_index = Arc::clone(spatial_index);
    let layout_store = Arc::clone(layout_store);
    let current_nodes = Arc::clone(current_nodes);
    thread::Builder::new()
        .name("shitview-slint-index-events".to_owned())
        .spawn(move || {
            while let Ok(event) = receiver.recv() {
                match event {
                    IndexEvent::Progress(progress) => {
                        let weak = weak.clone();
                        let expected_root = root_display.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = weak.upgrade() {
                                if ui.get_project_path().as_str() == expected_root {
                                    apply_progress(&ui, &progress);
                                }
                            }
                        });
                    }
                    IndexEvent::Nodes(nodes) => {
                        let node_count = nodes.len();
                        if let Ok(mut current) = current_nodes.lock() {
                            *current = nodes.clone();
                        }
                        let layout = layout_store.lock().ok().map(|layout| layout.clone());
                        let prepared = prepare_scene(
                            build_index_scene(&nodes, layout.as_ref()),
                            node_count,
                        );
                        let weak = weak.clone();
                        let expected_root = root_display.clone();
                        let spatial_index = Arc::clone(&spatial_index);
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = weak.upgrade() {
                                if ui.get_project_path().as_str() == expected_root {
                                    let generation = ui.get_generation_metric().to_string();
                                    apply_prepared_scene(
                                        &ui,
                                        prepared,
                                        &generation,
                                        &spatial_index,
                                    );
                                }
                            }
                        });
                    }
                    IndexEvent::Warning(message) => {
                        let weak = weak.clone();
                        let expected_root = root_display.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = weak.upgrade() {
                                if ui.get_project_path().as_str() == expected_root {
                                    ui.set_status_detail(SharedString::from(message));
                                }
                            }
                        });
                    }
                    IndexEvent::Failed(message) => {
                        let weak = weak.clone();
                        let expected_root = root_display.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = weak.upgrade() {
                                if ui.get_project_path().as_str() == expected_root {
                                    set_failed_status(&ui, &message);
                                }
                            }
                        });
                        break;
                    }
                }
            }
        })
        .expect("failed to start Slint index event bridge");
}

fn apply_progress(ui: &AppWindow, progress: &IndexProgress) {
    let (title, indexing, paused) = match progress.phase {
        IndexPhase::Starting => ("Starting index", true, false),
        IndexPhase::Scanning => ("Scanning project", true, false),
        IndexPhase::Paused => ("Index paused", true, true),
        IndexPhase::ReplayingChanges => ("Applying file changes", true, false),
        IndexPhase::Watching => ("Index current / watching", false, false),
        IndexPhase::Complete => ("Index complete", false, false),
        IndexPhase::CompleteWithWarnings => ("Index complete with warnings", false, false),
        IndexPhase::Cancelled => ("Index cancelled", false, false),
        IndexPhase::Failed => ("Index failed", false, false),
    };
    ui.set_status_title(SharedString::from(title));
    ui.set_status_detail(SharedString::from(format!(
        "{} nodes / {} folders pending / {} warnings{}",
        format_number(progress.indexed_nodes),
        format_number(progress.pending_directories),
        format_number(progress.issue_count),
        if progress.resumed { " / resumed" } else { "" }
    )));
    ui.set_node_metric(SharedString::from(format_number(progress.indexed_nodes)));
    ui.set_generation_metric(SharedString::from(format!("GEN {}", progress.generation)));
    ui.set_indexing(indexing);
    ui.set_paused(paused);
}

fn set_failed_status(ui: &AppWindow, message: &str) {
    ui.set_status_title(SharedString::from("Index failed"));
    ui.set_status_detail(SharedString::from(message));
    ui.set_indexing(false);
    ui.set_paused(false);
}

fn apply_selection(ui: &AppWindow, target: Option<&HitTarget>) {
    let Some(target) = target else {
        ui.set_selection_available(false);
        ui.set_selection_openable(false);
        ui.set_selected_path(SharedString::default());
        return;
    };
    ui.set_selection_available(true);
    ui.set_selection_openable(target.openable);
    ui.set_selection_pinned(target.pinned);
    ui.set_selected_path(SharedString::from(&target.path));
    ui.set_selected_x(target.x);
    ui.set_selected_y(target.y);
    ui.set_selected_width(target.width);
    ui.set_selected_height(target.height);
}

fn apply_selection_by_path(ui: &AppWindow, spatial_index: &Arc<Mutex<SpatialIndex>>, path: &str) {
    let target = spatial_index
        .lock()
        .ok()
        .and_then(|index| index.find_path(path).cloned());
    apply_selection(ui, target.as_ref());
}

fn rebuild_project_scene(
    ui: &AppWindow,
    nodes: Vec<StoredNode>,
    layout: LayoutStore,
    spatial_index: &Arc<Mutex<SpatialIndex>>,
    selected_path: Option<String>,
) {
    let generation = ui.get_generation_metric().to_string();
    let weak = ui.as_weak();
    let interaction = Arc::clone(spatial_index);
    thread::spawn(move || {
        let count = nodes.len();
        let prepared = prepare_scene(build_index_scene(&nodes, Some(&layout)), count);
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                apply_prepared_scene(&ui, prepared, &generation, &interaction);
                if let Some(path) = selected_path {
                    apply_selection_by_path(&ui, &interaction, &path);
                }
            }
        });
    });
}

fn open_external_editor(path: PathBuf) -> std::io::Result<()> {
    if let Some(editor) = std::env::var_os("SHITVIEW_EDITOR")
        .or_else(|| std::env::var_os("VISUAL"))
        .or_else(|| std::env::var_os("EDITOR"))
    {
        Command::new(editor).arg(path).spawn()?;
        return Ok(());
    }
    open_with_system_default(path)
}

#[cfg(windows)]
fn open_with_system_default(path: PathBuf) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    Command::new("cmd.exe")
        .creation_flags(CREATE_NO_WINDOW)
        .arg("/C")
        .arg("start")
        .arg("")
        .arg(path)
        .spawn()?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn open_with_system_default(path: PathBuf) -> std::io::Result<()> {
    Command::new("open").arg(path).spawn()?;
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open_with_system_default(path: PathBuf) -> std::io::Result<()> {
    Command::new("xdg-open").arg(path).spawn()?;
    Ok(())
}

fn display_path_for_ui(path: &PathBuf) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    normalized
        .strip_prefix("//?/UNC/")
        .map(|unc| format!("//{unc}"))
        .or_else(|| normalized.strip_prefix("//?/").map(str::to_owned))
        .unwrap_or(normalized)
}

fn build_index_scene(indexed_nodes: &[StoredNode], layout: Option<&LayoutStore>) -> SyntheticScene {
    const MODULE_COLUMNS_REAL: usize = 4;
    const MODULE_CAPACITY: usize = 360;
    const INNER_COLUMNS: usize = 18;

    let started = Instant::now();
    let root_path = indexed_nodes
        .iter()
        .find(|node| node.depth == 0)
        .map(|node| node.display_path.as_str())
        .unwrap_or("");
    let mut grouped = BTreeMap::<String, Vec<&StoredNode>>::new();
    for node in indexed_nodes {
        let group = if node.depth == 0 {
            "PROJECT".to_owned()
        } else {
            node.display_path
                .strip_prefix(root_path)
                .unwrap_or(&node.display_path)
                .trim_start_matches('/')
                .split('/')
                .next()
                .filter(|value| !value.is_empty())
                .unwrap_or("PROJECT")
                .to_owned()
        };
        grouped.entry(group).or_default().push(node);
    }
    let mut modules_data = Vec::new();
    for (group, nodes) in grouped {
        let chunks = nodes.len().div_ceil(MODULE_CAPACITY);
        for (index, chunk) in nodes.chunks(MODULE_CAPACITY).enumerate() {
            let label = if chunks > 1 {
                format!("{} {}/{}", group, index + 1, chunks)
            } else {
                group.clone()
            };
            modules_data.push((label, chunk.to_vec()));
        }
    }

    let node_width = 44.0;
    let node_height = 22.0;
    let gap_x = 14.0;
    let gap_y = 12.0;
    let module_padding_x = 46.0;
    let module_header = 58.0;
    let module_width = module_padding_x * 2.0 + INNER_COLUMNS as f32 * (node_width + gap_x);
    let module_gap_x = 180.0;
    let module_gap_y = 180.0;
    let outer_margin = 180.0;
    let scene_width = outer_margin * 2.0
        + MODULE_COLUMNS_REAL as f32 * module_width
        + (MODULE_COLUMNS_REAL - 1) as f32 * module_gap_x;
    let palette = [
        (0x55, 0xc7, 0x8a),
        (0x4b, 0xa8, 0xa2),
        (0x56, 0x93, 0xc7),
        (0xc1, 0x99, 0x52),
        (0xa5, 0x75, 0xb9),
        (0xb5, 0x67, 0x75),
    ];
    let mut nodes = Vec::with_capacity(indexed_nodes.len());
    let mut modules = Vec::with_capacity(modules_data.len());
    let mut segments = Vec::with_capacity(indexed_nodes.len() + 128);
    let mut labels = Vec::with_capacity(modules_data.len());
    let mut hit_targets = Vec::with_capacity(indexed_nodes.len());
    let mut column_heights = [outer_margin; MODULE_COLUMNS_REAL];

    for (module_index, (label, module_nodes)) in modules_data.into_iter().enumerate() {
        let column = column_heights
            .iter()
            .enumerate()
            .min_by(|(_, left), (_, right)| left.total_cmp(right))
            .map(|(index, _)| index)
            .unwrap_or(module_index % MODULE_COLUMNS_REAL);
        let rows = module_nodes.len().max(1).div_ceil(INNER_COLUMNS);
        let module_height = module_header + 34.0 + rows as f32 * (node_height + gap_y);
        let module_x = outer_margin + column as f32 * (module_width + module_gap_x);
        let module_y = column_heights[column];
        let (red, green, blue) = palette[module_index % palette.len()];
        push_module_frame(
            &mut modules,
            module_x,
            module_y,
            module_width,
            module_height,
            (red, green, blue),
        );
        let bus_x = module_x + 28.0;
        let first_center_y = module_y + module_header + node_height * 0.5;
        segments.push(SceneSegment {
            x: bus_x,
            y: first_center_y,
            width: 1.5,
            height: ((rows.saturating_sub(1)) as f32 * (node_height + gap_y)).max(1.5),
            color: Color::from_argb_u8(170, 0x55, 0xc7, 0x8a),
        });
        labels.push(SceneLabel {
            x: module_x + 20.0,
            y: module_y + 17.0,
            text: SharedString::from(format!("{} / {}", label, module_nodes.len())),
        });
        for (local_index, node) in module_nodes.into_iter().enumerate() {
            let local_column = local_index % INNER_COLUMNS;
            let local_row = local_index / INNER_COLUMNS;
            let auto_x = module_x + module_padding_x + local_column as f32 * (node_width + gap_x);
            let auto_y = module_y + module_header + local_row as f32 * (node_height + gap_y);
            let saved = layout.and_then(|layout| {
                layout.entry_for(node.stable_id.as_deref(), &node.display_path)
            });
            let (x, y, pinned) = saved
                .map(|entry| (entry.x.max(60.0), entry.y.max(60.0), entry.pinned))
                .unwrap_or((auto_x, auto_y, false));
            let is_directory = node.kind == NodeKind::Directory;
            let rendered_width = if is_directory { node_width + 4.0 } else { node_width };
            let rendered_height = if is_directory { node_height + 2.0 } else { node_height };
            nodes.push(SceneRect {
                x,
                y,
                width: rendered_width,
                height: rendered_height,
                fill: Color::from_argb_u8(
                    if is_directory { 215 } else { 155 },
                    red / 5 + 12,
                    green / 5 + 28,
                    blue / 5 + 16,
                ),
                border: Color::from_argb_u8(220, red, green, blue),
            });
            hit_targets.push(HitTarget {
                x,
                y,
                width: rendered_width,
                height: rendered_height,
                path: node.display_path.clone(),
                openable: node.kind == NodeKind::File,
                stable_id: node.stable_id.clone(),
                pinned,
            });
            if local_column > 0 {
                segments.push(SceneSegment {
                    x: x - gap_x,
                    y: y + node_height * 0.5,
                    width: gap_x,
                    height: 1.5,
                    color: Color::from_argb_u8(210, 0x67, 0xd9, 0x9a),
                });
            } else {
                segments.push(SceneSegment {
                    x: bus_x,
                    y: y + node_height * 0.5,
                    width: (x - bus_x).max(1.0),
                    height: 1.5,
                    color: Color::from_argb_u8(210, 0x67, 0xd9, 0x9a),
                });
            }
        }
        column_heights[column] = module_y + module_height + module_gap_y;
    }
    let scene_height = column_heights.iter().copied().fold(outer_margin, f32::max);
    let mut board_grid = Vec::new();
    add_grid(&mut board_grid, scene_width, scene_height);
    board_grid.extend(segments);
    segments = board_grid;
    let mut scene = SyntheticScene {
        nodes,
        modules,
        segments,
        labels,
        hit_targets,
        width: scene_width,
        height: scene_height,
        elapsed: started.elapsed(),
    };
    normalize_scene_bounds(&mut scene);
    scene
}

fn normalize_scene_bounds(scene: &mut SyntheticScene) {
    let max_x = scene
        .hit_targets
        .iter()
        .map(|target| target.x + target.width)
        .chain(scene.modules.iter().map(|rect| rect.x + rect.width))
        .fold(scene.width, f32::max);
    let max_y = scene
        .hit_targets
        .iter()
        .map(|target| target.y + target.height)
        .chain(scene.modules.iter().map(|rect| rect.y + rect.height))
        .fold(scene.height, f32::max);
    scene.width = max_x + 60.0;
    scene.height = max_y + 60.0;
}

#[cfg(windows)]
fn choose_project_folder() -> Option<PathBuf> {
    let script = "Add-Type -AssemblyName System.Windows.Forms; $dialog = New-Object System.Windows.Forms.FolderBrowserDialog; $dialog.Description = 'Open project folder'; $dialog.ShowNewFolderButton = $false; if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) { [Console]::Out.Write($dialog.SelectedPath) }";
    command_folder_output("powershell.exe", &["-NoProfile", "-STA", "-Command", script])
}

#[cfg(target_os = "macos")]
fn choose_project_folder() -> Option<PathBuf> {
    command_folder_output(
        "osascript",
        &["-e", "POSIX path of (choose folder with prompt \"Open project folder\")"],
    )
}

#[cfg(all(unix, not(target_os = "macos")))]
fn choose_project_folder() -> Option<PathBuf> {
    command_folder_output("zenity", &["--file-selection", "--directory", "--title=Open project folder"])
        .or_else(|| command_folder_output("kdialog", &["--getexistingdirectory", "."]))
}

fn command_folder_output(program: &str, arguments: &[&str]) -> Option<PathBuf> {
    let output = Command::new(program).args(arguments).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

fn build_scene(count: usize) -> SyntheticScene {
    let started = Instant::now();
    let nodes_per_module = count.div_ceil(MODULE_COUNT);
    let inner_columns = ((nodes_per_module as f32).sqrt().ceil() as usize).max(1);
    let inner_rows = nodes_per_module.div_ceil(inner_columns);

    let node_width = 44.0;
    let node_height = 22.0;
    let gap_x = 14.0;
    let gap_y = 12.0;
    let module_padding_x = 46.0;
    let module_header = 58.0;
    let module_width = module_padding_x * 2.0 + inner_columns as f32 * (node_width + gap_x);
    let module_height = module_header + 34.0 + inner_rows as f32 * (node_height + gap_y);
    let module_gap_x = 180.0;
    let module_gap_y = 180.0;
    let outer_margin = 180.0;

    let palette = [
        (0x55, 0xc7, 0x8a),
        (0x4b, 0xa8, 0xa2),
        (0x56, 0x93, 0xc7),
        (0xc1, 0x99, 0x52),
        (0xa5, 0x75, 0xb9),
        (0xb5, 0x67, 0x75),
    ];

    let mut nodes = Vec::with_capacity(count);
    let mut modules = Vec::with_capacity(MODULE_COUNT);
    let mut segments = Vec::with_capacity(count + 128);
    let mut labels = Vec::with_capacity(MODULE_COUNT);
    let mut hit_targets = Vec::with_capacity(count);

    let scene_width = outer_margin * 2.0
        + MODULE_COLUMNS as f32 * module_width
        + (MODULE_COLUMNS - 1) as f32 * module_gap_x;
    let module_rows = MODULE_COUNT.div_ceil(MODULE_COLUMNS);
    let scene_height = outer_margin * 2.0
        + module_rows as f32 * module_height
        + (module_rows - 1) as f32 * module_gap_y;

    add_grid(&mut segments, scene_width, scene_height);

    let mut remaining = count;
    for module_index in 0..MODULE_COUNT {
        let column = module_index % MODULE_COLUMNS;
        let row = module_index / MODULE_COLUMNS;
        let module_x = outer_margin + column as f32 * (module_width + module_gap_x);
        let module_y = outer_margin + row as f32 * (module_height + module_gap_y);
        let (red, green, blue) = palette[module_index % palette.len()];

        push_module_frame(
            &mut modules,
            module_x,
            module_y,
            module_width,
            module_height,
            (red, green, blue),
        );
        labels.push(SceneLabel {
            x: module_x + 20.0,
            y: module_y + 17.0,
            text: SharedString::from(format!("MODULE {:02}", module_index + 1)),
        });

        let module_node_count = remaining.min(nodes_per_module);
        remaining -= module_node_count;
        for local_index in 0..module_node_count {
            let local_column = local_index % inner_columns;
            let local_row = local_index / inner_columns;
            let x = module_x + module_padding_x + local_column as f32 * (node_width + gap_x);
            let y = module_y + module_header + local_row as f32 * (node_height + gap_y);
            let is_directory = local_index % 17 == 0;
            let rendered_width = if is_directory { node_width + 4.0 } else { node_width };
            let rendered_height = if is_directory { node_height + 2.0 } else { node_height };

            nodes.push(SceneRect {
                x,
                y,
                width: rendered_width,
                height: rendered_height,
                fill: Color::from_argb_u8(
                    if is_directory { 205 } else { 155 },
                    red / 5 + 12,
                    green / 5 + 28,
                    blue / 5 + 16,
                ),
                border: Color::from_argb_u8(220, red, green, blue),
            });
            hit_targets.push(HitTarget {
                x,
                y,
                width: rendered_width,
                height: rendered_height,
                path: format!("synthetic://module-{}/node-{}", module_index + 1, local_index + 1),
                openable: false,
                stable_id: None,
                pinned: false,
            });

            if local_column > 0 {
                segments.push(SceneSegment {
                    x: x - gap_x,
                    y: y + node_height * 0.5,
                    width: gap_x,
                    height: 1.5,
                    color: Color::from_argb_u8(210, 0x67, 0xd9, 0x9a),
                });
            }
        }
    }

    SyntheticScene {
        nodes,
        modules,
        segments,
        labels,
        hit_targets,
        width: scene_width,
        height: scene_height,
        elapsed: started.elapsed(),
    }
}

fn add_grid(segments: &mut Vec<SceneSegment>, width: f32, height: f32) {
    let grid_color = Color::from_argb_u8(38, 0x55, 0x7a, 0x64);
    let major_color = Color::from_argb_u8(62, 0x68, 0x9a, 0x73);
    let mut x = 0.0;
    while x <= width {
        segments.push(SceneSegment {
            x,
            y: 0.0,
            width: 1.0,
            height,
            color: if (x as i32) % 480 == 0 { major_color } else { grid_color },
        });
        x += 120.0;
    }
    let mut y = 0.0;
    while y <= height {
        segments.push(SceneSegment {
            x: 0.0,
            y,
            width,
            height: 1.0,
            color: if (y as i32) % 480 == 0 { major_color } else { grid_color },
        });
        y += 120.0;
    }
    let pad_color = Color::from_argb_u8(105, 0x6a, 0xb0, 0x78);
    let mut pad_x = 60.0;
    while pad_x < width {
        let mut pad_y = 60.0;
        while pad_y < height {
            segments.push(SceneSegment {
                x: pad_x,
                y: pad_y,
                width: 3.0,
                height: 3.0,
                color: pad_color,
            });
            pad_y += 120.0;
        }
        pad_x += 120.0;
    }
}

fn format_number(value: usize) -> String {
    let digits = value.to_string();
    let mut output = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            output.push(',');
        }
        output.push(character);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{build_index_scene, build_scene, raster_dimensions, rasterize_scene, LayoutEntry, LayoutStore, SpatialIndex};
    use shitview_core::NodeKind;
    use shitview_storage::StoredNode;
    use std::time::Instant;

    #[test]
    fn batch_rasterizer_handles_target_scene_sizes() {
        for count in [1_000, 5_000, 10_000] {
            let scene = build_scene(count);
            let started = Instant::now();
            let _pixels = rasterize_scene(&scene);
            let elapsed = started.elapsed();
            let (_, width, height) = raster_dimensions(&scene);
            assert!(width <= 4_096);
            assert!(height <= 4_096);
            assert!(width > 0 && height > 0);
            eprintln!(
                "batch scene nodes={count} image={}x{} raster_ms={:.2}",
                width,
                height,
                elapsed.as_secs_f64() * 1_000.0
            );
        }
    }

    #[test]
    fn spatial_index_hits_one_of_ten_thousand_nodes() {
        let scene = build_scene(10_000);
        let expected = scene.hit_targets[9_321].clone();
        let index = SpatialIndex::new(scene.hit_targets);
        let hit = index
            .hit(
                expected.x + expected.width * 0.5,
                expected.y + expected.height * 0.5,
            )
            .expect("target should be found in its grid cell");
        assert_eq!(hit.path, expected.path);
        assert!(index.hit(-10.0, -10.0).is_none());
    }

    #[test]
    fn indexed_layout_is_stable_when_scan_order_changes() {
        let root = StoredNode {
            stable_id: Some(vec![1]),
            display_path: "H:/project".to_owned(),
            display_name: "project".to_owned(),
            kind: NodeKind::Directory,
            depth: 0,
            size_bytes: 0,
        };
        let file = StoredNode {
            stable_id: Some(vec![2]),
            display_path: "H:/project/src/main.rs".to_owned(),
            display_name: "main.rs".to_owned(),
            kind: NodeKind::File,
            depth: 2,
            size_bytes: 12,
        };
        let mut layout = LayoutStore::default();
        layout.set(
            file.stable_id.as_deref(),
            &file.display_path,
            LayoutEntry { x: 777.0, y: 333.0, pinned: true },
        );
        for nodes in [vec![root.clone(), file.clone()], vec![file.clone(), root.clone()]] {
            let scene = build_index_scene(&nodes, Some(&layout));
            let target = scene
                .hit_targets
                .iter()
                .find(|target| target.path == file.display_path)
                .unwrap();
            assert_eq!((target.x, target.y), (777.0, 333.0));
            assert!(target.pinned);
        }
    }
}
