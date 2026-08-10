use slint::{Color, ModelRc, SharedString, Timer, TimerMode, VecModel};
use std::rc::Rc;
use std::time::{Duration, Instant};

slint::include_modules!();

const MODULE_COUNT: usize = 12;
const MODULE_COLUMNS: usize = 4;

struct SyntheticScene {
    nodes: Vec<SceneRect>,
    modules: Vec<SceneRect>,
    segments: Vec<SceneSegment>,
    labels: Vec<SceneLabel>,
    width: f32,
    height: f32,
    elapsed: Duration,
}

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    apply_scene(&ui, 1_000);

    let weak = ui.as_weak();
    ui.on_select_density(move |count| {
        if let Some(ui) = weak.upgrade() {
            apply_scene(&ui, count.clamp(1_000, 10_000) as usize);
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
    pulse_timer.start(TimerMode::Repeated, Duration::from_millis(33), move || {
        if let Some(ui) = weak.upgrade() {
            let phase = (pulse_started.elapsed().as_secs_f32() * 1.8).sin();
            ui.set_pulse((phase + 1.0) * 0.5);
        }
    });

    ui.run()
}

fn apply_scene(ui: &AppWindow, count: usize) {
    let scene = build_scene(count);
    let primitive_count = scene.nodes.len() + scene.modules.len() + scene.segments.len();
    ui.set_graph_nodes(ModelRc::from(Rc::new(VecModel::from(scene.nodes))));
    ui.set_graph_modules(ModelRc::from(Rc::new(VecModel::from(scene.modules))));
    ui.set_graph_segments(ModelRc::from(Rc::new(VecModel::from(scene.segments))));
    ui.set_graph_labels(ModelRc::from(Rc::new(VecModel::from(scene.labels))));
    ui.set_scene_width(scene.width);
    ui.set_scene_height(scene.height);
    ui.set_node_metric(SharedString::from(format_number(count)));
    ui.set_primitive_metric(SharedString::from(format_number(primitive_count)));
    ui.set_generation_metric(SharedString::from(format!(
        "{:.2} ms",
        scene.elapsed.as_secs_f64() * 1_000.0
    )));
}

fn build_scene(count: usize) -> SyntheticScene {
    let started = Instant::now();
    let nodes_per_module = count.div_ceil(MODULE_COUNT);
    let inner_columns = ((nodes_per_module as f32).sqrt().ceil() as usize).max(1);
    let inner_rows = nodes_per_module.div_ceil(inner_columns);

    let node_width = 38.0;
    let node_height = 18.0;
    let gap_x = 12.0;
    let gap_y = 11.0;
    let module_padding_x = 46.0;
    let module_header = 58.0;
    let module_width = module_padding_x * 2.0 + inner_columns as f32 * (node_width + gap_x);
    let module_height = module_header + 34.0 + inner_rows as f32 * (node_height + gap_y);
    let module_gap_x = 88.0;
    let module_gap_y = 92.0;
    let outer_margin = 90.0;

    let palette = [
        (0x3d, 0x83, 0xc6),
        (0x31, 0xa6, 0xa0),
        (0x76, 0x63, 0xb6),
        (0xc0, 0x76, 0x3e),
        (0x4f, 0x9d, 0x62),
        (0xb4, 0x59, 0x82),
    ];

    let mut nodes = Vec::with_capacity(count);
    let mut modules = Vec::with_capacity(MODULE_COUNT);
    let mut segments = Vec::with_capacity(count + 128);
    let mut labels = Vec::with_capacity(MODULE_COUNT);

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

        modules.push(SceneRect {
            x: module_x,
            y: module_y,
            width: module_width,
            height: module_height,
            fill: Color::from_argb_u8(48, red, green, blue),
            border: Color::from_argb_u8(150, red, green, blue),
        });
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

            nodes.push(SceneRect {
                x,
                y,
                width: if is_directory { node_width + 4.0 } else { node_width },
                height: if is_directory { node_height + 2.0 } else { node_height },
                fill: Color::from_argb_u8(
                    if is_directory { 205 } else { 155 },
                    red.saturating_sub(20),
                    green.saturating_sub(12),
                    blue.saturating_sub(8),
                ),
                border: Color::from_argb_u8(220, red, green, blue),
            });

            if local_column > 0 {
                segments.push(SceneSegment {
                    x: x - gap_x,
                    y: y + node_height * 0.5,
                    width: gap_x,
                    height: 1.5,
                    color: Color::from_argb_u8(190, 0x48, 0xe1, 0x91),
                });
            }
        }
    }

    SyntheticScene {
        nodes,
        modules,
        segments,
        labels,
        width: scene_width,
        height: scene_height,
        elapsed: started.elapsed(),
    }
}

fn add_grid(segments: &mut Vec<SceneSegment>, width: f32, height: f32) {
    let grid_color = Color::from_argb_u8(45, 0x52, 0x60, 0x70);
    let mut x = 0.0;
    while x <= width {
        segments.push(SceneSegment {
            x,
            y: 0.0,
            width: 1.0,
            height,
            color: grid_color,
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
            color: grid_color,
        });
        y += 120.0;
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
