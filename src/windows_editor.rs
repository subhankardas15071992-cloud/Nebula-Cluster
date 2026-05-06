use std::any::Any;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Once};

use nih_plug::prelude::{Editor, GuiContext, ParamSetter, ParentWindowHandle};
use parking_lot::Mutex;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{
    GetLastError, ERROR_CLASS_ALREADY_EXISTS, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM,
};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_UNKNOWN, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D_RECT_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Factory, ID2D1HwndRenderTarget, ID2D1SolidColorBrush,
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_ELLIPSE, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    D2D1_FEATURE_LEVEL_DEFAULT, D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_PRESENT_OPTIONS_NONE,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE,
    D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteFontCollection, IDWriteTextFormat,
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_DEMI_BOLD, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_MEASURING_MODE_NATURAL,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_TEXT_ALIGNMENT_TRAILING,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_UNKNOWN;
use windows::Win32::Graphics::Gdi::{
    BeginPaint, EndPaint, InvalidateRect, UpdateWindow, HBRUSH, PAINTSTRUCT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, SetFocus, VK_BACK, VK_ESCAPE, VK_RETURN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetWindowLongPtrW, KillTimer,
    LoadCursorW, RegisterClassW, SetTimer, SetWindowLongPtrW, ShowWindow, CREATESTRUCTW,
    CS_DBLCLKS, CS_HREDRAW, CS_VREDRAW, DLGC_WANTALLKEYS, DLGC_WANTCHARS, GWLP_USERDATA, HMENU,
    IDC_ARROW, SW_SHOW, WINDOW_EX_STYLE, WM_CANCELMODE, WM_CAPTURECHANGED, WM_CHAR, WM_ERASEBKGND,
    WM_GETDLGCODE, WM_KEYDOWN, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE,
    WM_NCCREATE, WM_NCDESTROY, WM_PAINT, WM_RBUTTONDOWN, WM_SIZE, WM_TIMER, WNDCLASSW, WS_CHILD,
    WS_CLIPCHILDREN, WS_CLIPSIBLINGS, WS_VISIBLE,
};
use windows_numerics::Vector2;

use super::analyzer::AnalyzerData;
use super::model::{format_value, parse_value, ControlId, Snapshot, ValueKind, ALL_CONTROLS};
use super::{
    apply_midi_cc_changes, set_control, snapshot_from_params, u32_to_f32, Meters, MidiLearnShared,
    NebulaClusterParams, MIDI_WAITING_FOR_CONTROL,
};

const BASE_W: f32 = 980.0;
const BASE_H: f32 = 640.0;
const TIMER_ID: usize = 9107;
const TIMER_MS: u32 = 50;
const MAX_UNDO: usize = 64;

const GLOBAL_CONTROLS: &[ControlId] = &[
    ControlId::InputLevel,
    ControlId::InputPan,
    ControlId::OutputLevel,
    ControlId::OutputPan,
    ControlId::GlobalMix,
    ControlId::Oversampling,
    ControlId::GlobalPhase,
];
const DISTORTION_CONTROLS: &[ControlId] = &[
    ControlId::DistortionEnabled,
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
const FILTER_CONTROLS: &[ControlId] = &[
    ControlId::FilterEnabled,
    ControlId::FilterHpf,
    ControlId::FilterHpSlope,
    ControlId::FilterHpRes,
    ControlId::FilterLpf,
    ControlId::FilterLpSlope,
    ControlId::FilterLpRes,
];
const COMPRESSOR_CONTROLS: &[ControlId] = &[
    ControlId::CompressorEnabled,
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

pub(super) fn create_editor(
    params: Arc<NebulaClusterParams>,
    analyzer: Arc<Mutex<AnalyzerData>>,
    meters: Arc<Meters>,
    midi_learn: Arc<MidiLearnShared>,
) -> Option<Box<dyn Editor>> {
    Some(Box::new(NativeEditor {
        params,
        analyzer,
        meters,
        midi_learn,
        scale_bits: AtomicU32::new(1.0_f32.to_bits()),
    }))
}

struct NativeEditor {
    params: Arc<NebulaClusterParams>,
    analyzer: Arc<Mutex<AnalyzerData>>,
    meters: Arc<Meters>,
    midi_learn: Arc<MidiLearnShared>,
    scale_bits: AtomicU32,
}

impl Editor for NativeEditor {
    fn spawn(
        &self,
        parent: ParentWindowHandle,
        context: Arc<dyn GuiContext>,
    ) -> Box<dyn Any + Send> {
        let ParentWindowHandle::Win32Hwnd(parent_hwnd) = parent else {
            return Box::new(());
        };
        if parent_hwnd.is_null() || !register_window_class() {
            return Box::new(());
        }

        let scale = f32::from_bits(self.scale_bits.load(Ordering::Acquire)).clamp(0.5, 3.0);
        let parent_hwnd = HWND(parent_hwnd);
        let scaled_width = (BASE_W * scale).round() as i32;
        let scaled_height = (BASE_H * scale).round() as i32;
        let (width, height) = client_size(parent_hwnd)
            .filter(|(width, height)| *width > 100 && *height > 100)
            .map(|(width, height)| (width as i32, height as i32))
            .unwrap_or((scaled_width, scaled_height));

        let state = Box::new(NativeWindowState::new(
            self.params.clone(),
            self.analyzer.clone(),
            self.meters.clone(),
            self.midi_learn.clone(),
            context,
            scale,
        ));
        let state_ptr = Box::into_raw(state);

        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class_name(),
                w!("Nebula Cluster"),
                WS_CHILD | WS_VISIBLE | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
                0,
                0,
                width,
                height,
                Some(parent_hwnd),
                Option::<HMENU>::None,
                module_instance(),
                Some(state_ptr.cast::<c_void>()),
            )
        };

        match hwnd {
            Ok(hwnd) => unsafe {
                let _ = ShowWindow(hwnd, SW_SHOW);
                let _ = UpdateWindow(hwnd);
                Box::new(NativeWindowHandle {
                    hwnd: hwnd.0 as isize,
                })
            },
            Err(_) => unsafe {
                drop(Box::from_raw(state_ptr));
                Box::new(())
            },
        }
    }

    fn size(&self) -> (u32, u32) {
        (BASE_W as u32, BASE_H as u32)
    }

    fn set_scale_factor(&self, factor: f32) -> bool {
        self.scale_bits
            .store(factor.max(0.5).to_bits(), Ordering::Release);
        true
    }

    fn param_value_changed(&self, _id: &str, _normalized_value: f32) {}

    fn param_modulation_changed(&self, _id: &str, _modulation_offset: f32) {}

    fn param_values_changed(&self) {}
}

struct NativeWindowHandle {
    hwnd: isize,
}

unsafe impl Send for NativeWindowHandle {}

impl Drop for NativeWindowHandle {
    fn drop(&mut self) {
        if self.hwnd != 0 {
            let hwnd = HWND(self.hwnd as *mut c_void);
            let _ = unsafe { DestroyWindow(hwnd) };
            self.hwnd = 0;
        }
    }
}

struct NativeWindowState {
    hwnd: HWND,
    params: Arc<NebulaClusterParams>,
    analyzer: Arc<Mutex<AnalyzerData>>,
    meters: Arc<Meters>,
    midi_learn: Arc<MidiLearnShared>,
    context: Arc<dyn GuiContext>,
    d2d_factory: Option<ID2D1Factory>,
    dwrite_factory: Option<IDWriteFactory>,
    render_target: Option<ID2D1HwndRenderTarget>,
    text_formats: Option<TextFormats>,
    active_tab: Tab,
    drag: Option<DragState>,
    drag_snapshot: Option<Snapshot>,
    presets: Vec<(String, Snapshot)>,
    preset_name_counter: usize,
    selected_preset: Option<usize>,
    preset_menu_open: bool,
    preset_name_input: Option<String>,
    numeric_input: Option<NumericInput>,
    choice_dropdown: Option<ChoiceDropdown>,
    midi_context_menu_open: bool,
    midi_cleanup_menu_open: bool,
    undo_stack: Vec<Snapshot>,
    redo_stack: Vec<Snapshot>,
    state_a: Snapshot,
    state_b: Snapshot,
    active_state_is_a: bool,
    chaos_seed: u64,
    scale: f32,
}

impl NativeWindowState {
    fn new(
        params: Arc<NebulaClusterParams>,
        analyzer: Arc<Mutex<AnalyzerData>>,
        meters: Arc<Meters>,
        midi_learn: Arc<MidiLearnShared>,
        context: Arc<dyn GuiContext>,
        scale: f32,
    ) -> Self {
        Self {
            hwnd: HWND::default(),
            params,
            analyzer,
            meters,
            midi_learn,
            context,
            d2d_factory: None,
            dwrite_factory: None,
            render_target: None,
            text_formats: None,
            active_tab: Tab::Global,
            drag: None,
            drag_snapshot: None,
            presets: Vec::new(),
            preset_name_counter: 1,
            selected_preset: None,
            preset_menu_open: false,
            preset_name_input: None,
            numeric_input: None,
            choice_dropdown: None,
            midi_context_menu_open: false,
            midi_cleanup_menu_open: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            state_a: Snapshot::default(),
            state_b: Snapshot::default(),
            active_state_is_a: true,
            chaos_seed: 0x4e43_6c75_7374_6572,
            scale,
        }
    }

    fn paint(&mut self) {
        self.apply_midi_changes();

        let Some(size) = client_size(self.hwnd) else {
            return;
        };
        let layout = Layout::new(size.0 as f32, size.1 as f32, self.scale);
        let Some(rt) = self.ensure_render_target() else {
            return;
        };
        let Some(formats) = self.ensure_text_formats(layout.s) else {
            return;
        };
        let Some(brushes) = Brushes::new(&rt) else {
            return;
        };
        let snapshot = snapshot_from_params(&self.params);

        unsafe {
            rt.BeginDraw();
            rt.Clear(Some(&Colors::BLACK));
        }

        self.draw_background(&rt, &brushes, &layout);
        self.draw_header(&rt, &brushes, &formats, &layout, snapshot);
        self.draw_toolbar(&rt, &brushes, &formats, &layout, snapshot);
        self.draw_analyzer(&rt, &brushes, &formats, &layout);
        self.draw_meters(&rt, &brushes, &formats, &layout);
        self.draw_tabs(&rt, &brushes, &formats, &layout);
        self.draw_controls(&rt, &brushes, &formats, &layout, snapshot);
        self.draw_choice_dropdown(&rt, &brushes, &formats, snapshot);
        self.draw_preset_menu(&rt, &brushes, &formats, &layout);
        self.draw_midi_context_menu(&rt, &brushes, &formats, &layout);
        self.draw_midi_cleanup_menu(&rt, &brushes, &formats, &layout);
        self.draw_preset_name_popup(&rt, &brushes, &formats, &layout);
        self.draw_numeric_popup(&rt, &brushes, &formats, &layout);

        if unsafe { rt.EndDraw(None, None) }.is_err() {
            self.render_target = None;
        }
    }

    fn apply_midi_changes(&self) {
        self.midi_learn.sync_mutex_from_atomic_if_needed();
        let setter = ParamSetter::new(self.context.as_ref());
        apply_midi_cc_changes(&self.midi_learn, &self.params, &setter);
    }

    fn draw_background(&self, rt: &ID2D1HwndRenderTarget, brushes: &Brushes, layout: &Layout) {
        fill_rect(rt, layout.full, &brushes.black);
        fill_rect(
            rt,
            UiRect::new(0.0, 0.0, layout.full.w, layout.full.h * 0.36),
            &brushes.top,
        );
        for index in 0..9 {
            let y = layout.header.bottom() + index as f32 * 42.0 * layout.s;
            draw_line(rt, 0.0, y, layout.full.right(), y, &brushes.grid_soft, 0.6);
        }
    }

    fn draw_header(
        &self,
        rt: &ID2D1HwndRenderTarget,
        brushes: &Brushes,
        formats: &TextFormats,
        layout: &Layout,
        snapshot: Snapshot,
    ) {
        fill_rect(rt, layout.header, &brushes.panel);
        draw_line(
            rt,
            layout.header.x,
            layout.header.bottom(),
            layout.header.right(),
            layout.header.bottom(),
            &brushes.cyan_dim,
            1.0,
        );

        let s = layout.s;
        let logo = UiRect::new(18.0 * s, 15.0 * s, 34.0 * s, 34.0 * s);
        fill_round(rt, logo, 6.0 * s, &brushes.cyan);
        draw_text(
            rt,
            "NC",
            logo,
            &formats.body_bold,
            &brushes.black,
            Align::Center,
        );
        draw_text(
            rt,
            "Nebula Cluster",
            UiRect::new(62.0 * s, 10.0 * s, 260.0 * s, 28.0 * s),
            &formats.title,
            &brushes.text_primary,
            Align::Leading,
        );
        draw_text(
            rt,
            "Native Direct2D editor",
            UiRect::new(64.0 * s, 36.0 * s, 260.0 * s, 18.0 * s),
            &formats.small,
            &brushes.text_dim,
            Align::Leading,
        );

        let status = if snapshot.bool(ControlId::FxBypass) {
            "FX bypassed"
        } else {
            "Processor active"
        };
        let status_rect = UiRect::new(
            layout.header.right() - 188.0 * s,
            18.0 * s,
            168.0 * s,
            28.0 * s,
        );
        fill_round(
            rt,
            status_rect,
            5.0 * s,
            if snapshot.bool(ControlId::FxBypass) {
                &brushes.red_soft
            } else {
                &brushes.card
            },
        );
        stroke_round(
            rt,
            status_rect,
            5.0 * s,
            if snapshot.bool(ControlId::FxBypass) {
                &brushes.red
            } else {
                &brushes.border
            },
            1.0,
        );
        draw_text(
            rt,
            status,
            status_rect,
            &formats.small,
            if snapshot.bool(ControlId::FxBypass) {
                &brushes.red
            } else {
                &brushes.text_secondary
            },
            Align::Center,
        );
    }

    fn draw_toolbar(
        &self,
        rt: &ID2D1HwndRenderTarget,
        brushes: &Brushes,
        formats: &TextFormats,
        layout: &Layout,
        snapshot: Snapshot,
    ) {
        fill_rect(rt, layout.toolbar, &brushes.panel_dark);
        draw_line(
            rt,
            layout.toolbar.x,
            layout.toolbar.bottom(),
            layout.toolbar.right(),
            layout.toolbar.bottom(),
            &brushes.border,
            1.0,
        );

        self.draw_button(
            rt,
            brushes,
            formats,
            layout.save_button,
            "Save",
            false,
            Accent::Cyan,
        );

        let preset_label = self
            .selected_preset
            .and_then(|index| self.presets.get(index))
            .map(|(name, _)| name.as_str())
            .unwrap_or("Presets");
        self.draw_button(
            rt,
            brushes,
            formats,
            layout.preset_button,
            preset_label,
            self.preset_menu_open,
            Accent::Cyan,
        );

        self.draw_button(
            rt,
            brushes,
            formats,
            layout.delete_button,
            "Delete",
            self.selected_preset.is_some(),
            Accent::Red,
        );

        self.draw_button(
            rt,
            brushes,
            formats,
            layout.undo_button,
            "Undo",
            !self.undo_stack.is_empty(),
            Accent::Purple,
        );

        self.draw_button(
            rt,
            brushes,
            formats,
            layout.redo_button,
            "Redo",
            !self.redo_stack.is_empty(),
            Accent::Purple,
        );

        self.draw_button(
            rt,
            brushes,
            formats,
            layout.ab_button,
            if self.active_state_is_a {
                "A/B A"
            } else {
                "A/B B"
            },
            true,
            Accent::Magenta,
        );

        self.draw_button(
            rt,
            brushes,
            formats,
            layout.chaos_button,
            "Chaos",
            false,
            Accent::Amber,
        );

        let bypass = snapshot.bool(ControlId::FxBypass);
        self.draw_button(
            rt,
            brushes,
            formats,
            layout.bypass_button,
            if bypass { "FX Bypassed" } else { "FX Bypass" },
            bypass,
            Accent::Red,
        );

        let learning = self.midi_learn.learning_target.load(Ordering::Relaxed);
        let midi_label = if learning == MIDI_WAITING_FOR_CONTROL {
            "MIDI: select"
        } else if learning >= 0 {
            "MIDI: move CC"
        } else {
            "MIDI Learn"
        };
        self.draw_button(
            rt,
            brushes,
            formats,
            layout.midi_button,
            midi_label,
            learning >= 0 || learning == MIDI_WAITING_FOR_CONTROL,
            Accent::Cyan,
        );

        let os = format_value(
            ControlId::Oversampling,
            snapshot.get(ControlId::Oversampling),
        );
        draw_text(
            rt,
            &format!("Oversampling {os}"),
            layout.toolbar_status,
            &formats.small,
            &brushes.text_dim,
            Align::Leading,
        );
    }

    fn draw_preset_menu(
        &self,
        rt: &ID2D1HwndRenderTarget,
        brushes: &Brushes,
        formats: &TextFormats,
        layout: &Layout,
    ) {
        if !self.preset_menu_open {
            return;
        }

        let s = layout.s;
        let Some((menu, row_h)) = preset_menu_rect(layout, self.presets.len()) else {
            return;
        };
        fill_round(rt, menu, 7.0 * s, &brushes.panel_dark);
        stroke_round(rt, menu, 7.0 * s, &brushes.cyan_dim, 1.0);

        if self.presets.is_empty() {
            draw_text(
                rt,
                "No saved presets",
                menu.shrink(6.0 * s),
                &formats.small,
                &brushes.text_dim,
                Align::Center,
            );
            return;
        }

        for (index, (name, _)) in self.presets.iter().take(8).enumerate() {
            let row = UiRect::new(
                menu.x + 5.0 * s,
                menu.y + 5.0 * s + row_h * index as f32,
                menu.w - 10.0 * s,
                row_h,
            );
            let selected = self.selected_preset == Some(index);
            fill_round(
                rt,
                row,
                4.0 * s,
                if selected {
                    &brushes.cyan_soft
                } else {
                    &brushes.card
                },
            );
            draw_text(
                rt,
                name,
                UiRect::new(row.x + 8.0 * s, row.y, row.w - 16.0 * s, row.h),
                &formats.small,
                if selected {
                    &brushes.cyan
                } else {
                    &brushes.text_secondary
                },
                Align::Leading,
            );
        }
    }

    fn draw_choice_dropdown(
        &self,
        rt: &ID2D1HwndRenderTarget,
        brushes: &Brushes,
        formats: &TextFormats,
        snapshot: Snapshot,
    ) {
        let Some(dropdown) = self.choice_dropdown else {
            return;
        };
        let ValueKind::Choice(labels) = dropdown.id.spec().kind else {
            return;
        };
        let menu = choice_dropdown_rect(dropdown.rect, labels.len());
        let s = dropdown.rect.scale_hint();
        fill_round(rt, menu, 5.0 * s, &brushes.panel_dark);
        stroke_round(
            rt,
            menu,
            5.0 * s,
            self.active_tab.accent().brush(brushes),
            1.0,
        );

        let row_h = dropdown.rect.h.max(20.0);
        let selected = snapshot.choice(dropdown.id);
        for (index, label) in labels.iter().enumerate() {
            let row = UiRect::new(menu.x, menu.y + row_h * index as f32, menu.w, row_h);
            if selected == index {
                fill_round(rt, row.shrink(2.0 * s), 4.0 * s, &brushes.cyan_soft);
            }
            draw_text(
                rt,
                label,
                row,
                &formats.small,
                if selected == index {
                    self.active_tab.accent().brush(brushes)
                } else {
                    &brushes.text_secondary
                },
                Align::Center,
            );
        }
    }

    fn draw_preset_name_popup(
        &self,
        rt: &ID2D1HwndRenderTarget,
        brushes: &Brushes,
        formats: &TextFormats,
        layout: &Layout,
    ) {
        let Some(name) = &self.preset_name_input else {
            return;
        };
        let popup = preset_name_popup_layout(layout);
        draw_modal_panel(rt, brushes, popup.rect, layout.s);
        draw_text(
            rt,
            "Save Preset",
            popup.title,
            &formats.body_bold,
            &brushes.text_primary,
            Align::Leading,
        );
        fill_round(rt, popup.input, 4.0 * layout.s, &brushes.black);
        stroke_round(rt, popup.input, 4.0 * layout.s, &brushes.cyan_dim, 1.0);
        draw_text(
            rt,
            name,
            popup.input.shrink(8.0 * layout.s),
            &formats.small,
            &brushes.text_primary,
            Align::Leading,
        );
        self.draw_button(rt, brushes, formats, popup.ok, "Save", true, Accent::Cyan);
        self.draw_button(
            rt,
            brushes,
            formats,
            popup.cancel,
            "Cancel",
            false,
            Accent::Red,
        );
    }

    fn draw_numeric_popup(
        &self,
        rt: &ID2D1HwndRenderTarget,
        brushes: &Brushes,
        formats: &TextFormats,
        layout: &Layout,
    ) {
        let Some(input) = &self.numeric_input else {
            return;
        };
        let popup = numeric_popup_layout(layout);
        draw_modal_panel(rt, brushes, popup.rect, layout.s);
        draw_text(
            rt,
            input.id.spec().name,
            popup.title,
            &formats.body_bold,
            &brushes.text_primary,
            Align::Leading,
        );
        fill_round(rt, popup.input, 4.0 * layout.s, &brushes.black);
        stroke_round(rt, popup.input, 4.0 * layout.s, &brushes.cyan_dim, 1.0);
        draw_text(
            rt,
            &input.value,
            popup.input.shrink(8.0 * layout.s),
            &formats.small,
            &brushes.text_primary,
            Align::Leading,
        );
        self.draw_button(rt, brushes, formats, popup.ok, "Apply", true, Accent::Cyan);
        self.draw_button(
            rt,
            brushes,
            formats,
            popup.cancel,
            "Cancel",
            false,
            Accent::Red,
        );
    }

    fn draw_midi_context_menu(
        &self,
        rt: &ID2D1HwndRenderTarget,
        brushes: &Brushes,
        formats: &TextFormats,
        layout: &Layout,
    ) {
        if !self.midi_context_menu_open {
            return;
        }
        let menu = midi_context_rect(layout);
        fill_round(rt, menu, 6.0 * layout.s, &brushes.panel_dark);
        stroke_round(rt, menu, 6.0 * layout.s, &brushes.green, 1.0);
        let enabled = self.midi_learn.midi_enabled.load(Ordering::Relaxed);
        let labels = [
            if enabled { "MIDI Off" } else { "MIDI On" },
            "Clean Up >",
            "Roll Back",
            "Save",
        ];
        for (index, label) in labels.iter().enumerate() {
            let row = midi_context_row(layout, index);
            let active = index == 1 && self.midi_cleanup_menu_open;
            if active {
                fill_round(
                    rt,
                    row.shrink(2.0 * layout.s),
                    4.0 * layout.s,
                    &brushes.green_soft,
                );
            }
            draw_text(
                rt,
                label,
                UiRect::new(
                    row.x + 9.0 * layout.s,
                    row.y,
                    row.w - 18.0 * layout.s,
                    row.h,
                ),
                &formats.small,
                if active {
                    &brushes.green
                } else {
                    &brushes.text_secondary
                },
                Align::Leading,
            );
        }
    }

    fn draw_midi_cleanup_menu(
        &self,
        rt: &ID2D1HwndRenderTarget,
        brushes: &Brushes,
        formats: &TextFormats,
        layout: &Layout,
    ) {
        if !self.midi_context_menu_open || !self.midi_cleanup_menu_open {
            return;
        }

        self.midi_learn.sync_mutex_from_atomic_if_needed();
        let mappings = self.midi_learn.mappings.lock().clone();
        let mut sorted: Vec<(u8, u8)> = mappings.iter().map(|(&cc, &param)| (cc, param)).collect();
        sorted.sort_by_key(|(cc, _)| *cc);

        let menu = midi_cleanup_rect(layout, sorted.len());
        fill_round(rt, menu, 6.0 * layout.s, &brushes.panel_dark);
        stroke_round(rt, menu, 6.0 * layout.s, &brushes.green, 1.0);

        if sorted.is_empty() {
            draw_text(
                rt,
                "No MIDI mappings",
                menu.shrink(7.0 * layout.s),
                &formats.small,
                &brushes.text_dim,
                Align::Center,
            );
            return;
        }

        for (index, (cc, param_index)) in sorted.iter().take(8).enumerate() {
            let row = midi_cleanup_row(layout, index);
            let name = ControlId::from_index(*param_index as usize)
                .map(|id| id.spec().name)
                .unwrap_or("Unknown");
            draw_text(
                rt,
                &format!("CC {cc}: {name}"),
                UiRect::new(
                    row.x + 8.0 * layout.s,
                    row.y,
                    row.w - 16.0 * layout.s,
                    row.h,
                ),
                &formats.tiny,
                &brushes.text_secondary,
                Align::Leading,
            );
        }

        let clear = midi_cleanup_row(layout, sorted.len().min(8));
        fill_round(
            rt,
            clear.shrink(2.0 * layout.s),
            4.0 * layout.s,
            &brushes.red_soft,
        );
        draw_text(
            rt,
            "Clear All",
            clear,
            &formats.small,
            &brushes.red,
            Align::Center,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_button(
        &self,
        rt: &ID2D1HwndRenderTarget,
        brushes: &Brushes,
        formats: &TextFormats,
        rect: UiRect,
        label: &str,
        active: bool,
        accent: Accent,
    ) {
        fill_round(
            rt,
            rect,
            5.0 * rect.scale_hint(),
            if active {
                accent.soft_brush(brushes)
            } else {
                &brushes.control
            },
        );
        stroke_round(
            rt,
            rect,
            5.0 * rect.scale_hint(),
            if active {
                accent.brush(brushes)
            } else {
                &brushes.border
            },
            1.0,
        );
        draw_text(
            rt,
            label,
            rect,
            &formats.small,
            if active {
                accent.brush(brushes)
            } else {
                &brushes.text_secondary
            },
            Align::Center,
        );
    }

    fn draw_analyzer(
        &self,
        rt: &ID2D1HwndRenderTarget,
        brushes: &Brushes,
        formats: &TextFormats,
        layout: &Layout,
    ) {
        card(rt, layout.analyzer, 8.0 * layout.s, brushes);
        let inner = layout.analyzer.shrink(12.0 * layout.s);
        draw_text(
            rt,
            "Post FX Analyzer",
            UiRect::new(inner.x, inner.y, inner.w, 18.0 * layout.s),
            &formats.body_bold,
            &brushes.text_secondary,
            Align::Leading,
        );
        let graph = UiRect::new(
            inner.x,
            inner.y + 24.0 * layout.s,
            inner.w,
            inner.h - 30.0 * layout.s,
        );
        fill_round(rt, graph, 5.0 * layout.s, &brushes.black);
        stroke_round(rt, graph, 5.0 * layout.s, &brushes.border, 1.0);

        for db in [-80.0_f32, -60.0, -40.0, -20.0, 0.0] {
            let y = graph.y + graph.h * (1.0 - ((db + 90.0) / 114.0));
            draw_line(rt, graph.x, y, graph.right(), y, &brushes.grid, 0.7);
            draw_text(
                rt,
                &format!("{}", db as i32),
                UiRect::new(
                    graph.x + 4.0 * layout.s,
                    y - 12.0 * layout.s,
                    34.0 * layout.s,
                    12.0 * layout.s,
                ),
                &formats.tiny,
                &brushes.text_muted,
                Align::Leading,
            );
        }
        for freq in [
            50.0_f32, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0, 20000.0,
        ] {
            let x = graph.x + freq_to_x(freq, graph.w);
            draw_line(rt, x, graph.y, x, graph.bottom(), &brushes.grid, 0.7);
        }

        let data = self
            .analyzer
            .try_lock()
            .map(|data| data.clone())
            .unwrap_or_default();
        let nyquist = (data.sample_rate as f32 * 0.5).max(1.0);
        let mags = &data.magnitudes_db;
        let mut prev = None;
        for (index, db) in mags.iter().enumerate().skip(1) {
            let freq = nyquist * index as f32 / mags.len().saturating_sub(1).max(1) as f32;
            let x = graph.x + freq_to_x(freq, graph.w);
            let norm = ((*db + 90.0) / 114.0).clamp(0.0, 1.0);
            let y = graph.y + graph.h * (1.0 - norm);
            if let Some((px, py)) = prev {
                draw_line(rt, px, py, x, y, &brushes.cyan, 1.4 * layout.s);
            }
            prev = Some((x, y));
        }

        let wave_y = graph.y + graph.h * 0.78;
        let wave_h = graph.h * 0.16;
        let mut prev = None;
        for (index, sample) in data.waveform.iter().enumerate() {
            let x = graph.x
                + graph.w * index as f32 / data.waveform.len().saturating_sub(1).max(1) as f32;
            let y = wave_y - sample.clamp(-1.0, 1.0) * wave_h;
            if let Some((px, py)) = prev {
                draw_line(rt, px, py, x, y, &brushes.magenta, 0.9 * layout.s);
            }
            prev = Some((x, y));
        }
    }

    fn draw_meters(
        &self,
        rt: &ID2D1HwndRenderTarget,
        brushes: &Brushes,
        formats: &TextFormats,
        layout: &Layout,
    ) {
        let peak = u32_to_f32(self.meters.peak_bits.load(Ordering::Relaxed));
        let gain_reduction = u32_to_f32(self.meters.reduction_bits.load(Ordering::Relaxed));
        self.draw_meter(
            rt,
            brushes,
            formats,
            layout.peak_meter,
            "Output Peak",
            peak,
            ((peak + 60.0) / 72.0).clamp(0.0, 1.0),
            Accent::Green,
        );
        self.draw_meter(
            rt,
            brushes,
            formats,
            layout.reduction_meter,
            "Gain Reduction",
            -gain_reduction.abs(),
            ((-gain_reduction.abs() + 24.0) / 24.0).clamp(0.0, 1.0),
            Accent::Amber,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_meter(
        &self,
        rt: &ID2D1HwndRenderTarget,
        brushes: &Brushes,
        formats: &TextFormats,
        rect: UiRect,
        label: &str,
        value_db: f32,
        norm: f32,
        accent: Accent,
    ) {
        let s = rect.scale_hint();
        card(rt, rect, 8.0 * s, brushes);
        let inner = rect.shrink(7.0 * s);
        let label_w = (98.0 * s).min(inner.w * 0.46).max(70.0 * s);
        let label_rect = UiRect::new(inner.x, inner.y, label_w, inner.h);
        let bar = UiRect::new(
            inner.x + label_w,
            inner.y + 8.0 * s,
            (inner.w - label_w).max(24.0 * s),
            (inner.h - 16.0 * s).max(10.0 * s),
        );
        let fill = UiRect::new(bar.x, bar.y, bar.w * norm.clamp(0.0, 1.0), bar.h);

        draw_text(
            rt,
            label,
            label_rect,
            &formats.tiny,
            &brushes.text_dim,
            Align::Leading,
        );
        fill_round(rt, bar, 4.0 * s, &brushes.black);
        fill_round(rt, fill, 4.0 * s, accent.soft_brush(brushes));
        stroke_round(rt, bar, 4.0 * s, accent.brush(brushes), 1.0);
        let value_w = (74.0 * s).min((bar.w - 6.0 * s).max(1.0));
        let value_rect = UiRect::new(
            bar.right() - value_w - 3.0 * s,
            bar.y + 3.0 * s,
            value_w,
            (bar.h - 6.0 * s).max(8.0 * s),
        );
        fill_round(rt, value_rect, 3.0 * s, &brushes.panel_dark);
        draw_text(
            rt,
            &format_db(value_db),
            value_rect.shrink(4.0 * s),
            &formats.tiny,
            &brushes.text_primary,
            Align::Trailing,
        );
    }

    fn draw_tabs(
        &self,
        rt: &ID2D1HwndRenderTarget,
        brushes: &Brushes,
        formats: &TextFormats,
        layout: &Layout,
    ) {
        let tabs = tab_rects(layout);
        for (tab, rect) in tabs {
            let active = tab == self.active_tab;
            fill_round(
                rt,
                rect,
                6.0 * layout.s,
                if active {
                    tab.accent().soft_brush(brushes)
                } else {
                    &brushes.card
                },
            );
            stroke_round(
                rt,
                rect,
                6.0 * layout.s,
                if active {
                    tab.accent().brush(brushes)
                } else {
                    &brushes.border
                },
                1.0,
            );
            draw_text(
                rt,
                tab.label(),
                rect,
                &formats.small,
                if active {
                    tab.accent().brush(brushes)
                } else {
                    &brushes.text_secondary
                },
                Align::Center,
            );
        }
    }

    fn draw_controls(
        &self,
        rt: &ID2D1HwndRenderTarget,
        brushes: &Brushes,
        formats: &TextFormats,
        layout: &Layout,
        snapshot: Snapshot,
    ) {
        card(rt, layout.controls, 8.0 * layout.s, brushes);
        let title = format!("{} Controls", self.active_tab.label());
        draw_text(
            rt,
            &title,
            UiRect::new(
                layout.controls.x + 14.0 * layout.s,
                layout.controls.y + 10.0 * layout.s,
                260.0 * layout.s,
                20.0 * layout.s,
            ),
            &formats.body_bold,
            self.active_tab.accent().brush(brushes),
            Align::Leading,
        );

        let learning = self.midi_learn.learning_target.load(Ordering::Relaxed);
        if learning == MIDI_WAITING_FOR_CONTROL {
            draw_text(
                rt,
                "Click a control to assign the next incoming MIDI CC",
                UiRect::new(
                    layout.controls.right() - 360.0 * layout.s,
                    layout.controls.y + 11.0 * layout.s,
                    340.0 * layout.s,
                    18.0 * layout.s,
                ),
                &formats.small,
                &brushes.cyan,
                Align::Trailing,
            );
        }

        for cell in control_cells(layout, self.active_tab) {
            self.draw_control_cell(rt, brushes, formats, cell, snapshot);
        }
    }

    fn draw_control_cell(
        &self,
        rt: &ID2D1HwndRenderTarget,
        brushes: &Brushes,
        formats: &TextFormats,
        cell: ControlCell,
        snapshot: Snapshot,
    ) {
        let id = cell.id;
        let spec = id.spec();
        let value = snapshot.get(id);
        let s = cell.rect.scale_hint();
        fill_round(rt, cell.rect, 6.0 * s, &brushes.row);
        stroke_round(rt, cell.rect, 6.0 * s, &brushes.border, 1.0);
        draw_text(
            rt,
            spec.name,
            UiRect::new(
                cell.rect.x + 7.0 * s,
                cell.rect.y + 4.0 * s,
                cell.rect.w - 14.0 * s,
                17.0 * s,
            ),
            &formats.small,
            &brushes.text_secondary,
            Align::Center,
        );

        match spec.kind {
            ValueKind::Boolean => {
                let active = value >= 0.5;
                let pill = UiRect::new(
                    cell.knob_rect.x + cell.knob_rect.w * 0.5 - 26.0 * s,
                    cell.knob_rect.center_y() - 11.0 * s,
                    52.0 * s,
                    22.0 * s,
                );
                fill_round(
                    rt,
                    pill,
                    11.0 * s,
                    if active {
                        self.active_tab.accent().soft_brush(brushes)
                    } else {
                        &brushes.control
                    },
                );
                stroke_round(
                    rt,
                    pill,
                    11.0 * s,
                    if active {
                        self.active_tab.accent().brush(brushes)
                    } else {
                        &brushes.border
                    },
                    1.0,
                );
                let knob_x = if active {
                    pill.right() - 11.0 * s
                } else {
                    pill.x + 11.0 * s
                };
                fill_circle(rt, knob_x, pill.center_y(), 7.0 * s, &brushes.text_primary);
            }
            ValueKind::Choice(labels) => {
                fill_round(rt, cell.segment_rect, 4.0 * s, &brushes.control);
                let segment_w = cell.segment_rect.w / labels.len().max(1) as f32;
                let selected = value
                    .round()
                    .clamp(0.0, labels.len().saturating_sub(1) as f64)
                    as usize;
                for (index, label) in labels.iter().enumerate() {
                    let segment = UiRect::new(
                        cell.segment_rect.x + segment_w * index as f32,
                        cell.segment_rect.y,
                        segment_w,
                        cell.segment_rect.h,
                    );
                    if index == selected {
                        fill_round(
                            rt,
                            segment.shrink(2.0 * s),
                            4.0 * s,
                            self.active_tab.accent().brush(brushes),
                        );
                    }
                    draw_text(
                        rt,
                        label,
                        segment,
                        &formats.tiny,
                        if index == selected {
                            &brushes.black
                        } else {
                            &brushes.text_secondary
                        },
                        Align::Center,
                    );
                }
                stroke_round(rt, cell.segment_rect, 4.0 * s, &brushes.border, 1.0);
            }
            _ => {
                draw_knob(
                    rt,
                    cell.knob_rect,
                    spec.unit_from_value(value) as f32,
                    self.active_tab.accent(),
                    brushes,
                    s,
                );
            }
        }

        fill_round(rt, cell.value_rect, 4.0 * s, &brushes.black);
        stroke_round(rt, cell.value_rect, 4.0 * s, &brushes.border, 1.0);
        draw_text(
            rt,
            &format_value(id, value),
            cell.value_rect,
            &formats.tiny,
            self.active_tab.accent().brush(brushes),
            Align::Center,
        );
    }

    fn mouse_down(&mut self, x: f32, y: f32) {
        let _ = unsafe { SetFocus(Some(self.hwnd)) };
        let Some(size) = client_size(self.hwnd) else {
            return;
        };
        let layout = Layout::new(size.0 as f32, size.1 as f32, self.scale);

        if self.handle_modal_click(x, y, &layout) {
            invalidate(self.hwnd);
            return;
        }
        if self.handle_choice_dropdown_click(x, y) {
            invalidate(self.hwnd);
            return;
        }
        if self.handle_midi_menu_click(x, y, &layout) {
            invalidate(self.hwnd);
            return;
        }
        if self.handle_preset_menu_click(x, y, &layout) {
            invalidate(self.hwnd);
            return;
        }
        if layout.save_button.contains(x, y) {
            self.open_preset_save();
            invalidate(self.hwnd);
            return;
        }
        if layout.preset_button.contains(x, y) {
            self.preset_menu_open = !self.preset_menu_open;
            invalidate(self.hwnd);
            return;
        }
        if layout.delete_button.contains(x, y) {
            self.delete_selected_preset();
            invalidate(self.hwnd);
            return;
        }
        if layout.undo_button.contains(x, y) {
            self.undo();
            invalidate(self.hwnd);
            return;
        }
        if layout.redo_button.contains(x, y) {
            self.redo();
            invalidate(self.hwnd);
            return;
        }
        if layout.ab_button.contains(x, y) {
            self.toggle_ab();
            invalidate(self.hwnd);
            return;
        }
        if layout.chaos_button.contains(x, y) {
            self.apply_chaos();
            invalidate(self.hwnd);
            return;
        }
        if layout.bypass_button.contains(x, y) {
            self.push_undo_snapshot(snapshot_from_params(&self.params));
            self.toggle_control(ControlId::FxBypass);
            invalidate(self.hwnd);
            return;
        }
        if layout.midi_button.contains(x, y) {
            self.toggle_midi_learn();
            invalidate(self.hwnd);
            return;
        }

        for (tab, rect) in tab_rects(&layout) {
            if rect.contains(x, y) {
                self.active_tab = tab;
                invalidate(self.hwnd);
                return;
            }
        }

        for cell in control_cells(&layout, self.active_tab) {
            if !cell.rect.contains(x, y) {
                continue;
            }

            if self.midi_learn.learning_target.load(Ordering::Relaxed) == MIDI_WAITING_FOR_CONTROL {
                self.midi_learn
                    .learning_target
                    .store(cell.id.index() as i32, Ordering::Release);
                invalidate(self.hwnd);
                return;
            }

            let before = snapshot_from_params(&self.params);
            if cell.value_rect.contains(x, y) {
                self.open_numeric_input(cell.id, before);
                invalidate(self.hwnd);
                return;
            }

            match cell.id.spec().kind {
                ValueKind::Boolean => {
                    self.push_undo_snapshot(before);
                    self.toggle_control(cell.id);
                }
                ValueKind::Choice(labels) => {
                    let _ = labels;
                    self.choice_dropdown = Some(ChoiceDropdown {
                        id: cell.id,
                        rect: cell.segment_rect,
                    });
                }
                _ => {
                    self.drag_snapshot = Some(before);
                    self.drag = Some(DragState {
                        id: cell.id,
                        start_x: x,
                        start_y: y,
                        start_unit: cell.id.spec().unit_from_value(before.get(cell.id)),
                    });
                    unsafe {
                        let _ = SetCapture(self.hwnd);
                    }
                }
            }
            invalidate(self.hwnd);
            return;
        }
    }

    fn mouse_move(&mut self, x: f32, y: f32) {
        if let Some(drag) = self.drag {
            self.set_from_drag(drag, x, y);
            invalidate(self.hwnd);
        }
    }

    fn mouse_up(&mut self, x: f32, y: f32) {
        if let Some(drag) = self.drag.take() {
            self.set_from_drag(drag, x, y);
            let _ = unsafe { ReleaseCapture() };
            if let Some(before) = self.drag_snapshot.take() {
                self.record_undo_if_changed(before);
            }
            invalidate(self.hwnd);
        }
    }

    fn mouse_right_down(&mut self, x: f32, y: f32) {
        let _ = unsafe { SetFocus(Some(self.hwnd)) };
        let Some(size) = client_size(self.hwnd) else {
            return;
        };
        let layout = Layout::new(size.0 as f32, size.1 as f32, self.scale);
        if layout.midi_button.contains(x, y) {
            self.midi_context_menu_open = !self.midi_context_menu_open;
            self.midi_cleanup_menu_open = false;
            self.preset_menu_open = false;
            self.choice_dropdown = None;
            invalidate(self.hwnd);
        }
    }

    fn mouse_double_click(&mut self, x: f32, y: f32) {
        let Some(size) = client_size(self.hwnd) else {
            return;
        };
        let layout = Layout::new(size.0 as f32, size.1 as f32, self.scale);
        for cell in control_cells(&layout, self.active_tab) {
            if cell.rect.contains(x, y) {
                let before = snapshot_from_params(&self.params);
                self.push_undo_snapshot(before);
                self.set_control_value(cell.id, cell.id.spec().default);
                invalidate(self.hwnd);
                return;
            }
        }
    }

    fn toggle_midi_learn(&self) {
        let current = self.midi_learn.learning_target.load(Ordering::Relaxed);
        let next = if current == MIDI_WAITING_FOR_CONTROL || current >= 0 {
            -1
        } else {
            MIDI_WAITING_FOR_CONTROL
        };
        self.midi_learn
            .learning_target
            .store(next, Ordering::Release);
    }

    fn toggle_control(&self, id: ControlId) {
        let current = snapshot_from_params(&self.params).get(id);
        self.set_control_value(id, if current >= 0.5 { 0.0 } else { 1.0 });
    }

    fn set_from_drag(&self, drag: DragState, x: f32, y: f32) {
        let delta = (drag.start_y - y) + (x - drag.start_x) * 0.28;
        let sensitivity = match drag.id.spec().kind {
            ValueKind::Hertz => 0.0035,
            ValueKind::Milliseconds => 0.0035,
            ValueKind::Decibel => 0.0045,
            _ => 0.0055,
        };
        let unit = (drag.start_unit + delta as f64 * sensitivity).clamp(0.0, 1.0);
        self.set_control_value(drag.id, drag.id.spec().value_from_unit(unit));
    }

    fn set_control_value(&self, id: ControlId, value: f64) {
        let setter = ParamSetter::new(self.context.as_ref());
        set_control(&self.params, &setter, id, value);
    }

    fn apply_snapshot(&self, snapshot: Snapshot) {
        for id in ALL_CONTROLS {
            self.set_control_value(id, snapshot.get(id));
        }
    }

    fn push_undo_snapshot(&mut self, snapshot: Snapshot) {
        if self.undo_stack.last().copied() != Some(snapshot) {
            self.undo_stack.push(snapshot);
            if self.undo_stack.len() > MAX_UNDO {
                self.undo_stack.remove(0);
            }
            self.redo_stack.clear();
        }
    }

    fn record_undo_if_changed(&mut self, before: Snapshot) {
        if snapshot_from_params(&self.params) != before {
            self.push_undo_snapshot(before);
        }
    }

    fn undo(&mut self) {
        if let Some(snapshot) = self.undo_stack.pop() {
            self.redo_stack.push(snapshot_from_params(&self.params));
            self.redo_stack.truncate(MAX_UNDO);
            self.apply_snapshot(snapshot);
        }
    }

    fn redo(&mut self) {
        if let Some(snapshot) = self.redo_stack.pop() {
            self.undo_stack.push(snapshot_from_params(&self.params));
            self.undo_stack.truncate(MAX_UNDO);
            self.apply_snapshot(snapshot);
        }
    }

    fn open_preset_save(&mut self) {
        self.preset_name_input = Some(format!("Preset {}", self.preset_name_counter));
        self.preset_menu_open = false;
        self.choice_dropdown = None;
        let _ = unsafe { SetFocus(Some(self.hwnd)) };
    }

    fn save_preset(&mut self, name: String) {
        let name = default_preset_name(name.trim(), &self.presets);
        self.preset_name_counter += 1;
        self.presets
            .push((name, snapshot_from_params(&self.params)));
        self.selected_preset = Some(self.presets.len() - 1);
        self.preset_menu_open = false;
        self.preset_name_input = None;
    }

    fn open_numeric_input(&mut self, id: ControlId, snapshot: Snapshot) {
        self.numeric_input = Some(NumericInput {
            id,
            value: format_value(id, snapshot.get(id)),
        });
        self.choice_dropdown = None;
        let _ = unsafe { SetFocus(Some(self.hwnd)) };
    }

    fn confirm_numeric_input(&mut self) {
        let Some(input) = self.numeric_input.take() else {
            return;
        };
        if let Some(value) = parse_value(input.id, &input.value) {
            let before = snapshot_from_params(&self.params);
            self.push_undo_snapshot(before);
            self.set_control_value(input.id, value);
        }
    }

    fn delete_selected_preset(&mut self) {
        let Some(index) = self.selected_preset else {
            return;
        };
        if index < self.presets.len() {
            self.presets.remove(index);
        }
        self.selected_preset = if self.presets.is_empty() {
            None
        } else {
            Some(index.min(self.presets.len() - 1))
        };
        self.preset_menu_open = false;
    }

    fn handle_preset_menu_click(&mut self, x: f32, y: f32, layout: &Layout) -> bool {
        if !self.preset_menu_open {
            return false;
        }

        let Some((menu, row_h)) = preset_menu_rect(layout, self.presets.len()) else {
            self.preset_menu_open = false;
            return true;
        };

        if !menu.contains(x, y) {
            self.preset_menu_open = false;
            return false;
        }

        if self.presets.is_empty() {
            return true;
        }

        let row = ((y - menu.y - 5.0 * layout.s) / row_h).floor() as usize;
        if let Some((_, snapshot)) = self.presets.get(row).cloned() {
            let before = snapshot_from_params(&self.params);
            self.push_undo_snapshot(before);
            self.apply_snapshot(snapshot);
            self.selected_preset = Some(row);
            self.preset_menu_open = false;
        }
        true
    }

    fn handle_modal_click(&mut self, x: f32, y: f32, layout: &Layout) -> bool {
        if self.preset_name_input.is_some() {
            let popup = preset_name_popup_layout(layout);
            if popup.ok.contains(x, y) {
                if let Some(name) = self.preset_name_input.take() {
                    self.save_preset(name);
                }
                return true;
            }
            if popup.cancel.contains(x, y) || !popup.rect.contains(x, y) {
                self.preset_name_input = None;
                return true;
            }
            return true;
        }

        if self.numeric_input.is_some() {
            let popup = numeric_popup_layout(layout);
            if popup.ok.contains(x, y) {
                self.confirm_numeric_input();
                return true;
            }
            if popup.cancel.contains(x, y) || !popup.rect.contains(x, y) {
                self.numeric_input = None;
                return true;
            }
            return true;
        }

        false
    }

    fn handle_choice_dropdown_click(&mut self, x: f32, y: f32) -> bool {
        let Some(dropdown) = self.choice_dropdown else {
            return false;
        };
        let ValueKind::Choice(labels) = dropdown.id.spec().kind else {
            self.choice_dropdown = None;
            return false;
        };
        let menu = choice_dropdown_rect(dropdown.rect, labels.len());
        if !menu.contains(x, y) {
            self.choice_dropdown = None;
            return false;
        }

        let row_h = dropdown.rect.h.max(20.0);
        let index = ((y - menu.y) / row_h)
            .floor()
            .clamp(0.0, labels.len().saturating_sub(1) as f32) as usize;
        let before = snapshot_from_params(&self.params);
        self.push_undo_snapshot(before);
        self.set_control_value(dropdown.id, index as f64);
        self.choice_dropdown = None;
        true
    }

    fn handle_midi_menu_click(&mut self, x: f32, y: f32, layout: &Layout) -> bool {
        if self.midi_cleanup_menu_open {
            self.midi_learn.sync_mutex_from_atomic_if_needed();
            let mappings = self.midi_learn.mappings.lock().clone();
            let mut sorted: Vec<(u8, u8)> =
                mappings.iter().map(|(&cc, &param)| (cc, param)).collect();
            sorted.sort_by_key(|(cc, _)| *cc);
            if let Some(action) = midi_cleanup_hit(layout, x, y, &sorted) {
                match action {
                    CleanupAction::Remove(cc) => {
                        self.midi_learn.mappings.lock().remove(&cc);
                        self.midi_learn.sync_atomic_from_mutex();
                    }
                    CleanupAction::ClearAll => {
                        self.midi_learn.mappings.lock().clear();
                        self.midi_learn.sync_atomic_from_mutex();
                        self.midi_learn.learning_target.store(-1, Ordering::Release);
                    }
                }
                return true;
            }
        }

        if !self.midi_context_menu_open {
            return false;
        }

        let Some(action) = midi_context_hit(layout, x, y) else {
            self.midi_context_menu_open = false;
            self.midi_cleanup_menu_open = false;
            return false;
        };

        match action {
            MidiMenuAction::ToggleMidi => {
                let enabled = self.midi_learn.midi_enabled.load(Ordering::Relaxed);
                self.midi_learn
                    .midi_enabled
                    .store(!enabled, Ordering::Release);
                self.midi_context_menu_open = false;
                self.midi_cleanup_menu_open = false;
            }
            MidiMenuAction::CleanUp => {
                self.midi_cleanup_menu_open = !self.midi_cleanup_menu_open;
            }
            MidiMenuAction::RollBack => {
                let saved = self.midi_learn.saved_mappings.lock().clone();
                *self.midi_learn.mappings.lock() = saved;
                self.midi_learn.sync_atomic_from_mutex();
                self.midi_learn.learning_target.store(-1, Ordering::Release);
                self.midi_context_menu_open = false;
                self.midi_cleanup_menu_open = false;
            }
            MidiMenuAction::Save => {
                self.midi_learn.save_current_mapping();
                self.midi_context_menu_open = false;
                self.midi_cleanup_menu_open = false;
            }
        }
        true
    }

    fn toggle_ab(&mut self) {
        let before = snapshot_from_params(&self.params);
        if self.active_state_is_a {
            self.state_a = before;
            self.active_state_is_a = false;
            self.push_undo_snapshot(before);
            self.apply_snapshot(self.state_b);
        } else {
            self.state_b = before;
            self.active_state_is_a = true;
            self.push_undo_snapshot(before);
            self.apply_snapshot(self.state_a);
        }
    }

    fn apply_chaos(&mut self) {
        let before = snapshot_from_params(&self.params);
        self.push_undo_snapshot(before);
        let chaos = chaos_snapshot(&mut self.chaos_seed);
        self.apply_snapshot(chaos);
    }

    fn clear_drag(&mut self) {
        self.drag = None;
        self.drag_snapshot = None;
    }

    fn char_input(&mut self, ch: char) {
        if ch.is_control() {
            return;
        }
        if let Some(name) = &mut self.preset_name_input {
            if name.len() < 48 {
                name.push(ch);
            }
        } else if let Some(input) = &mut self.numeric_input {
            if input.value.len() < 48 {
                input.value.push(ch);
            }
        }
    }

    fn key_down(&mut self, key: u32) {
        if key == VK_ESCAPE.0 as u32 {
            self.preset_name_input = None;
            self.numeric_input = None;
            self.choice_dropdown = None;
            self.midi_context_menu_open = false;
            self.midi_cleanup_menu_open = false;
            return;
        }

        if key == VK_RETURN.0 as u32 {
            if let Some(name) = self.preset_name_input.take() {
                self.save_preset(name);
            } else if self.numeric_input.is_some() {
                self.confirm_numeric_input();
            }
            return;
        }

        if key == VK_BACK.0 as u32 {
            if let Some(name) = &mut self.preset_name_input {
                name.pop();
            } else if let Some(input) = &mut self.numeric_input {
                input.value.pop();
            }
        }
    }

    fn ensure_render_target(&mut self) -> Option<ID2D1HwndRenderTarget> {
        if self.render_target.is_none() {
            if self.d2d_factory.is_none() {
                self.d2d_factory = unsafe {
                    D2D1CreateFactory::<ID2D1Factory>(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).ok()
                };
            }
            let factory = self.d2d_factory.as_ref()?;
            let (width, height) = client_size(self.hwnd)?;
            let rt_props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_UNKNOWN,
                    alphaMode: D2D1_ALPHA_MODE_UNKNOWN,
                },
                dpiX: 0.0,
                dpiY: 0.0,
                usage: D2D1_RENDER_TARGET_USAGE_NONE,
                minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
            };
            let hwnd_props = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                hwnd: self.hwnd,
                pixelSize: D2D_SIZE_U {
                    width: width.max(1),
                    height: height.max(1),
                },
                presentOptions: D2D1_PRESENT_OPTIONS_NONE,
            };
            self.render_target =
                unsafe { factory.CreateHwndRenderTarget(&rt_props, &hwnd_props).ok() };
        }
        self.render_target.clone()
    }

    fn ensure_text_formats(&mut self, scale: f32) -> Option<TextFormats> {
        if self.text_formats.is_none() {
            if self.dwrite_factory.is_none() {
                self.dwrite_factory = unsafe {
                    DWriteCreateFactory::<IDWriteFactory>(DWRITE_FACTORY_TYPE_SHARED).ok()
                };
            }
            let factory = self.dwrite_factory.as_ref()?;
            self.text_formats = Some(TextFormats::new(factory, scale)?);
        }
        self.text_formats.clone()
    }
}

#[derive(Clone, Copy, Debug)]
struct DragState {
    id: ControlId,
    start_x: f32,
    start_y: f32,
    start_unit: f64,
}

#[derive(Clone, Debug)]
struct NumericInput {
    id: ControlId,
    value: String,
}

#[derive(Clone, Copy, Debug)]
struct ChoiceDropdown {
    id: ControlId,
    rect: UiRect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Global,
    Distortion,
    Filter,
    Compressor,
}

impl Tab {
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

    fn controls(self) -> &'static [ControlId] {
        match self {
            Self::Global => GLOBAL_CONTROLS,
            Self::Distortion => DISTORTION_CONTROLS,
            Self::Filter => FILTER_CONTROLS,
            Self::Compressor => COMPRESSOR_CONTROLS,
        }
    }

    fn accent(self) -> Accent {
        match self {
            Self::Global => Accent::Cyan,
            Self::Distortion => Accent::Magenta,
            Self::Filter => Accent::Purple,
            Self::Compressor => Accent::Green,
        }
    }
}

#[derive(Clone, Copy)]
enum Accent {
    Cyan,
    Magenta,
    Purple,
    Green,
    Amber,
    Red,
}

impl Accent {
    fn brush(self, brushes: &Brushes) -> &ID2D1SolidColorBrush {
        match self {
            Self::Cyan => &brushes.cyan,
            Self::Magenta => &brushes.magenta,
            Self::Purple => &brushes.purple,
            Self::Green => &brushes.green,
            Self::Amber => &brushes.amber,
            Self::Red => &brushes.red,
        }
    }

    fn soft_brush(self, brushes: &Brushes) -> &ID2D1SolidColorBrush {
        match self {
            Self::Cyan => &brushes.cyan_soft,
            Self::Magenta => &brushes.magenta_soft,
            Self::Purple => &brushes.purple_soft,
            Self::Green => &brushes.green_soft,
            Self::Amber => &brushes.amber_soft,
            Self::Red => &brushes.red_soft,
        }
    }
}

#[derive(Clone)]
struct TextFormats {
    tiny: IDWriteTextFormat,
    small: IDWriteTextFormat,
    body_bold: IDWriteTextFormat,
    title: IDWriteTextFormat,
}

impl TextFormats {
    fn new(factory: &IDWriteFactory, scale: f32) -> Option<Self> {
        Some(Self {
            tiny: create_text_format(factory, 10.0 * scale, false)?,
            small: create_text_format(factory, 12.0 * scale, false)?,
            body_bold: create_text_format(factory, 14.0 * scale, true)?,
            title: create_text_format(factory, 22.0 * scale, true)?,
        })
    }
}

fn create_text_format(
    factory: &IDWriteFactory,
    size: f32,
    bold: bool,
) -> Option<IDWriteTextFormat> {
    let format = unsafe {
        factory
            .CreateTextFormat(
                w!("Segoe UI"),
                Option::<&IDWriteFontCollection>::None,
                if bold {
                    DWRITE_FONT_WEIGHT_DEMI_BOLD
                } else {
                    DWRITE_FONT_WEIGHT_NORMAL
                },
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                size,
                w!("en-us"),
            )
            .ok()?
    };
    let _ = unsafe { format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING) };
    let _ = unsafe { format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER) };
    Some(format)
}

struct Brushes {
    black: ID2D1SolidColorBrush,
    top: ID2D1SolidColorBrush,
    panel: ID2D1SolidColorBrush,
    panel_dark: ID2D1SolidColorBrush,
    card: ID2D1SolidColorBrush,
    row: ID2D1SolidColorBrush,
    control: ID2D1SolidColorBrush,
    border: ID2D1SolidColorBrush,
    grid: ID2D1SolidColorBrush,
    grid_soft: ID2D1SolidColorBrush,
    cyan: ID2D1SolidColorBrush,
    cyan_dim: ID2D1SolidColorBrush,
    cyan_soft: ID2D1SolidColorBrush,
    magenta: ID2D1SolidColorBrush,
    magenta_soft: ID2D1SolidColorBrush,
    purple: ID2D1SolidColorBrush,
    purple_soft: ID2D1SolidColorBrush,
    green: ID2D1SolidColorBrush,
    green_soft: ID2D1SolidColorBrush,
    amber: ID2D1SolidColorBrush,
    amber_soft: ID2D1SolidColorBrush,
    red: ID2D1SolidColorBrush,
    red_soft: ID2D1SolidColorBrush,
    text_primary: ID2D1SolidColorBrush,
    text_secondary: ID2D1SolidColorBrush,
    text_dim: ID2D1SolidColorBrush,
    text_muted: ID2D1SolidColorBrush,
}

impl Brushes {
    fn new(rt: &ID2D1HwndRenderTarget) -> Option<Self> {
        Some(Self {
            black: solid(rt, Colors::BLACK)?,
            top: solid(rt, Colors::TOP)?,
            panel: solid(rt, Colors::PANEL)?,
            panel_dark: solid(rt, Colors::PANEL_DARK)?,
            card: solid(rt, Colors::CARD)?,
            row: solid(rt, Colors::ROW)?,
            control: solid(rt, Colors::CONTROL)?,
            border: solid(rt, Colors::BORDER)?,
            grid: solid(rt, Colors::GRID)?,
            grid_soft: solid(rt, Colors::GRID_SOFT)?,
            cyan: solid(rt, Colors::CYAN)?,
            cyan_dim: solid(rt, Colors::CYAN_DIM)?,
            cyan_soft: solid(rt, Colors::CYAN_SOFT)?,
            magenta: solid(rt, Colors::MAGENTA)?,
            magenta_soft: solid(rt, Colors::MAGENTA_SOFT)?,
            purple: solid(rt, Colors::PURPLE)?,
            purple_soft: solid(rt, Colors::PURPLE_SOFT)?,
            green: solid(rt, Colors::GREEN)?,
            green_soft: solid(rt, Colors::GREEN_SOFT)?,
            amber: solid(rt, Colors::AMBER)?,
            amber_soft: solid(rt, Colors::AMBER_SOFT)?,
            red: solid(rt, Colors::RED)?,
            red_soft: solid(rt, Colors::RED_SOFT)?,
            text_primary: solid(rt, Colors::TEXT_PRIMARY)?,
            text_secondary: solid(rt, Colors::TEXT_SECONDARY)?,
            text_dim: solid(rt, Colors::TEXT_DIM)?,
            text_muted: solid(rt, Colors::TEXT_MUTED)?,
        })
    }
}

struct Colors;

impl Colors {
    const BLACK: D2D1_COLOR_F = color(2, 2, 8, 255);
    const TOP: D2D1_COLOR_F = color(8, 5, 24, 255);
    const PANEL: D2D1_COLOR_F = color(8, 7, 20, 255);
    const PANEL_DARK: D2D1_COLOR_F = color(5, 5, 15, 255);
    const CARD: D2D1_COLOR_F = color(12, 10, 30, 255);
    const ROW: D2D1_COLOR_F = color(14, 12, 34, 255);
    const CONTROL: D2D1_COLOR_F = color(20, 18, 44, 255);
    const BORDER: D2D1_COLOR_F = color(46, 38, 82, 255);
    const GRID: D2D1_COLOR_F = color(45, 54, 80, 115);
    const GRID_SOFT: D2D1_COLOR_F = color(35, 40, 62, 70);
    const CYAN: D2D1_COLOR_F = color(0, 220, 255, 255);
    const CYAN_DIM: D2D1_COLOR_F = color(0, 130, 170, 180);
    const CYAN_SOFT: D2D1_COLOR_F = color(0, 220, 255, 42);
    const MAGENTA: D2D1_COLOR_F = color(255, 38, 196, 255);
    const MAGENTA_SOFT: D2D1_COLOR_F = color(255, 38, 196, 42);
    const PURPLE: D2D1_COLOR_F = color(150, 72, 255, 255);
    const PURPLE_SOFT: D2D1_COLOR_F = color(150, 72, 255, 46);
    const GREEN: D2D1_COLOR_F = color(0, 235, 132, 255);
    const GREEN_SOFT: D2D1_COLOR_F = color(0, 235, 132, 42);
    const AMBER: D2D1_COLOR_F = color(255, 184, 58, 255);
    const AMBER_SOFT: D2D1_COLOR_F = color(255, 184, 58, 48);
    const RED: D2D1_COLOR_F = color(255, 64, 82, 255);
    const RED_SOFT: D2D1_COLOR_F = color(255, 64, 82, 46);
    const TEXT_PRIMARY: D2D1_COLOR_F = color(218, 235, 255, 255);
    const TEXT_SECONDARY: D2D1_COLOR_F = color(145, 178, 220, 255);
    const TEXT_DIM: D2D1_COLOR_F = color(96, 126, 166, 255);
    const TEXT_MUTED: D2D1_COLOR_F = color(75, 95, 130, 255);
}

const fn color(r: u8, g: u8, b: u8, a: u8) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: a as f32 / 255.0,
    }
}

#[derive(Clone, Copy)]
struct Layout {
    full: UiRect,
    header: UiRect,
    toolbar: UiRect,
    analyzer: UiRect,
    peak_meter: UiRect,
    reduction_meter: UiRect,
    tabs: UiRect,
    controls: UiRect,
    save_button: UiRect,
    preset_button: UiRect,
    delete_button: UiRect,
    undo_button: UiRect,
    redo_button: UiRect,
    ab_button: UiRect,
    chaos_button: UiRect,
    bypass_button: UiRect,
    midi_button: UiRect,
    toolbar_status: UiRect,
    s: f32,
}

impl Layout {
    fn new(w: f32, h: f32, _scale_hint: f32) -> Self {
        let s = (w / BASE_W).min(h / BASE_H).clamp(0.50, 1.4);
        let full = UiRect::new(0.0, 0.0, w, h);
        let header = UiRect::new(0.0, 0.0, w, 58.0 * s);
        let toolbar = UiRect::new(0.0, header.bottom(), w, 42.0 * s);
        let margin = 12.0 * s;
        let gap = 9.0 * s;
        let meter_w = (164.0 * s).min(w * 0.22).max(120.0 * s);
        let analyzer_y = toolbar.bottom() + margin;
        let content_available = (h - analyzer_y - margin).max(1.0);
        let analyzer_h = (content_available * 0.34)
            .clamp(118.0 * s, 185.0 * s)
            .min(content_available * 0.48);
        let analyzer_w = (w - margin * 2.0 - meter_w - gap).max(260.0 * s);
        let analyzer = UiRect::new(margin, analyzer_y, analyzer_w, analyzer_h);
        let peak_meter = UiRect::new(
            analyzer.right() + gap,
            analyzer_y,
            meter_w,
            (analyzer_h - gap) * 0.5,
        );
        let reduction_meter = UiRect::new(
            analyzer.right() + gap,
            peak_meter.bottom() + gap,
            meter_w,
            (analyzer_h - gap) * 0.5,
        );
        let tabs = UiRect::new(margin, analyzer.bottom() + gap, w - margin * 2.0, 36.0 * s);
        let controls = UiRect::new(
            margin,
            tabs.bottom() + gap,
            w - margin * 2.0,
            (h - tabs.bottom() - gap - margin).max(1.0),
        );
        let button_y = toolbar.y + 7.0 * s;
        let button_h = 28.0 * s;
        let save_button = UiRect::new(margin, button_y, 64.0 * s, button_h);
        let preset_button =
            UiRect::new(save_button.right() + 7.0 * s, button_y, 122.0 * s, button_h);
        let delete_button = UiRect::new(
            preset_button.right() + 7.0 * s,
            button_y,
            68.0 * s,
            button_h,
        );
        let undo_button = UiRect::new(
            delete_button.right() + 12.0 * s,
            button_y,
            64.0 * s,
            button_h,
        );
        let redo_button = UiRect::new(undo_button.right() + 7.0 * s, button_y, 64.0 * s, button_h);
        let ab_button = UiRect::new(redo_button.right() + 7.0 * s, button_y, 66.0 * s, button_h);
        let chaos_button = UiRect::new(ab_button.right() + 7.0 * s, button_y, 70.0 * s, button_h);
        let bypass_button = UiRect::new(
            chaos_button.right() + 12.0 * s,
            button_y,
            104.0 * s,
            button_h,
        );
        let midi_button = UiRect::new(
            bypass_button.right() + 8.0 * s,
            button_y,
            132.0 * s,
            button_h,
        );
        let toolbar_status = UiRect::new(
            midi_button.right() + 16.0 * s,
            button_y,
            (w - midi_button.right() - 28.0 * s).max(90.0 * s),
            button_h,
        );
        Self {
            full,
            header,
            toolbar,
            analyzer,
            peak_meter,
            reduction_meter,
            tabs,
            controls,
            save_button,
            preset_button,
            delete_button,
            undo_button,
            redo_button,
            ab_button,
            chaos_button,
            bypass_button,
            midi_button,
            toolbar_status,
            s,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct UiRect {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
}

impl UiRect {
    const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    fn right(self) -> f32 {
        self.x + self.w
    }

    fn bottom(self) -> f32 {
        self.y + self.h
    }

    fn center_x(self) -> f32 {
        self.x + self.w * 0.5
    }

    fn center_y(self) -> f32 {
        self.y + self.h * 0.5
    }

    fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.right() && y >= self.y && y <= self.bottom()
    }

    fn shrink(self, amount: f32) -> Self {
        Self::new(
            self.x + amount,
            self.y + amount,
            (self.w - amount * 2.0).max(1.0),
            (self.h - amount * 2.0).max(1.0),
        )
    }

    fn scale_hint(self) -> f32 {
        (self.h / 44.0).clamp(0.55, 1.6)
    }

    fn d2d(self) -> D2D_RECT_F {
        D2D_RECT_F {
            left: self.x,
            top: self.y,
            right: self.right(),
            bottom: self.bottom(),
        }
    }
}

#[derive(Clone, Copy)]
struct ControlCell {
    id: ControlId,
    rect: UiRect,
    knob_rect: UiRect,
    segment_rect: UiRect,
    value_rect: UiRect,
}

fn control_cells(layout: &Layout, tab: Tab) -> Vec<ControlCell> {
    let controls = tab.controls();
    let s = layout.s;
    let inner = layout.controls.shrink(14.0 * s);
    let top = inner.y + 30.0 * s;
    let available_h = (inner.bottom() - top).max(1.0);
    let columns = if inner.w >= 900.0 * s {
        4
    } else if inner.w >= 650.0 * s {
        3
    } else if inner.w >= 430.0 * s {
        2
    } else {
        1
    };
    let gap = 10.0 * s;
    let rows_per_col = controls.len().div_ceil(columns);
    let row_h = ((available_h - gap * (rows_per_col.saturating_sub(1)) as f32)
        / rows_per_col.max(1) as f32)
        .clamp(68.0 * s, 86.0 * s);
    let col_w = (inner.w - gap * (columns.saturating_sub(1)) as f32) / columns as f32;
    let mut cells = Vec::with_capacity(controls.len());

    for (index, id) in controls.iter().copied().enumerate() {
        let col = index / rows_per_col.max(1);
        let row = index % rows_per_col.max(1);
        let rect = UiRect::new(
            inner.x + (col_w + gap) * col as f32,
            top + (row_h + gap) * row as f32,
            col_w,
            row_h,
        );
        let knob_size = (rect.h * 0.43).min(rect.w * 0.32).max(28.0 * s);
        let knob_rect = UiRect::new(
            rect.center_x() - knob_size * 0.5,
            rect.y + 23.0 * s,
            knob_size,
            knob_size,
        );
        let segment_rect = UiRect::new(
            rect.x + 9.0 * s,
            rect.y + 33.0 * s,
            rect.w - 18.0 * s,
            24.0 * s,
        );
        let value_rect = UiRect::new(
            rect.x + 9.0 * s,
            rect.bottom() - 22.0 * s,
            rect.w - 18.0 * s,
            18.0 * s,
        );
        cells.push(ControlCell {
            id,
            rect,
            knob_rect,
            segment_rect,
            value_rect,
        });
    }

    cells
}

fn preset_menu_rect(layout: &Layout, preset_count: usize) -> Option<(UiRect, f32)> {
    if layout.preset_button.w <= 0.0 {
        return None;
    }

    let s = layout.s;
    let rows = preset_count.clamp(1, 8);
    let row_h = 28.0 * s;
    Some((
        UiRect::new(
            layout.preset_button.x,
            layout.preset_button.bottom() + 4.0 * s,
            (220.0 * s).max(layout.preset_button.w),
            row_h * rows as f32 + 10.0 * s,
        ),
        row_h,
    ))
}

fn draw_modal_panel(rt: &ID2D1HwndRenderTarget, brushes: &Brushes, rect: UiRect, scale: f32) {
    fill_round(rt, rect, 8.0 * scale, &brushes.panel_dark);
    stroke_round(rt, rect, 8.0 * scale, &brushes.cyan_dim, 1.0);
}

fn default_preset_name(input: &str, presets: &[(String, Snapshot)]) -> String {
    let base = if input.trim().is_empty() {
        format!("Preset {}", presets.len() + 1)
    } else {
        input.trim().to_string()
    };

    if !presets
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case(&base))
    {
        return base;
    }

    for suffix in 2.. {
        let candidate = format!("{base} {suffix}");
        if !presets
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case(&candidate))
        {
            return candidate;
        }
    }

    base
}

fn chaos_snapshot(seed: &mut u64) -> Snapshot {
    let mut snapshot = Snapshot::default();
    for id in ALL_CONTROLS {
        let spec = id.spec();
        let value = match spec.kind {
            ValueKind::Boolean => {
                if rand_unit(seed) > 0.42 {
                    1.0
                } else {
                    0.0
                }
            }
            ValueKind::Choice(labels) => (rand_unit(seed) * labels.len() as f64)
                .floor()
                .min(labels.len().saturating_sub(1) as f64),
            ValueKind::Decibel if matches!(id, ControlId::InputLevel | ControlId::OutputLevel) => {
                -12.0 + rand_unit(seed) * 18.0
            }
            _ => spec.value_from_unit(rand_unit(seed)),
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

#[derive(Clone, Copy)]
struct ModalLayout {
    rect: UiRect,
    title: UiRect,
    input: UiRect,
    ok: UiRect,
    cancel: UiRect,
}

fn preset_name_popup_layout(layout: &Layout) -> ModalLayout {
    modal_layout(layout, 360.0 * layout.s, 150.0 * layout.s)
}

fn numeric_popup_layout(layout: &Layout) -> ModalLayout {
    modal_layout(layout, 330.0 * layout.s, 150.0 * layout.s)
}

fn modal_layout(layout: &Layout, width: f32, height: f32) -> ModalLayout {
    let s = layout.s;
    let rect = UiRect::new(
        layout.full.center_x() - width * 0.5,
        layout.full.center_y() - height * 0.5,
        width,
        height,
    );
    ModalLayout {
        rect,
        title: UiRect::new(
            rect.x + 16.0 * s,
            rect.y + 12.0 * s,
            rect.w - 32.0 * s,
            24.0 * s,
        ),
        input: UiRect::new(
            rect.x + 16.0 * s,
            rect.y + 46.0 * s,
            rect.w - 32.0 * s,
            34.0 * s,
        ),
        ok: UiRect::new(
            rect.right() - 168.0 * s,
            rect.bottom() - 46.0 * s,
            72.0 * s,
            28.0 * s,
        ),
        cancel: UiRect::new(
            rect.right() - 88.0 * s,
            rect.bottom() - 46.0 * s,
            72.0 * s,
            28.0 * s,
        ),
    }
}

fn choice_dropdown_rect(anchor: UiRect, label_count: usize) -> UiRect {
    let row_h = anchor.h.max(20.0);
    UiRect::new(
        anchor.x,
        anchor.bottom() + 2.0,
        anchor.w,
        row_h * label_count.max(1) as f32,
    )
}

#[derive(Clone, Copy)]
enum MidiMenuAction {
    ToggleMidi,
    CleanUp,
    RollBack,
    Save,
}

#[derive(Clone, Copy)]
enum CleanupAction {
    Remove(u8),
    ClearAll,
}

fn midi_context_rect(layout: &Layout) -> UiRect {
    UiRect::new(
        layout.midi_button.x,
        layout.midi_button.bottom() + 4.0 * layout.s,
        132.0 * layout.s,
        116.0 * layout.s,
    )
}

fn midi_context_row(layout: &Layout, index: usize) -> UiRect {
    let menu = midi_context_rect(layout);
    UiRect::new(
        menu.x + 4.0 * layout.s,
        menu.y + 4.0 * layout.s + index as f32 * 27.0 * layout.s,
        menu.w - 8.0 * layout.s,
        26.0 * layout.s,
    )
}

fn midi_context_hit(layout: &Layout, x: f32, y: f32) -> Option<MidiMenuAction> {
    let menu = midi_context_rect(layout);
    if !menu.contains(x, y) {
        return None;
    }
    for index in 0..4 {
        if midi_context_row(layout, index).contains(x, y) {
            return Some(match index {
                0 => MidiMenuAction::ToggleMidi,
                1 => MidiMenuAction::CleanUp,
                2 => MidiMenuAction::RollBack,
                _ => MidiMenuAction::Save,
            });
        }
    }
    None
}

fn midi_cleanup_rect(layout: &Layout, mapping_count: usize) -> UiRect {
    let context = midi_context_rect(layout);
    let rows = mapping_count.min(8) + 1;
    UiRect::new(
        context.right() + 5.0 * layout.s,
        context.y + 27.0 * layout.s,
        230.0 * layout.s,
        8.0 * layout.s + rows as f32 * 26.0 * layout.s,
    )
}

fn midi_cleanup_row(layout: &Layout, index: usize) -> UiRect {
    let rect = midi_cleanup_rect(layout, 8);
    UiRect::new(
        rect.x + 4.0 * layout.s,
        rect.y + 4.0 * layout.s + index as f32 * 26.0 * layout.s,
        rect.w - 8.0 * layout.s,
        25.0 * layout.s,
    )
}

fn midi_cleanup_hit(
    layout: &Layout,
    x: f32,
    y: f32,
    mappings: &[(u8, u8)],
) -> Option<CleanupAction> {
    let visible_count = mappings.len().min(8);
    let rect = midi_cleanup_rect(layout, mappings.len());
    if !rect.contains(x, y) {
        return None;
    }
    let mut row = ((y - rect.y - 4.0 * layout.s) / (26.0 * layout.s)).floor() as usize;
    row = row.min(visible_count);
    if row == visible_count {
        return Some(CleanupAction::ClearAll);
    }
    mappings.get(row).map(|(cc, _)| CleanupAction::Remove(*cc))
}

fn tab_rects(layout: &Layout) -> Vec<(Tab, UiRect)> {
    let s = layout.s;
    let gap = 8.0 * s;
    let width = (layout.tabs.w - gap * 3.0) / 4.0;
    Tab::ALL
        .iter()
        .enumerate()
        .map(|(index, tab)| {
            (
                *tab,
                UiRect::new(
                    layout.tabs.x + (width + gap) * index as f32,
                    layout.tabs.y,
                    width,
                    layout.tabs.h,
                ),
            )
        })
        .collect()
}

fn freq_to_x(freq: f32, width: f32) -> f32 {
    let lmin = 20.0_f32.log10();
    let lmax = 22_000.0_f32.log10();
    (freq.clamp(20.0, 22_000.0).log10() - lmin) / (lmax - lmin) * width
}

fn card(rt: &ID2D1HwndRenderTarget, rect: UiRect, radius: f32, brushes: &Brushes) {
    fill_round(rt, rect, radius, &brushes.card);
    stroke_round(rt, rect, radius, &brushes.border, 1.0);
}

fn solid(rt: &ID2D1HwndRenderTarget, color: D2D1_COLOR_F) -> Option<ID2D1SolidColorBrush> {
    unsafe { rt.CreateSolidColorBrush(&color, None).ok() }
}

fn fill_rect(rt: &ID2D1HwndRenderTarget, rect: UiRect, brush: &ID2D1SolidColorBrush) {
    unsafe {
        rt.FillRectangle(&rect.d2d(), brush);
    }
}

fn fill_round(rt: &ID2D1HwndRenderTarget, rect: UiRect, radius: f32, brush: &ID2D1SolidColorBrush) {
    let rr = D2D1_ROUNDED_RECT {
        rect: rect.d2d(),
        radiusX: radius,
        radiusY: radius,
    };
    unsafe {
        rt.FillRoundedRectangle(&rr, brush);
    }
}

fn stroke_round(
    rt: &ID2D1HwndRenderTarget,
    rect: UiRect,
    radius: f32,
    brush: &ID2D1SolidColorBrush,
    width: f32,
) {
    let rr = D2D1_ROUNDED_RECT {
        rect: rect.d2d(),
        radiusX: radius,
        radiusY: radius,
    };
    unsafe {
        rt.DrawRoundedRectangle(
            &rr,
            brush,
            width,
            Option::<&windows::Win32::Graphics::Direct2D::ID2D1StrokeStyle>::None,
        );
    }
}

fn draw_line(
    rt: &ID2D1HwndRenderTarget,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    brush: &ID2D1SolidColorBrush,
    width: f32,
) {
    unsafe {
        rt.DrawLine(
            Vector2 { X: x0, Y: y0 },
            Vector2 { X: x1, Y: y1 },
            brush,
            width,
            Option::<&windows::Win32::Graphics::Direct2D::ID2D1StrokeStyle>::None,
        );
    }
}

fn fill_circle(
    rt: &ID2D1HwndRenderTarget,
    x: f32,
    y: f32,
    radius: f32,
    brush: &ID2D1SolidColorBrush,
) {
    let ellipse = D2D1_ELLIPSE {
        point: Vector2 { X: x, Y: y },
        radiusX: radius,
        radiusY: radius,
    };
    unsafe {
        rt.FillEllipse(&ellipse, brush);
    }
}

fn stroke_circle(
    rt: &ID2D1HwndRenderTarget,
    x: f32,
    y: f32,
    radius: f32,
    brush: &ID2D1SolidColorBrush,
    width: f32,
) {
    let ellipse = D2D1_ELLIPSE {
        point: Vector2 { X: x, Y: y },
        radiusX: radius,
        radiusY: radius,
    };
    unsafe {
        rt.DrawEllipse(
            &ellipse,
            brush,
            width,
            Option::<&windows::Win32::Graphics::Direct2D::ID2D1StrokeStyle>::None,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_arc(
    rt: &ID2D1HwndRenderTarget,
    cx: f32,
    cy: f32,
    radius: f32,
    start: f32,
    end: f32,
    brush: &ID2D1SolidColorBrush,
    width: f32,
) {
    let steps = 30;
    let span = end - start;
    let mut prev = None;
    for index in 0..=steps {
        let angle = start + span * index as f32 / steps as f32;
        let point = (cx + radius * angle.cos(), cy + radius * angle.sin());
        if let Some((px, py)) = prev {
            draw_line(rt, px, py, point.0, point.1, brush, width);
        }
        prev = Some(point);
    }
}

fn draw_knob(
    rt: &ID2D1HwndRenderTarget,
    rect: UiRect,
    unit: f32,
    accent: Accent,
    brushes: &Brushes,
    s: f32,
) {
    let unit = unit.clamp(0.0, 1.0);
    let radius = rect.w.min(rect.h) * 0.5;
    let cx = rect.center_x();
    let cy = rect.center_y();
    let start = std::f32::consts::PI * 0.75;
    let sweep = std::f32::consts::PI * 1.5;
    let angle = start + sweep * unit;
    let accent_brush = accent.brush(brushes);

    fill_circle(rt, cx, cy + 1.5 * s, radius + 1.0 * s, &brushes.black);
    fill_circle(rt, cx, cy, radius, &brushes.control);
    stroke_circle(rt, cx, cy, radius, &brushes.border, 1.0);
    draw_arc(
        rt,
        cx,
        cy,
        radius * 0.78,
        start,
        start + sweep,
        &brushes.border,
        3.0 * s,
    );
    if unit > 0.003 {
        draw_arc(
            rt,
            cx,
            cy,
            radius * 0.78,
            start,
            angle,
            accent_brush,
            3.2 * s,
        );
    }

    let dot_x = cx + radius * 0.48 * angle.cos();
    let dot_y = cy + radius * 0.48 * angle.sin();
    fill_circle(rt, dot_x, dot_y, 2.7 * s, accent_brush);
    fill_circle(rt, dot_x, dot_y, 1.2 * s, &brushes.text_primary);
}

enum Align {
    Leading,
    Center,
    Trailing,
}

fn draw_text(
    rt: &ID2D1HwndRenderTarget,
    text: &str,
    rect: UiRect,
    format: &IDWriteTextFormat,
    brush: &ID2D1SolidColorBrush,
    align: Align,
) {
    let wide: Vec<u16> = text.encode_utf16().collect();
    if wide.is_empty() {
        return;
    }
    let alignment = match align {
        Align::Leading => DWRITE_TEXT_ALIGNMENT_LEADING,
        Align::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
        Align::Trailing => DWRITE_TEXT_ALIGNMENT_TRAILING,
    };
    let _ = unsafe { format.SetTextAlignment(alignment) };
    let _ = unsafe { format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER) };
    unsafe {
        rt.DrawText(
            &wide,
            format,
            &rect.d2d(),
            brush,
            D2D1_DRAW_TEXT_OPTIONS_NONE,
            DWRITE_MEASURING_MODE_NATURAL,
        );
    }
}

fn client_size(hwnd: HWND) -> Option<(u32, u32)> {
    let mut rect = RECT::default();
    unsafe { GetClientRect(hwnd, &mut rect).ok()? };
    Some((
        (rect.right - rect.left).max(1) as u32,
        (rect.bottom - rect.top).max(1) as u32,
    ))
}

fn invalidate(hwnd: HWND) {
    let _ = unsafe { InvalidateRect(Some(hwnd), None, false) };
}

fn class_name() -> PCWSTR {
    w!("NebulaClusterNativeEditor")
}

fn module_instance() -> Option<HINSTANCE> {
    unsafe {
        GetModuleHandleW(None)
            .ok()
            .map(|module| HINSTANCE(module.0))
    }
}

fn register_window_class() -> bool {
    static REGISTER_ONCE: Once = Once::new();
    static REGISTERED: AtomicBool = AtomicBool::new(false);

    REGISTER_ONCE.call_once(|| {
        let Some(instance) = module_instance() else {
            return;
        };
        let cursor = unsafe { LoadCursorW(None, IDC_ARROW).unwrap_or_default() };
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
            lpfnWndProc: Some(window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: Default::default(),
            hCursor: cursor,
            hbrBackground: HBRUSH::default(),
            lpszMenuName: PCWSTR::null(),
            lpszClassName: class_name(),
        };
        let atom = unsafe { RegisterClassW(&wc) };
        if atom != 0 || unsafe { GetLastError() } == ERROR_CLASS_ALREADY_EXISTS {
            REGISTERED.store(true, Ordering::Release);
        }
    });

    REGISTERED.load(Ordering::Acquire)
}

extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        if msg == WM_NCCREATE {
            let create = lparam.0 as *const CREATESTRUCTW;
            if !create.is_null() {
                let state = (*create).lpCreateParams.cast::<NativeWindowState>();
                if !state.is_null() {
                    (*state).hwnd = hwnd;
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize);
                    let _ = SetTimer(Some(hwnd), TIMER_ID, TIMER_MS, None);
                    return LRESULT(1);
                }
            }
            return LRESULT(0);
        }

        let state_ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut NativeWindowState;
        if msg == WM_NCDESTROY {
            let _ = KillTimer(Some(hwnd), TIMER_ID);
            if !state_ptr.is_null() {
                (*state_ptr).midi_learn.save_current_mapping();
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                drop(Box::from_raw(state_ptr));
            }
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }

        if state_ptr.is_null() {
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        let state = &mut *state_ptr;

        match msg {
            WM_ERASEBKGND => LRESULT(1),
            WM_GETDLGCODE => {
                if state.preset_name_input.is_some() || state.numeric_input.is_some() {
                    LRESULT((DLGC_WANTALLKEYS | DLGC_WANTCHARS) as isize)
                } else {
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }
            }
            WM_SIZE => {
                state.render_target = None;
                state.text_formats = None;
                invalidate(hwnd);
                LRESULT(0)
            }
            WM_TIMER => {
                invalidate(hwnd);
                LRESULT(0)
            }
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                BeginPaint(hwnd, &mut ps);
                state.paint();
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_LBUTTONDOWN => {
                let (x, y) = point_from_lparam(lparam);
                state.mouse_down(x, y);
                LRESULT(0)
            }
            WM_LBUTTONDBLCLK => {
                let (x, y) = point_from_lparam(lparam);
                state.mouse_double_click(x, y);
                LRESULT(0)
            }
            WM_RBUTTONDOWN => {
                let (x, y) = point_from_lparam(lparam);
                state.mouse_right_down(x, y);
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                if state.drag.is_some() {
                    let (x, y) = point_from_lparam(lparam);
                    state.mouse_move(x, y);
                    LRESULT(0)
                } else {
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }
            }
            WM_LBUTTONUP => {
                let (x, y) = point_from_lparam(lparam);
                state.mouse_up(x, y);
                LRESULT(0)
            }
            WM_CHAR => {
                if state.preset_name_input.is_some() || state.numeric_input.is_some() {
                    if let Some(ch) = char::from_u32(wparam.0 as u32) {
                        state.char_input(ch);
                        invalidate(hwnd);
                    }
                    LRESULT(0)
                } else {
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }
            }
            WM_KEYDOWN => {
                let wants_key = state.preset_name_input.is_some()
                    || state.numeric_input.is_some()
                    || state.choice_dropdown.is_some()
                    || state.preset_menu_open
                    || state.midi_context_menu_open
                    || state.midi_cleanup_menu_open;
                if wants_key {
                    state.key_down(wparam.0 as u32);
                    invalidate(hwnd);
                    LRESULT(0)
                } else {
                    DefWindowProcW(hwnd, msg, wparam, lparam)
                }
            }
            WM_CAPTURECHANGED | WM_CANCELMODE => {
                state.clear_drag();
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

fn point_from_lparam(lparam: LPARAM) -> (f32, f32) {
    let raw = lparam.0 as u32;
    let x = (raw & 0xffff) as u16 as i16 as f32;
    let y = ((raw >> 16) & 0xffff) as u16 as i16 as f32;
    (x, y)
}
