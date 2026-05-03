use crate::analyzer::AnalyzerData;
use crate::model::{format_value, parse_value, ControlId, Snapshot, ValueKind, ALL_CONTROLS};
use crate::{MidiLearnShared, MIDI_WAITING_FOR_CONTROL};
use nih_plug_egui::egui::{
    self, Color32, Context, FontFamily, FontId, Pos2, Rect, Response, Sense, Stroke, Ui, UiBuilder,
    Vec2,
};
use nih_plug_egui::resizable_window::ResizableWindow;
use nih_plug_egui::EguiState;
use parking_lot::Mutex;
use std::sync::atomic::Ordering;
use std::sync::Arc;

const BASE_W: f32 = 1180.0;
const BASE_H: f32 = 760.0;
const MIN_EDITOR_W: f32 = 620.0;
const MIN_EDITOR_H: f32 = 460.0;

const BLACK: Color32 = Color32::from_rgb(2, 2, 8);
const PANEL: Color32 = Color32::from_rgb(8, 7, 20);
const PANEL_2: Color32 = Color32::from_rgb(12, 10, 30);
const PANEL_3: Color32 = Color32::from_rgb(18, 14, 42);
const TEXT: Color32 = Color32::from_rgb(218, 235, 255);
const TEXT_DIM: Color32 = Color32::from_rgb(112, 142, 182);
const CYAN: Color32 = Color32::from_rgb(0, 220, 255);
const MAGENTA: Color32 = Color32::from_rgb(255, 38, 196);
const PURPLE: Color32 = Color32::from_rgb(150, 72, 255);
const GREEN: Color32 = Color32::from_rgb(0, 235, 132);
const AMBER: Color32 = Color32::from_rgb(255, 184, 58);
const RED: Color32 = Color32::from_rgb(255, 64, 82);

#[derive(Clone, Copy, Debug)]
pub struct MeterSnapshot {
    pub peak_db: f32,
    pub gain_reduction_db: f32,
}

#[derive(Clone, Debug)]
pub struct GuiParams {
    pub snapshot: Snapshot,
    pub meters: MeterSnapshot,
}

#[derive(Clone, Copy, Debug)]
pub struct ControlChange {
    pub id: ControlId,
    pub value: f64,
}

#[derive(Clone, Debug, Default)]
pub struct GuiChanges {
    pub changes: Vec<ControlChange>,
}

impl GuiChanges {
    fn set(&mut self, id: ControlId, value: f64) {
        self.changes.push(ControlChange {
            id,
            value: id.spec().clamp(value),
        });
    }

    fn apply_snapshot(&mut self, snapshot: Snapshot) {
        for id in ALL_CONTROLS {
            self.set(id, snapshot.get(id));
        }
    }
}

#[derive(Clone, Debug)]
struct NumInput {
    id: ControlId,
    value: String,
}

#[derive(Clone, Copy, Debug)]
struct DragState {
    id: ControlId,
    current_unit: f64,
}

#[derive(Clone, Copy, Debug)]
struct ChoiceDropdown {
    id: ControlId,
    anchor: Rect,
    accent: Color32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActiveTab {
    Global,
    Distortion,
    Filter,
    Compressor,
}

impl ActiveTab {
    const ALL: [Self; 4] = [
        Self::Global,
        Self::Distortion,
        Self::Filter,
        Self::Compressor,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Global => "Global",
            Self::Distortion => "Distortion",
            Self::Filter => "Filter",
            Self::Compressor => "Compressor",
        }
    }

    fn accent(self) -> Color32 {
        match self {
            Self::Global => CYAN,
            Self::Distortion => MAGENTA,
            Self::Filter => PURPLE,
            Self::Compressor => GREEN,
        }
    }
}

pub struct NebulaClusterGui {
    analyzer: Arc<Mutex<AnalyzerData>>,
    midi_learn: Arc<MidiLearnShared>,
    num_input: Option<NumInput>,
    presets: Vec<(String, Snapshot)>,
    preset_name: String,
    undo_stack: Vec<Snapshot>,
    redo_stack: Vec<Snapshot>,
    state_a: Snapshot,
    state_b: Snapshot,
    active_state_is_a: bool,
    active_tab: ActiveTab,
    dragging: Option<DragState>,
    preset_dropdown_open: bool,
    choice_dropdown: Option<ChoiceDropdown>,
    midi_menu_open: bool,
    cleanup_open: bool,
    chaos_seed: u64,
}

struct ControlDrawCtx<'a> {
    gui: &'a mut NebulaClusterGui,
    params: &'a GuiParams,
    changes: &'a mut GuiChanges,
    scale: f32,
    accent: Color32,
}

impl NebulaClusterGui {
    pub fn new(analyzer: Arc<Mutex<AnalyzerData>>, midi_learn: Arc<MidiLearnShared>) -> Self {
        Self {
            analyzer,
            midi_learn,
            num_input: None,
            presets: Vec::new(),
            preset_name: String::from("Preset 1"),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            state_a: Snapshot::default(),
            state_b: Snapshot::default(),
            active_state_is_a: true,
            active_tab: ActiveTab::Global,
            dragging: None,
            preset_dropdown_open: false,
            choice_dropdown: None,
            midi_menu_open: false,
            cleanup_open: false,
            chaos_seed: 0x4e43_6c75_7374_6572,
        }
    }
}

impl Drop for NebulaClusterGui {
    fn drop(&mut self) {
        self.midi_learn.save_current_mapping();
    }
}

pub fn draw(
    ctx: &Context,
    egui_state: &EguiState,
    gui: &mut NebulaClusterGui,
    params: &GuiParams,
) -> GuiChanges {
    let mut changes = GuiChanges::default();
    let mut style = (*ctx.style()).clone();
    style.visuals.panel_fill = BLACK;
    style.visuals.window_fill = PANEL;
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.widgets.inactive.bg_fill = PANEL_2;
    style.visuals.widgets.hovered.bg_fill = PANEL_3;
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(22, 18, 52);
    style.spacing.item_spacing = Vec2::new(8.0, 6.0);
    ctx.set_style(style);

    let (win_w, win_h) = egui_state.size();
    let scale = (win_w as f32 / BASE_W)
        .min(win_h as f32 / BASE_H)
        .clamp(0.82, 1.45);
    if !ctx.input(|input| input.pointer.primary_down()) {
        gui.dragging = None;
    }

    ResizableWindow::new("nebula_cluster_editor")
        .min_size(Vec2::new(MIN_EDITOR_W, MIN_EDITOR_H))
        .show(ctx, egui_state, |ui| {
            let rect = ui.max_rect();
            draw_background(ui, rect, scale);
            let mut content = rect.shrink(14.0 * scale);

            let header_h = 68.0 * scale;
            let header = Rect::from_min_size(content.min, Vec2::new(content.width(), header_h));
            draw_header(ui, header, scale);
            content.min.y += header_h + 10.0 * scale;

            let toolbar_h = toolbar_height(content.width(), scale);
            let toolbar = Rect::from_min_size(content.min, Vec2::new(content.width(), toolbar_h));
            draw_toolbar(ui, toolbar, gui, params, &mut changes, scale);
            content.min.y += toolbar_h + 10.0 * scale;

            let analyzer_h = analyzer_height(content.height(), scale);
            let analyzer = Rect::from_min_size(content.min, Vec2::new(content.width(), analyzer_h));
            draw_global_analyzer(ui, analyzer, gui, params, scale);
            content.min.y += analyzer_h + 10.0 * scale;

            let tabs_h = tab_bar_height(content.width(), scale);
            let tabs = Rect::from_min_size(content.min, Vec2::new(content.width(), tabs_h));
            draw_tabs(ui, tabs, gui, scale);
            content.min.y += tabs_h + 10.0 * scale;

            draw_active_tab(ui, content, gui, params, &mut changes, scale);
        });

    draw_num_popup(ctx, gui, &mut changes);
    draw_preset_popup(ctx, gui, params, &mut changes);
    draw_choice_dropdown(ctx, gui, params, &mut changes, scale);
    draw_midi_menu(ctx, gui);
    changes
}

fn draw_background(ui: &mut Ui, rect: Rect, scale: f32) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, BLACK);

    let horizon_y = rect.min.y + rect.height() * 0.42;
    painter.rect_filled(
        Rect::from_min_max(rect.min, Pos2::new(rect.max.x, horizon_y)),
        0.0,
        Color32::from_rgb(4, 3, 16),
    );
    painter.line_segment(
        [
            Pos2::new(rect.min.x, horizon_y),
            Pos2::new(rect.max.x, horizon_y),
        ],
        Stroke::new(1.0, rgba(CYAN, 90)),
    );

    let grid_color = rgba(PURPLE, 56);
    let step = 42.0 * scale;
    let mut x = rect.min.x;
    while x < rect.max.x {
        painter.line_segment(
            [
                Pos2::new(x, horizon_y),
                Pos2::new(x - rect.width() * 0.18, rect.max.y),
            ],
            Stroke::new(1.0, grid_color),
        );
        x += step;
    }
    let mut y = horizon_y + step;
    while y < rect.max.y {
        painter.line_segment(
            [Pos2::new(rect.min.x, y), Pos2::new(rect.max.x, y)],
            Stroke::new(1.0, grid_color),
        );
        y += step * 0.75;
    }
}

fn draw_header(ui: &mut Ui, rect: Rect, scale: f32) {
    let painter = ui.painter_at(rect);
    panel(
        painter.clone(),
        rect,
        7.0 * scale,
        rgba(PANEL, 236),
        rgba(CYAN, 110),
    );
    painter.text(
        Pos2::new(rect.min.x + 20.0 * scale, rect.center().y - 10.0),
        egui::Align2::LEFT_CENTER,
        "Nebula Cluster",
        FontId::new(scaled_font(28.0, scale), FontFamily::Proportional),
        TEXT,
    );
    painter.text(
        Pos2::new(rect.min.x + 22.0 * scale, rect.center().y + 18.0),
        egui::Align2::LEFT_CENTER,
        "Made by Nebula Audio  |  MIT open-source  |  v1.0",
        FontId::new(scaled_font(12.0, scale), FontFamily::Proportional),
        TEXT_DIM,
    );
    if rect.width() > 700.0 * scale {
        painter.text(
            Pos2::new(rect.max.x - 18.0 * scale, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            "64-bit f64 DSP",
            FontId::new(scaled_font(13.0, scale), FontFamily::Proportional),
            CYAN,
        );
    }
}

fn draw_toolbar(
    ui: &mut Ui,
    rect: Rect,
    gui: &mut NebulaClusterGui,
    params: &GuiParams,
    changes: &mut GuiChanges,
    scale: f32,
) {
    panel(
        ui.painter_at(rect),
        rect,
        7.0 * scale,
        rgba(PANEL, 232),
        rgba(PURPLE, 90),
    );
    let pad = 8.0 * scale;
    let gap = 6.0 * scale;
    let button_h = 28.0 * scale;
    let mut flow = ToolbarFlow::new(rect, pad, gap, button_h);

    let save_rect = flow.next(66.0 * scale);
    if toolbar_button(ui, save_rect, "Save", false, CYAN, scale).clicked() {
        gui.presets.push((gui.preset_name.clone(), params.snapshot));
        gui.preset_name = format!("Preset {}", gui.presets.len() + 1);
    }

    let preset_rect = flow.next(132.0 * scale);
    let preset_label = gui
        .presets
        .last()
        .map(|preset| preset.0.as_str())
        .unwrap_or("Presets");
    if toolbar_button(
        ui,
        preset_rect,
        preset_label,
        gui.preset_dropdown_open,
        CYAN,
        scale,
    )
    .clicked()
    {
        gui.preset_dropdown_open = !gui.preset_dropdown_open;
    }

    let name_rect = flow.next(116.0 * scale);
    draw_preset_name_field(ui, name_rect, &mut gui.preset_name, scale);

    flow.add_gap(4.0 * scale);
    let undo_rect = flow.next(60.0 * scale);
    if toolbar_button(ui, undo_rect, "Undo", false, PURPLE, scale).clicked() {
        if let Some(snapshot) = gui.undo_stack.pop() {
            gui.redo_stack.push(params.snapshot);
            changes.apply_snapshot(snapshot);
        }
    }

    let redo_rect = flow.next(60.0 * scale);
    if toolbar_button(ui, redo_rect, "Redo", false, PURPLE, scale).clicked() {
        if let Some(snapshot) = gui.redo_stack.pop() {
            gui.undo_stack.push(params.snapshot);
            changes.apply_snapshot(snapshot);
        }
    }

    let ab_label = if gui.active_state_is_a {
        "A/B  A"
    } else {
        "A/B  B"
    };
    let ab_rect = flow.next(70.0 * scale);
    if toolbar_button(ui, ab_rect, ab_label, true, MAGENTA, scale).clicked() {
        if gui.active_state_is_a {
            gui.state_a = params.snapshot;
            gui.active_state_is_a = false;
            push_undo(gui, params.snapshot);
            changes.apply_snapshot(gui.state_b);
        } else {
            gui.state_b = params.snapshot;
            gui.active_state_is_a = true;
            push_undo(gui, params.snapshot);
            changes.apply_snapshot(gui.state_a);
        }
    }

    let learn_state = gui.midi_learn.learning_target.load(Ordering::Acquire);
    let midi_active = learn_state >= 0 || learn_state == MIDI_WAITING_FOR_CONTROL;
    let midi_rect = flow.next(96.0 * scale);
    let midi_label = if learn_state == MIDI_WAITING_FOR_CONTROL {
        "Pick Control"
    } else if learn_state >= 0 {
        "Move MIDI"
    } else {
        "MIDI Learn"
    };
    let midi = toolbar_button(ui, midi_rect, midi_label, midi_active, GREEN, scale);
    if midi.clicked() {
        if midi_active {
            gui.midi_learn.learning_target.store(-1, Ordering::Release);
        } else {
            gui.midi_learn
                .learning_target
                .store(MIDI_WAITING_FOR_CONTROL, Ordering::Release);
        }
        ui.ctx().request_repaint();
    }
    if midi.secondary_clicked() {
        gui.midi_menu_open = true;
    }

    let chaos_rect = flow.next(68.0 * scale);
    if toolbar_button(ui, chaos_rect, "Chaos", false, AMBER, scale).clicked() {
        push_undo(gui, params.snapshot);
        changes.apply_snapshot(chaos_snapshot(gui));
    }

    let oversampling_id = ControlId::Oversampling;
    let ValueKind::Choice(oversampling_labels) = oversampling_id.spec().kind else {
        return;
    };
    let oversampling_current = params
        .snapshot
        .choice(oversampling_id)
        .min(oversampling_labels.len().saturating_sub(1));
    let oversampling_rect = flow.next(148.0 * scale);
    let oversampling_active = gui
        .choice_dropdown
        .is_some_and(|dropdown| dropdown.id == oversampling_id);
    let oversampling_label = format!(
        "Oversampling: {}",
        oversampling_labels[oversampling_current]
    );
    let oversampling = dropdown_button(
        ui,
        oversampling_rect,
        &oversampling_label,
        oversampling_active,
        AMBER,
        scale,
    );
    let oversampling_selected = handle_control_selection(gui, oversampling_id, &oversampling);
    update_choice_anchor(gui, oversampling_id, oversampling_rect, AMBER);
    if oversampling.clicked() && !oversampling_selected {
        toggle_choice_dropdown(gui, oversampling_id, oversampling_rect, AMBER);
    }

    let bypass_id = ControlId::FxBypass;
    let bypass = params.snapshot.bool(bypass_id);
    let bypass_rect = flow.next(98.0 * scale);
    let bypass_response = toolbar_button(ui, bypass_rect, "FX Bypass", bypass, RED, scale);
    let bypass_selected = handle_control_selection(gui, bypass_id, &bypass_response);
    if bypass_response.clicked() && !bypass_selected {
        toggle_control(ui, gui, params, changes, bypass_id);
    }
}

fn draw_tabs(ui: &mut Ui, rect: Rect, gui: &mut NebulaClusterGui, scale: f32) {
    panel(
        ui.painter_at(rect),
        rect,
        7.0 * scale,
        rgba(PANEL, 232),
        rgba(CYAN, 70),
    );

    let pad = 7.0 * scale;
    let gap = 6.0 * scale;
    let tab_h = 30.0 * scale;
    let mut flow = ToolbarFlow::new(rect, pad, gap, tab_h);
    for tab in ActiveTab::ALL {
        let width = tab_width(tab, scale);
        let active = gui.active_tab == tab;
        if toolbar_button(
            ui,
            flow.next(width),
            tab.label(),
            active,
            tab.accent(),
            scale,
        )
        .clicked()
        {
            gui.active_tab = tab;
        }
    }
}

fn draw_active_tab(
    ui: &mut Ui,
    rect: Rect,
    gui: &mut NebulaClusterGui,
    params: &GuiParams,
    changes: &mut GuiChanges,
    scale: f32,
) {
    let mut tab_ui = ui.new_child(UiBuilder::new().max_rect(rect).layout(*ui.layout()));
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_viewport(&mut tab_ui, |ui, _| {
            let width = rect.width().max(1.0);
            ui.set_min_width(width);
            let section_rect = match gui.active_tab {
                ActiveTab::Global => {
                    let height = section_height(6, width, 5, scale);
                    ui.allocate_space(Vec2::new(width, height)).1
                }
                ActiveTab::Distortion => {
                    let height = section_height(13, width, 5, scale);
                    ui.allocate_space(Vec2::new(width, height)).1
                }
                ActiveTab::Filter => {
                    let height = section_height(6, width, 4, scale);
                    ui.allocate_space(Vec2::new(width, height)).1
                }
                ActiveTab::Compressor => {
                    let height = section_height(10, width, 5, scale);
                    ui.allocate_space(Vec2::new(width, height)).1
                }
            };

            match gui.active_tab {
                ActiveTab::Global => {
                    draw_global_section(ui, section_rect, gui, params, changes, scale);
                }
                ActiveTab::Distortion => {
                    draw_distortion_section(ui, section_rect, gui, params, changes, scale);
                }
                ActiveTab::Filter => {
                    draw_filter_section(ui, section_rect, gui, params, changes, scale);
                }
                ActiveTab::Compressor => {
                    draw_compressor_section(ui, section_rect, gui, params, changes, scale);
                }
            }
        });
}

fn draw_global_analyzer(
    ui: &mut Ui,
    rect: Rect,
    gui: &mut NebulaClusterGui,
    params: &GuiParams,
    scale: f32,
) {
    panel(
        ui.painter_at(rect),
        rect,
        7.0 * scale,
        rgba(PANEL, 238),
        rgba(AMBER, 125),
    );

    let painter = ui.painter_at(rect);
    let inner = rect.shrink(14.0 * scale);
    let header_h = 30.0 * scale;
    let gap = 9.0 * scale;
    let title_rect = Rect::from_min_size(inner.min, Vec2::new(inner.width(), header_h));
    painter.text(
        title_rect.left_center(),
        egui::Align2::LEFT_CENTER,
        "Post FX Spectrum Analyzer",
        FontId::new(scaled_font(15.0, scale), FontFamily::Proportional),
        TEXT,
    );
    if title_rect.width() > 540.0 * scale {
        painter.text(
            title_rect.right_center(),
            egui::Align2::RIGHT_CENTER,
            format!(
                "Peak {}   Reduction {}",
                format_db(params.meters.peak_db),
                format_db(params.meters.gain_reduction_db)
            ),
            FontId::new(scaled_font(12.0, scale), FontFamily::Proportional),
            AMBER,
        );
    }

    let data = gui
        .analyzer
        .try_lock()
        .map(|data| data.clone())
        .unwrap_or_default();

    let body = Rect::from_min_max(
        Pos2::new(inner.min.x, title_rect.max.y + gap * 0.55),
        inner.max,
    );
    let (graph_rect, meter_rect) = if body.width() >= 720.0 * scale
        && body.height() >= 112.0 * scale
    {
        let meter_w = (body.width() * 0.24).clamp(190.0 * scale, 260.0 * scale);
        let meter_rect = Rect::from_min_max(Pos2::new(body.max.x - meter_w, body.min.y), body.max);
        let graph_rect =
            Rect::from_min_max(body.min, Pos2::new(meter_rect.min.x - gap, body.max.y));
        (graph_rect, meter_rect)
    } else {
        let meter_h = (body.height() * 0.34).clamp(40.0 * scale, 58.0 * scale);
        let meter_rect = Rect::from_min_size(body.min, Vec2::new(body.width(), meter_h));
        let graph_rect =
            Rect::from_min_max(Pos2::new(body.min.x, meter_rect.max.y + gap), body.max);
        (graph_rect, meter_rect)
    };

    draw_analyzer_meters(&painter, meter_rect, params, scale);

    let graph_h = graph_rect.height().max(1.0);
    let spectrum_h = (graph_h * 0.64).max(1.0);
    let spectrum_rect =
        Rect::from_min_size(graph_rect.min, Vec2::new(graph_rect.width(), spectrum_h));
    let waveform_rect = Rect::from_min_max(
        Pos2::new(graph_rect.min.x, spectrum_rect.max.y + gap),
        graph_rect.max,
    );

    draw_spectrum_curve(
        &painter,
        spectrum_rect,
        &data.magnitudes_db,
        data.sample_rate,
        scale,
    );
    draw_waveform_curve(&painter, waveform_rect, &data.waveform, scale);
}

struct ToolbarFlow {
    min_x: f32,
    max_x: f32,
    x: f32,
    y: f32,
    gap: f32,
    button_h: f32,
}

impl ToolbarFlow {
    fn new(rect: Rect, pad: f32, gap: f32, button_h: f32) -> Self {
        Self {
            min_x: rect.min.x + pad,
            max_x: rect.max.x - pad,
            x: rect.min.x + pad,
            y: rect.min.y + pad,
            gap,
            button_h,
        }
    }

    fn add_gap(&mut self, gap: f32) {
        self.x += gap;
    }

    fn next(&mut self, width: f32) -> Rect {
        let width = width.min((self.max_x - self.min_x).max(1.0));
        if self.x > self.min_x && self.x + width > self.max_x {
            self.x = self.min_x;
            self.y += self.button_h + self.gap;
        }

        let rect = Rect::from_min_size(Pos2::new(self.x, self.y), Vec2::new(width, self.button_h));
        self.x += width + self.gap;
        rect
    }
}

fn toolbar_height(width: f32, scale: f32) -> f32 {
    let pad = 8.0 * scale;
    let row_h = 28.0 * scale;
    let gap = 6.0 * scale;
    let total_button_width = 920.0 * scale;
    let rows = (total_button_width / width.max(1.0)).ceil().clamp(1.0, 2.0);
    pad * 2.0 + rows * row_h + (rows - 1.0) * gap
}

fn analyzer_height(available_height: f32, scale: f32) -> f32 {
    (available_height * 0.36).clamp(160.0 * scale, 260.0 * scale)
}

fn tab_bar_height(width: f32, scale: f32) -> f32 {
    let pad = 7.0 * scale;
    let row_h = 30.0 * scale;
    let gap = 6.0 * scale;
    let total_width = ActiveTab::ALL
        .iter()
        .map(|tab| tab_width(*tab, scale) + gap)
        .sum::<f32>();
    let rows = (total_width / width.max(1.0)).ceil().clamp(1.0, 2.0);
    pad * 2.0 + rows * row_h + (rows - 1.0) * gap
}

fn tab_width(tab: ActiveTab, scale: f32) -> f32 {
    match tab {
        ActiveTab::Global => 92.0 * scale,
        ActiveTab::Distortion => 124.0 * scale,
        ActiveTab::Filter => 88.0 * scale,
        ActiveTab::Compressor => 134.0 * scale,
    }
}

fn section_height(control_count: usize, width: f32, preferred_columns: usize, scale: f32) -> f32 {
    let inner_width = (width - 24.0 * scale).max(1.0);
    let columns = responsive_columns(inner_width, preferred_columns, scale);
    let rows = control_count.div_ceil(columns);
    46.0 * scale
        + rows as f32 * control_cell_height(scale)
        + rows.saturating_sub(1) as f32 * control_gap(scale)
        + 14.0 * scale
}

fn responsive_columns(width: f32, preferred_columns: usize, scale: f32) -> usize {
    let min_cell_width = 116.0 * scale;
    ((width / min_cell_width).floor() as usize).clamp(1, preferred_columns.max(1))
}

fn control_cell_height(scale: f32) -> f32 {
    110.0 * scale
}

fn control_gap(scale: f32) -> f32 {
    10.0 * scale
}

fn section_content_rect(rect: Rect, scale: f32) -> Rect {
    let mut inner = rect.shrink(12.0 * scale);
    inner.min.y += 30.0 * scale;
    inner
}

fn draw_global_section(
    ui: &mut Ui,
    rect: Rect,
    gui: &mut NebulaClusterGui,
    params: &GuiParams,
    changes: &mut GuiChanges,
    scale: f32,
) {
    section_panel(ui, rect, "Global", true, scale);
    let inner = section_content_rect(rect, scale);
    let controls = [
        ControlId::InputLevel,
        ControlId::InputPan,
        ControlId::OutputLevel,
        ControlId::OutputPan,
        ControlId::GlobalMix,
        ControlId::GlobalPhase,
    ];
    let mut control_ctx = ControlDrawCtx {
        gui,
        params,
        changes,
        scale,
        accent: CYAN,
    };
    draw_control_grid(ui, inner, &mut control_ctx, &controls, 7);
}

fn draw_distortion_section(
    ui: &mut Ui,
    rect: Rect,
    gui: &mut NebulaClusterGui,
    params: &GuiParams,
    changes: &mut GuiChanges,
    scale: f32,
) {
    let enabled = params.snapshot.bool(ControlId::DistortionEnabled);
    section_panel(ui, rect, "Distortion", enabled, scale);
    let toggle_rect = section_toggle_rect(rect, scale);
    if toolbar_button(
        ui,
        toggle_rect,
        if enabled { "On" } else { "Off" },
        enabled,
        MAGENTA,
        scale,
    )
    .clicked()
    {
        toggle_control(ui, gui, params, changes, ControlId::DistortionEnabled);
    }

    let inner = section_content_rect(rect, scale);
    let controls = [
        ControlId::DistSaturation,
        ControlId::Harmonic2,
        ControlId::Harmonic3,
        ControlId::Harmonic4,
        ControlId::Harmonic5,
        ControlId::Harmonic6,
        ControlId::Harmonic7,
        ControlId::DistMix,
        ControlId::DistPhase,
        ControlId::DistHpf,
        ControlId::DistHpSlope,
        ControlId::DistLpf,
        ControlId::DistLpSlope,
    ];
    let mut control_ctx = ControlDrawCtx {
        gui,
        params,
        changes,
        scale,
        accent: MAGENTA,
    };
    draw_control_grid(ui, inner, &mut control_ctx, &controls, 5);
}

fn draw_filter_section(
    ui: &mut Ui,
    rect: Rect,
    gui: &mut NebulaClusterGui,
    params: &GuiParams,
    changes: &mut GuiChanges,
    scale: f32,
) {
    let enabled = params.snapshot.bool(ControlId::FilterEnabled);
    section_panel(ui, rect, "Filter", enabled, scale);
    let toggle_rect = section_toggle_rect(rect, scale);
    if toolbar_button(
        ui,
        toggle_rect,
        if enabled { "On" } else { "Off" },
        enabled,
        PURPLE,
        scale,
    )
    .clicked()
    {
        toggle_control(ui, gui, params, changes, ControlId::FilterEnabled);
    }

    let inner = section_content_rect(rect, scale);
    let controls = [
        ControlId::FilterHpf,
        ControlId::FilterHpSlope,
        ControlId::FilterHpRes,
        ControlId::FilterLpf,
        ControlId::FilterLpSlope,
        ControlId::FilterLpRes,
    ];
    let mut control_ctx = ControlDrawCtx {
        gui,
        params,
        changes,
        scale,
        accent: PURPLE,
    };
    draw_control_grid(ui, inner, &mut control_ctx, &controls, 3);
}

fn draw_compressor_section(
    ui: &mut Ui,
    rect: Rect,
    gui: &mut NebulaClusterGui,
    params: &GuiParams,
    changes: &mut GuiChanges,
    scale: f32,
) {
    let enabled = params.snapshot.bool(ControlId::CompressorEnabled);
    section_panel(ui, rect, "Compressor", enabled, scale);
    let toggle_rect = section_toggle_rect(rect, scale);
    if toolbar_button(
        ui,
        toggle_rect,
        if enabled { "On" } else { "Off" },
        enabled,
        GREEN,
        scale,
    )
    .clicked()
    {
        toggle_control(ui, gui, params, changes, ControlId::CompressorEnabled);
    }

    let inner = section_content_rect(rect, scale);
    let controls = [
        ControlId::CompMode,
        ControlId::CompRatio,
        ControlId::CompKnee,
        ControlId::CompMakeup,
        ControlId::CompBoost,
        ControlId::CompAttackThreshold,
        ControlId::CompAttackMs,
        ControlId::CompReleaseThreshold,
        ControlId::CompReleaseMs,
        ControlId::CompHold,
    ];
    let mut control_ctx = ControlDrawCtx {
        gui,
        params,
        changes,
        scale,
        accent: GREEN,
    };
    draw_control_grid(ui, inner, &mut control_ctx, &controls, 10);
}

fn draw_control_grid(
    ui: &mut Ui,
    rect: Rect,
    ctx: &mut ControlDrawCtx<'_>,
    controls: &[ControlId],
    columns: usize,
) {
    let columns = responsive_columns(rect.width(), columns, ctx.scale);
    let gap = control_gap(ctx.scale);
    let cell_w = (rect.width() - gap * (columns.saturating_sub(1) as f32)) / columns as f32;
    let cell_h = control_cell_height(ctx.scale);

    for (index, id) in controls.iter().copied().enumerate() {
        let col = index % columns;
        let row = index / columns;
        let cell = Rect::from_min_size(
            Pos2::new(
                rect.min.x + col as f32 * (cell_w + gap),
                rect.min.y + row as f32 * (cell_h + gap),
            ),
            Vec2::new(cell_w, cell_h),
        );
        draw_control(ui, cell, ctx, id);
    }
}

fn draw_control(ui: &mut Ui, rect: Rect, ctx: &mut ControlDrawCtx<'_>, id: ControlId) {
    match id.spec().kind {
        ValueKind::Boolean => draw_toggle_control(ui, rect, ctx, id),
        ValueKind::Choice(labels) => draw_choice_control(ui, rect, ctx, id, labels),
        _ => draw_knob_control(ui, rect, ctx, id),
    }
}

fn draw_knob_control(ui: &mut Ui, rect: Rect, ctx: &mut ControlDrawCtx<'_>, id: ControlId) {
    let value = ctx.params.snapshot.get(id);
    let spec = id.spec();
    let inner = rect.shrink2(Vec2::new(5.0 * ctx.scale, 5.0 * ctx.scale));
    let label_h = 18.0 * ctx.scale;
    let field_h = 24.0 * ctx.scale;
    let gap = 6.0 * ctx.scale;
    let knob_area = Rect::from_min_max(
        Pos2::new(inner.min.x, inner.min.y + label_h + gap),
        Pos2::new(inner.max.x, inner.max.y - field_h - gap),
    );
    let knob_size = knob_area
        .width()
        .min(knob_area.height())
        .clamp(28.0 * ctx.scale, 58.0 * ctx.scale);
    let center = knob_area.center();
    let knob_rect = Rect::from_center_size(center, Vec2::splat(knob_size));
    let response = ui.allocate_rect(knob_rect.expand(8.0 * ctx.scale), Sense::click_and_drag());
    let knob_selected = handle_control_selection(ctx.gui, id, &response);

    if knob_selected {
        ctx.gui.dragging = None;
    } else if response.double_clicked() {
        push_undo(ctx.gui, ctx.params.snapshot);
        ctx.changes.set(id, spec.default);
    } else if response.dragged() {
        if ctx.gui.dragging.map(|drag| drag.id) != Some(id) {
            push_undo(ctx.gui, ctx.params.snapshot);
            ctx.gui.dragging = Some(DragState {
                id,
                current_unit: spec.unit_from_value(value),
            });
        }
        let delta = ui.input(|input| input.pointer.delta());
        let sensitivity = ui.input(|input| {
            if input.modifiers.shift {
                0.001_15
            } else {
                0.004_4
            }
        });
        if let Some(drag) = &mut ctx.gui.dragging {
            let next = (drag.current_unit + (-delta.y + delta.x * 0.28) as f64 * sensitivity)
                .clamp(0.0, 1.0);
            drag.current_unit = next;
            ctx.changes.set(id, spec.value_from_unit(next));
        }
        ui.ctx().request_repaint();
    }

    let value_text = format_value(id, value);
    let field_rect = Rect::from_center_size(
        Pos2::new(inner.center().x, inner.max.y - field_h * 0.5),
        Vec2::new(inner.width().min(104.0 * ctx.scale), field_h),
    );
    let field_response = ui.allocate_rect(field_rect, Sense::click());
    let field_selected = handle_control_selection(ctx.gui, id, &field_response);
    if field_response.clicked() && !field_selected {
        ctx.gui.num_input = Some(NumInput {
            id,
            value: format_value(id, value),
        });
    }

    let painter = ui.painter_at(rect);
    painter.text(
        Pos2::new(inner.center().x, inner.min.y + label_h * 0.5),
        egui::Align2::CENTER_CENTER,
        spec.name,
        FontId::new(
            fitted_font(spec.name, 11.0, ctx.scale, inner.width()),
            FontFamily::Proportional,
        ),
        TEXT_DIM,
    );
    if matches!(spec.kind, ValueKind::Pan) {
        paint_pan_knob(&painter, knob_rect, value, ctx.accent, ctx.scale);
    } else {
        paint_knob(
            &painter,
            knob_rect,
            spec.unit_from_value(value),
            ctx.accent,
            ctx.scale,
        );
    }
    painter.rect_filled(field_rect, 4.0 * ctx.scale, PANEL_2);
    painter.rect_stroke(
        field_rect,
        4.0 * ctx.scale,
        Stroke::new(
            1.0,
            rgba(ctx.accent, if field_response.hovered() { 180 } else { 90 }),
        ),
        egui::StrokeKind::Outside,
    );
    painter.text(
        field_rect.center(),
        egui::Align2::CENTER_CENTER,
        &value_text,
        FontId::new(
            fitted_font(&value_text, 11.0, ctx.scale, field_rect.width() - 6.0),
            FontFamily::Proportional,
        ),
        TEXT,
    );
}

fn draw_toggle_control(ui: &mut Ui, rect: Rect, ctx: &mut ControlDrawCtx<'_>, id: ControlId) {
    let active = ctx.params.snapshot.bool(id);
    let button = Rect::from_center_size(
        rect.center(),
        Vec2::new((rect.width() - 18.0 * ctx.scale).max(1.0), 32.0 * ctx.scale),
    );
    let response = toolbar_button(ui, button, id.spec().name, active, ctx.accent, ctx.scale);
    let selected = handle_control_selection(ctx.gui, id, &response);
    if response.clicked() && !selected {
        toggle_control(ui, ctx.gui, ctx.params, ctx.changes, id);
    }
}

fn draw_choice_control(
    ui: &mut Ui,
    rect: Rect,
    ctx: &mut ControlDrawCtx<'_>,
    id: ControlId,
    labels: &'static [&'static str],
) {
    let current = ctx
        .params
        .snapshot
        .choice(id)
        .min(labels.len().saturating_sub(1));
    let inner = rect.shrink2(Vec2::new(8.0 * ctx.scale, 8.0 * ctx.scale));
    let label_rect = Rect::from_min_size(inner.min, Vec2::new(inner.width(), 20.0 * ctx.scale));
    ui.painter_at(rect).text(
        label_rect.center(),
        egui::Align2::CENTER_CENTER,
        id.spec().name,
        FontId::new(
            fitted_font(id.spec().name, 11.0, ctx.scale, inner.width()),
            FontFamily::Proportional,
        ),
        TEXT_DIM,
    );

    let combo_rect = Rect::from_center_size(
        Pos2::new(inner.center().x, inner.center().y + 12.0 * ctx.scale),
        Vec2::new(inner.width(), 34.0 * ctx.scale),
    );
    let response = ui.allocate_rect(combo_rect, Sense::click());
    let selected = handle_control_selection(ctx.gui, id, &response);
    paint_dropdown_button(
        ui,
        combo_rect,
        labels[current],
        ctx.gui
            .choice_dropdown
            .is_some_and(|dropdown| dropdown.id == id),
        response.hovered(),
        ctx.accent,
        ctx.scale,
    );
    update_choice_anchor(ctx.gui, id, combo_rect, ctx.accent);

    if selected {
        gui_close_choice_if_matching(ctx.gui, id);
    } else if response.double_clicked() {
        push_undo(ctx.gui, ctx.params.snapshot);
        ctx.changes.set(id, id.spec().default);
    } else if response.clicked() {
        toggle_choice_dropdown(ctx.gui, id, combo_rect, ctx.accent);
    }
}

fn paint_knob(painter: &egui::Painter, rect: Rect, unit: f64, accent: Color32, scale: f32) {
    let center = rect.center();
    let radius = rect.width().min(rect.height()) * 0.5;
    painter.circle_filled(center, radius, Color32::from_rgb(5, 5, 15));
    painter.circle_stroke(center, radius, Stroke::new(1.0, rgba(accent, 150)));
    painter.circle_stroke(center, radius * 0.78, Stroke::new(1.0, rgba(PURPLE, 80)));

    let start = -PI_F32 * 0.78;
    let end = PI_F32 * 0.78;
    let angle = start + (end - start) * unit.clamp(0.0, 1.0) as f32;
    let arc_steps = 48;
    let mut prev = None;
    for step in 0..=arc_steps {
        let t = step as f32 / arc_steps as f32;
        let a = start + (angle - start) * t;
        let point = Pos2::new(
            center.x + a.cos() * radius * 0.88,
            center.y + a.sin() * radius * 0.88,
        );
        if let Some(previous) = prev {
            painter.line_segment([previous, point], Stroke::new(2.0 * scale, accent));
        }
        prev = Some(point);
    }
    let indicator = Pos2::new(
        center.x + angle.cos() * radius * 0.58,
        center.y + angle.sin() * radius * 0.58,
    );
    painter.line_segment([center, indicator], Stroke::new(2.0 * scale, TEXT));
    painter.circle_filled(center, radius * 0.08, accent);
}

fn paint_pan_knob(painter: &egui::Painter, rect: Rect, value: f64, accent: Color32, scale: f32) {
    let center = rect.center();
    let radius = rect.width().min(rect.height()) * 0.5;
    let pan = value.clamp(-1.0, 1.0) as f32;
    let center_angle = -PI_F32 * 0.5;
    let spread = PI_F32 * 0.78;
    let angle = center_angle + spread * pan;

    painter.circle_filled(center, radius, Color32::from_rgb(5, 5, 15));
    painter.circle_stroke(center, radius, Stroke::new(1.0, rgba(accent, 150)));
    painter.circle_stroke(center, radius * 0.78, Stroke::new(1.0, rgba(PURPLE, 80)));
    paint_knob_arc(
        painter,
        center,
        radius * 0.88,
        center_angle - spread,
        center_angle + spread,
        Stroke::new(1.0 * scale, rgba(TEXT_DIM, 74)),
    );

    if pan.abs() > 0.001 {
        let (start, end) = if pan < 0.0 {
            (angle, center_angle)
        } else {
            (center_angle, angle)
        };
        paint_knob_arc(
            painter,
            center,
            radius * 0.88,
            start,
            end,
            Stroke::new(2.2 * scale, accent),
        );
    }

    let indicator = Pos2::new(
        center.x + angle.cos() * radius * 0.58,
        center.y + angle.sin() * radius * 0.58,
    );
    painter.line_segment([center, indicator], Stroke::new(2.0 * scale, TEXT));
    let top = Pos2::new(
        center.x + center_angle.cos() * radius * 0.9,
        center.y + center_angle.sin() * radius * 0.9,
    );
    painter.circle_filled(top, 2.0 * scale, rgba(TEXT_DIM, 160));
    painter.circle_filled(center, radius * 0.08, accent);
}

fn paint_knob_arc(
    painter: &egui::Painter,
    center: Pos2,
    radius: f32,
    start: f32,
    end: f32,
    stroke: Stroke,
) {
    let arc_steps = 48;
    let mut previous = None;
    for step in 0..=arc_steps {
        let t = step as f32 / arc_steps as f32;
        let angle = start + (end - start) * t;
        let point = Pos2::new(
            center.x + angle.cos() * radius,
            center.y + angle.sin() * radius,
        );
        if let Some(prev) = previous {
            painter.line_segment([prev, point], stroke);
        }
        previous = Some(point);
    }
}

fn draw_num_popup(ctx: &Context, gui: &mut NebulaClusterGui, changes: &mut GuiChanges) {
    let Some(mut input) = gui.num_input.clone() else {
        return;
    };
    let mut open = true;
    egui::Window::new(input.id.spec().name)
        .collapsible(false)
        .resizable(false)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.add(egui::TextEdit::singleline(&mut input.value).desired_width(180.0));
            ui.horizontal(|ui| {
                if ui.button("Apply").clicked() {
                    if let Some(value) = parse_value(input.id, &input.value) {
                        changes.set(input.id, value);
                    }
                    gui.num_input = None;
                }
                if ui.button("Cancel").clicked() {
                    gui.num_input = None;
                }
            });
        });
    if open {
        gui.num_input = Some(input);
    } else {
        gui.num_input = None;
    }
}

fn draw_preset_popup(
    ctx: &Context,
    gui: &mut NebulaClusterGui,
    params: &GuiParams,
    changes: &mut GuiChanges,
) {
    if !gui.preset_dropdown_open {
        return;
    }

    let mut open = true;
    egui::Window::new("Presets")
        .collapsible(false)
        .resizable(false)
        .default_width(260.0)
        .open(&mut open)
        .show(ctx, |ui| {
            if gui.presets.is_empty() {
                ui.label("No saved presets");
            }
            for (name, snapshot) in gui.presets.clone() {
                if ui.button(name).clicked() {
                    push_undo(gui, params.snapshot);
                    changes.apply_snapshot(snapshot);
                    gui.preset_dropdown_open = false;
                }
            }
        });
    gui.preset_dropdown_open = open && gui.preset_dropdown_open;
}

fn draw_preset_name_field(ui: &mut Ui, rect: Rect, name: &mut String, scale: f32) {
    panel_inside(
        ui.painter_at(rect),
        rect,
        5.0 * scale,
        PANEL_2,
        rgba(CYAN, 75),
    );
    let edit_rect = Rect::from_center_size(
        rect.center(),
        Vec2::new((rect.width() - 14.0 * scale).max(1.0), 22.0 * scale),
    );
    ui.put(
        edit_rect,
        egui::TextEdit::singleline(name)
            .font(FontId::new(
                scaled_font(12.0, scale),
                FontFamily::Proportional,
            ))
            .frame(false),
    );
}

fn draw_choice_dropdown(
    ctx: &Context,
    gui: &mut NebulaClusterGui,
    params: &GuiParams,
    changes: &mut GuiChanges,
    scale: f32,
) {
    let Some(dropdown) = gui.choice_dropdown else {
        return;
    };
    let id = dropdown.id;
    let ValueKind::Choice(labels) = id.spec().kind else {
        gui.choice_dropdown = None;
        return;
    };
    if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
        gui.choice_dropdown = None;
        return;
    }

    let row_h = 28.0 * scale;
    let pad = 5.0 * scale;
    let width = dropdown.anchor.width().max(122.0 * scale);
    let height = pad * 2.0 + row_h * labels.len() as f32;
    let position = Pos2::new(dropdown.anchor.min.x, dropdown.anchor.max.y + 4.0 * scale);
    egui::Area::new(egui::Id::new(("choice_dropdown", id.index())))
        .order(egui::Order::Foreground)
        .fixed_pos(position)
        .show(ctx, |ui| {
            let rect = ui.allocate_space(Vec2::new(width, height)).1;
            panel_inside(
                ui.painter_at(rect),
                rect,
                6.0 * scale,
                rgba(PANEL, 248),
                rgba(dropdown.accent, 210),
            );

            let current = params
                .snapshot
                .choice(id)
                .min(labels.len().saturating_sub(1));
            for (index, label) in labels.iter().enumerate() {
                let row = Rect::from_min_size(
                    Pos2::new(rect.min.x + pad, rect.min.y + pad + row_h * index as f32),
                    Vec2::new(width - pad * 2.0, row_h),
                );
                let response = ui.allocate_rect(row, Sense::click());
                let selected = index == current;
                let fill = if selected {
                    rgba(dropdown.accent, 165)
                } else if response.hovered() {
                    PANEL_3
                } else {
                    Color32::TRANSPARENT
                };
                ui.painter_at(rect).rect_filled(row, 4.0 * scale, fill);
                ui.painter_at(rect).text(
                    row.center(),
                    egui::Align2::CENTER_CENTER,
                    *label,
                    FontId::new(
                        fitted_font(label, 12.0, scale, row.width() - 8.0 * scale),
                        FontFamily::Proportional,
                    ),
                    if selected { Color32::WHITE } else { TEXT },
                );
                if response.clicked() {
                    push_undo(gui, params.snapshot);
                    changes.set(id, index as f64);
                    gui.choice_dropdown = None;
                }
            }
        });
}

fn draw_midi_menu(ctx: &Context, gui: &mut NebulaClusterGui) {
    if !gui.midi_menu_open {
        return;
    }

    let mut open = true;
    egui::Window::new("MIDI")
        .collapsible(false)
        .resizable(true)
        .default_width(280.0)
        .open(&mut open)
        .show(ctx, |ui| {
            let mut midi_enabled = gui.midi_learn.midi_enabled.load(Ordering::Acquire);
            if ui.checkbox(&mut midi_enabled, "MIDI On/Off").changed() {
                gui.midi_learn
                    .midi_enabled
                    .store(midi_enabled, Ordering::Release);
            }
            ui.separator();
            if ui.button("Clean Up").clicked() {
                gui.cleanup_open = !gui.cleanup_open;
            }
            if gui.cleanup_open {
                let mappings = gui.midi_learn.mappings.lock().clone();
                for (cc, id_index) in mappings {
                    if let Some(id) = ControlId::from_index(id_index as usize) {
                        ui.horizontal(|ui| {
                            ui.label(format!("CC {cc}: {}", id.spec().name));
                            if ui.button("Delete").clicked() {
                                gui.midi_learn.mappings.lock().remove(&cc);
                                gui.midi_learn.sync_atomic_from_mutex();
                            }
                        });
                    }
                }
                if ui.button("Clear All").clicked() {
                    gui.midi_learn.mappings.lock().clear();
                    gui.midi_learn.sync_atomic_from_mutex();
                }
            }
            if ui.button("Roll Back").clicked() {
                let saved = gui.midi_learn.saved_mappings.lock().clone();
                *gui.midi_learn.mappings.lock() = saved;
                gui.midi_learn.sync_atomic_from_mutex();
            }
            if ui.button("Save").clicked() {
                gui.midi_learn.save_current_mapping();
            }
        });
    gui.midi_menu_open = open;
}

fn draw_spectrum_curve(
    painter: &egui::Painter,
    rect: Rect,
    magnitudes: &[f32],
    sample_rate: f64,
    scale: f32,
) {
    painter.rect_filled(rect, 5.0 * scale, Color32::from_rgb(3, 3, 12));
    painter.rect_stroke(
        rect,
        5.0 * scale,
        Stroke::new(1.0, rgba(CYAN, 70)),
        egui::StrokeKind::Inside,
    );

    let plot_rect = analyzer_plot_rect(rect, scale);
    let grid_painter = painter.with_clip_rect(plot_rect);
    for db in [-90.0_f32, -60.0, -30.0, 0.0] {
        let y = spectrum_y(plot_rect, db);
        grid_painter.line_segment(
            [Pos2::new(plot_rect.min.x, y), Pos2::new(plot_rect.max.x, y)],
            Stroke::new(1.0, rgba(TEXT_DIM, 45)),
        );
        let label_y = y.clamp(
            plot_rect.min.y + 9.0 * scale,
            plot_rect.max.y - 10.0 * scale,
        );
        painter.text(
            Pos2::new(plot_rect.min.x + 7.0 * scale, label_y),
            egui::Align2::LEFT_CENTER,
            format!("{db:.0} dB"),
            FontId::new(scaled_font(9.0, scale), FontFamily::Proportional),
            TEXT_DIM,
        );
    }

    for (freq, label) in [
        (20.0_f32, "20"),
        (100.0, "100"),
        (1000.0, "1k"),
        (10_000.0, "10k"),
        (20_000.0, "20k"),
    ] {
        let x = spectrum_x(plot_rect, freq);
        grid_painter.line_segment(
            [Pos2::new(x, plot_rect.min.y), Pos2::new(x, plot_rect.max.y)],
            Stroke::new(1.0, rgba(PURPLE, 45)),
        );
        let label_x = x.clamp(
            plot_rect.min.x + 10.0 * scale,
            plot_rect.max.x - 10.0 * scale,
        );
        let align = if freq <= 20.0 {
            egui::Align2::LEFT_BOTTOM
        } else if freq >= 20_000.0 {
            egui::Align2::RIGHT_BOTTOM
        } else {
            egui::Align2::CENTER_BOTTOM
        };
        painter.text(
            Pos2::new(label_x, rect.max.y - 5.0 * scale),
            align,
            label,
            FontId::new(scaled_font(9.0, scale), FontFamily::Proportional),
            TEXT_DIM,
        );
    }

    let bins = magnitudes.len().max(2);
    let nyquist_hz = ((sample_rate * 0.5) as f32).max(20.0);
    let curve_painter = painter.with_clip_rect(plot_rect);
    let mut previous = None;
    for (index, mag) in magnitudes.iter().enumerate().skip(1) {
        let freq = (index as f32 / (bins - 1) as f32) * nyquist_hz;
        if !(20.0..=20_000.0).contains(&freq) {
            continue;
        }
        let point = Pos2::new(spectrum_x(plot_rect, freq), spectrum_y(plot_rect, *mag));
        if let Some(prev) = previous {
            curve_painter.line_segment([prev, point], Stroke::new(1.4 * scale, CYAN));
        }
        previous = Some(point);
    }
}

fn draw_waveform_curve(painter: &egui::Painter, rect: Rect, waveform: &[f32], scale: f32) {
    painter.rect_filled(rect, 5.0 * scale, Color32::from_rgb(3, 3, 12));
    painter.rect_stroke(
        rect,
        5.0 * scale,
        Stroke::new(1.0, rgba(MAGENTA, 70)),
        egui::StrokeKind::Inside,
    );
    let plot_rect = analyzer_plot_rect(rect, scale);
    let center_y = plot_rect.center().y;
    let grid_painter = painter.with_clip_rect(plot_rect);
    for level in [-1.0_f32, -0.5, 0.0, 0.5, 1.0] {
        let y = center_y - level * plot_rect.height() * 0.44;
        grid_painter.line_segment(
            [Pos2::new(plot_rect.min.x, y), Pos2::new(plot_rect.max.x, y)],
            Stroke::new(1.0, rgba(TEXT_DIM, if level == 0.0 { 90 } else { 38 })),
        );
    }
    let count = waveform.len().max(2);
    let curve_painter = painter.with_clip_rect(plot_rect);
    let mut previous = None;
    for (index, sample) in waveform.iter().enumerate() {
        let x = plot_rect.min.x + plot_rect.width() * index as f32 / (count - 1) as f32;
        let y = center_y - sample.clamp(-1.0, 1.0) * plot_rect.height() * 0.46;
        let point = Pos2::new(x, y);
        if let Some(prev) = previous {
            curve_painter.line_segment([prev, point], Stroke::new(1.2 * scale, MAGENTA));
        }
        previous = Some(point);
    }
}

fn analyzer_plot_rect(rect: Rect, scale: f32) -> Rect {
    let inset_x = (10.0 * scale).min(rect.width() * 0.12);
    let inset_y = (10.0 * scale).min(rect.height() * 0.18);
    rect.shrink2(Vec2::new(inset_x, inset_y))
}

fn draw_analyzer_meters(painter: &egui::Painter, rect: Rect, params: &GuiParams, scale: f32) {
    painter.rect_filled(rect, 5.0 * scale, Color32::from_rgb(4, 4, 14));
    painter.rect_stroke(
        rect,
        5.0 * scale,
        Stroke::new(1.0, rgba(AMBER, 80)),
        egui::StrokeKind::Inside,
    );

    let inner = rect.shrink(7.0 * scale);
    let gap = 7.0 * scale;
    let meters = [
        AnalyzerMeter {
            label: "Output Peak",
            value: params.meters.peak_db,
            min: -60.0,
            max: 12.0,
            accent: CYAN,
        },
        AnalyzerMeter {
            label: "Gain Reduction",
            value: -params.meters.gain_reduction_db.abs(),
            min: -24.0,
            max: 0.0,
            accent: AMBER,
        },
    ];

    if inner.width() > 390.0 * scale && inner.height() < 62.0 * scale {
        let meter_w = (inner.width() - gap) * 0.5;
        for (index, meter) in meters.into_iter().enumerate() {
            let meter_rect = Rect::from_min_size(
                Pos2::new(inner.min.x + index as f32 * (meter_w + gap), inner.min.y),
                Vec2::new(meter_w, inner.height()),
            );
            draw_horizontal_meter(painter, meter_rect, meter, scale);
        }
    } else {
        let meter_h = (inner.height() - gap) * 0.5;
        for (index, meter) in meters.into_iter().enumerate() {
            let meter_rect = Rect::from_min_size(
                Pos2::new(inner.min.x, inner.min.y + index as f32 * (meter_h + gap)),
                Vec2::new(inner.width(), meter_h),
            );
            draw_horizontal_meter(painter, meter_rect, meter, scale);
        }
    }
}

struct AnalyzerMeter {
    label: &'static str,
    value: f32,
    min: f32,
    max: f32,
    accent: Color32,
}

fn draw_horizontal_meter(painter: &egui::Painter, rect: Rect, meter: AnalyzerMeter, scale: f32) {
    painter.rect_filled(rect, 4.0 * scale, PANEL_2);
    painter.rect_stroke(
        rect,
        4.0 * scale,
        Stroke::new(1.0, rgba(meter.accent, 95)),
        egui::StrokeKind::Inside,
    );

    let label_w = (104.0 * scale).min(rect.width() * 0.44);
    let bar = Rect::from_min_max(
        Pos2::new(rect.min.x + label_w, rect.min.y + 8.0 * scale),
        Pos2::new(rect.max.x - 8.0 * scale, rect.max.y - 8.0 * scale),
    );
    let norm = ((meter.value - meter.min) / (meter.max - meter.min)).clamp(0.0, 1.0);
    let fill = Rect::from_min_size(
        bar.min,
        Vec2::new((bar.width() * norm).max(0.0), bar.height().max(0.0)),
    );
    painter.rect_filled(bar, 3.0 * scale, Color32::from_rgb(4, 4, 14));
    painter.rect_filled(fill, 3.0 * scale, rgba(meter.accent, 190));
    painter.text(
        Pos2::new(rect.min.x + 8.0 * scale, rect.center().y),
        egui::Align2::LEFT_CENTER,
        meter.label,
        FontId::new(
            fitted_font(meter.label, 10.5, scale, label_w - 12.0 * scale),
            FontFamily::Proportional,
        ),
        TEXT_DIM,
    );
    painter.text(
        Pos2::new(bar.max.x - 5.0 * scale, bar.center().y),
        egui::Align2::RIGHT_CENTER,
        format_db(meter.value),
        FontId::new(
            fitted_font(
                &format_db(meter.value),
                10.5,
                scale,
                bar.width() - 8.0 * scale,
            ),
            FontFamily::Proportional,
        ),
        TEXT,
    );
}

fn spectrum_x(rect: Rect, freq: f32) -> f32 {
    let min = 20.0_f32.log10();
    let max = 20_000.0_f32.log10();
    let norm = ((freq.clamp(20.0, 20_000.0).log10() - min) / (max - min)).clamp(0.0, 1.0);
    rect.min.x + rect.width() * norm
}

fn spectrum_y(rect: Rect, db: f32) -> f32 {
    let norm = ((db.clamp(-120.0, 12.0) + 120.0) / 132.0).clamp(0.0, 1.0);
    rect.max.y - rect.height() * norm
}

fn section_panel(ui: &mut Ui, rect: Rect, title: &str, enabled: bool, scale: f32) {
    let border = if enabled {
        rgba(CYAN, 130)
    } else {
        rgba(TEXT_DIM, 70)
    };
    panel(
        ui.painter_at(rect),
        rect,
        7.0 * scale,
        rgba(PANEL, 238),
        border,
    );
    ui.painter_at(rect).text(
        Pos2::new(rect.min.x + 12.0 * scale, rect.min.y + 10.0 * scale),
        egui::Align2::LEFT_TOP,
        title,
        FontId::new(scaled_font(14.0, scale), FontFamily::Proportional),
        if enabled { TEXT } else { TEXT_DIM },
    );
}

fn panel(painter: egui::Painter, rect: Rect, radius: f32, fill: Color32, stroke: Color32) {
    painter.rect_filled(rect, radius, fill);
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, stroke),
        egui::StrokeKind::Outside,
    );
}

fn panel_inside(painter: egui::Painter, rect: Rect, radius: f32, fill: Color32, stroke: Color32) {
    painter.rect_filled(rect, radius, fill);
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, stroke),
        egui::StrokeKind::Inside,
    );
}

fn dropdown_button(
    ui: &mut Ui,
    rect: Rect,
    text: &str,
    active: bool,
    accent: Color32,
    scale: f32,
) -> Response {
    let response = ui.allocate_rect(rect, Sense::click());
    paint_dropdown_button(ui, rect, text, active, response.hovered(), accent, scale);
    response
}

fn paint_dropdown_button(
    ui: &mut Ui,
    rect: Rect,
    text: &str,
    active: bool,
    hovered: bool,
    accent: Color32,
    scale: f32,
) {
    let fill = if active {
        rgba(accent, 135)
    } else if hovered {
        PANEL_3
    } else {
        PANEL_2
    };
    let stroke = if active || hovered {
        rgba(accent, 220)
    } else {
        rgba(TEXT_DIM, 75)
    };
    panel_inside(ui.painter_at(rect), rect, 5.0 * scale, fill, stroke);

    let arrow_w = 17.0 * scale;
    let text_rect = Rect::from_min_max(
        Pos2::new(rect.min.x + 8.0 * scale, rect.min.y),
        Pos2::new(rect.max.x - arrow_w - 4.0 * scale, rect.max.y),
    );
    ui.painter_at(rect).text(
        text_rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        FontId::new(
            fitted_font(text, 12.0, scale, text_rect.width()),
            FontFamily::Proportional,
        ),
        if active { Color32::WHITE } else { TEXT },
    );

    let center = Pos2::new(rect.max.x - arrow_w * 0.65, rect.center().y + 1.0 * scale);
    let half = 4.0 * scale;
    ui.painter_at(rect).line_segment(
        [
            Pos2::new(center.x - half, center.y - half * 0.45),
            Pos2::new(center.x, center.y + half * 0.45),
        ],
        Stroke::new(1.3 * scale, accent),
    );
    ui.painter_at(rect).line_segment(
        [
            Pos2::new(center.x, center.y + half * 0.45),
            Pos2::new(center.x + half, center.y - half * 0.45),
        ],
        Stroke::new(1.3 * scale, accent),
    );
}

fn toolbar_button(
    ui: &mut Ui,
    rect: Rect,
    text: &str,
    active: bool,
    accent: Color32,
    scale: f32,
) -> Response {
    let response = ui.allocate_rect(rect, Sense::click());
    let fill = if active {
        rgba(accent, 180)
    } else if response.hovered() {
        PANEL_3
    } else {
        PANEL_2
    };
    let stroke = if active || response.hovered() {
        rgba(accent, 220)
    } else {
        rgba(TEXT_DIM, 70)
    };
    panel(ui.painter_at(rect), rect, 5.0 * scale, fill, stroke);
    ui.painter_at(rect).text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        FontId::new(
            fitted_font(text, 12.0, scale, rect.width() - 8.0 * scale),
            FontFamily::Proportional,
        ),
        if active { Color32::WHITE } else { TEXT },
    );
    response
}

fn toggle_choice_dropdown(
    gui: &mut NebulaClusterGui,
    id: ControlId,
    anchor: Rect,
    accent: Color32,
) {
    if gui
        .choice_dropdown
        .is_some_and(|dropdown| dropdown.id == id)
    {
        gui.choice_dropdown = None;
    } else {
        gui.choice_dropdown = Some(ChoiceDropdown { id, anchor, accent });
    }
}

fn gui_close_choice_if_matching(gui: &mut NebulaClusterGui, id: ControlId) {
    if gui
        .choice_dropdown
        .is_some_and(|dropdown| dropdown.id == id)
    {
        gui.choice_dropdown = None;
    }
}

fn update_choice_anchor(gui: &mut NebulaClusterGui, id: ControlId, anchor: Rect, accent: Color32) {
    if let Some(dropdown) = &mut gui.choice_dropdown {
        if dropdown.id == id {
            dropdown.anchor = anchor;
            dropdown.accent = accent;
        }
    }
}

fn section_toggle_rect(rect: Rect, scale: f32) -> Rect {
    Rect::from_min_size(
        Pos2::new(rect.max.x - 66.0 * scale, rect.min.y + 8.0 * scale),
        Vec2::new(54.0 * scale, 24.0 * scale),
    )
}

fn toggle_control(
    _ui: &mut Ui,
    gui: &mut NebulaClusterGui,
    params: &GuiParams,
    changes: &mut GuiChanges,
    id: ControlId,
) {
    push_undo(gui, params.snapshot);
    changes.set(id, if params.snapshot.bool(id) { 0.0 } else { 1.0 });
}

fn handle_control_selection(
    gui: &mut NebulaClusterGui,
    id: ControlId,
    response: &Response,
) -> bool {
    if !response.clicked() {
        return false;
    }

    let learning_target = gui.midi_learn.learning_target.load(Ordering::Acquire);
    if learning_target == MIDI_WAITING_FOR_CONTROL || learning_target >= 0 {
        gui.midi_learn
            .learning_target
            .store(id.index() as i32, Ordering::Release);
        response.ctx.request_repaint();
        true
    } else {
        false
    }
}

fn push_undo(gui: &mut NebulaClusterGui, snapshot: Snapshot) {
    if gui.undo_stack.last().copied() != Some(snapshot) {
        gui.undo_stack.push(snapshot);
        if gui.undo_stack.len() > 64 {
            gui.undo_stack.remove(0);
        }
        gui.redo_stack.clear();
    }
}

fn chaos_snapshot(gui: &mut NebulaClusterGui) -> Snapshot {
    let mut snapshot = Snapshot::default();
    for id in ALL_CONTROLS {
        let spec = id.spec();
        let value = match spec.kind {
            ValueKind::Boolean => {
                if rand_unit(&mut gui.chaos_seed) > 0.42 {
                    1.0
                } else {
                    0.0
                }
            }
            ValueKind::Choice(labels) => (rand_unit(&mut gui.chaos_seed) * labels.len() as f64)
                .floor()
                .min(labels.len().saturating_sub(1) as f64),
            ValueKind::Decibel if matches!(id, ControlId::InputLevel | ControlId::OutputLevel) => {
                -12.0 + rand_unit(&mut gui.chaos_seed) * 18.0
            }
            _ => spec.value_from_unit(rand_unit(&mut gui.chaos_seed)),
        };
        snapshot.set(id, value);
    }
    snapshot.set(ControlId::FxBypass, 0.0);
    snapshot
}

fn rand_unit(seed: &mut u64) -> f64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    ((*seed >> 11) as f64) * (1.0 / ((1_u64 << 53) as f64))
}

fn format_db(value: f32) -> String {
    if value <= -119.0 {
        String::from("-inf dB")
    } else {
        format!("{value:.1} dB")
    }
}

fn scaled_font(base: f32, scale: f32) -> f32 {
    (base * scale).clamp(base * 0.72, base * 1.35)
}

fn fitted_font(text: &str, base: f32, scale: f32, max_width: f32) -> f32 {
    let size = scaled_font(base, scale);
    let estimated_width = text.chars().count() as f32 * size * 0.56;
    if estimated_width <= max_width.max(1.0) {
        size
    } else {
        (size * max_width.max(1.0) / estimated_width).clamp(8.0, size)
    }
}

fn rgba(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_premultiplied(
        ((color.r() as u16 * alpha as u16) / 255) as u8,
        ((color.g() as u16 * alpha as u16) / 255) as u8,
        ((color.b() as u16 * alpha as u16) / 255) as u8,
        alpha,
    )
}

const PI_F32: f32 = std::f32::consts::PI;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_close_saves_current_midi_mapping_for_rollback() {
        let analyzer = Arc::new(Mutex::new(AnalyzerData::default()));
        let midi_learn = Arc::new(MidiLearnShared::new());
        let gui = NebulaClusterGui::new(analyzer, Arc::clone(&midi_learn));

        midi_learn.learn_cc(71, ControlId::FilterLpf);
        drop(gui);

        assert_eq!(
            midi_learn.saved_mappings.lock().get(&71).copied(),
            Some(ControlId::FilterLpf.index() as u8)
        );
    }
}
