//! Hàm dùng chung cho giao diện: màu, hàng danh sách, hộp xác nhận, theme và typography.

use gpui::{
    div, hsla, prelude::*, px, App, Context, Div, ElementId, Hsla, Pixels, SharedString,
    Stateful, Window,
};
use gpui_component::{highlighter::HighlightTheme, h_flex, ActiveTheme, Theme};

/// Font chữ đều mặc định cho diff, blame, đồ thị.
pub const MONO: &str = if cfg!(target_os = "windows") { "Consolas" } else { "Menlo" };
/// Chiều cao tiêu chuẩn một dòng trong danh sách (chuẩn macOS Table View: thanh mảnh, sắc nét).
pub const ROW_H: Pixels = px(25.);
/// Độ rộng một làn của đồ thị nhánh.
pub const LANE_W: f32 = 14.;
/// Cỡ chữ giao diện tiêu chuẩn.
pub const UI_FONT_SIZE: f32 = 14.0;

/// Màu của làn thứ `i` trong đồ thị.
pub fn lane_color(i: usize) -> Hsla {
    const HUES: [f32; 8] = [0.58, 0.33, 0.08, 0.83, 0.0, 0.5, 0.15, 0.7];
    hsla(HUES[i % HUES.len()], 0.65, 0.52, 1.0)
}

/// Màu theo ký hiệu thay đổi git (A, M, D, R, U, ?).
pub fn status_color(mark: char, cx: &App) -> Hsla {
    let t = cx.theme();
    match mark {
        'A' | '?' => t.success,
        'D' => t.danger,
        'U' => t.warning,
        'R' | 'C' => t.info,
        _ => t.warning,
    }
}

/// Chuyển đổi RGB sang Hsla chuẩn xác.
pub fn rgb_color(r: u8, g: u8, b: u8) -> Hsla {
    let r = r as f32 / 255.0;
    let g = g as f32 / 255.0;
    let b = b as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let l = (max + min) / 2.0;
    let (h, s) = if delta == 0.0 {
        (0.0, 0.0)
    } else {
        let s = if l < 0.5 {
            delta / (max + min)
        } else {
            delta / (2.0 - max - min)
        };
        let h = if max == r {
            (g - b) / delta + (if g < b { 6.0 } else { 0.0 })
        } else if max == g {
            (b - r) / delta + 2.0
        } else {
            (r - g) / delta + 4.0
        } / 6.0;
        (h, s)
    };
    hsla(h, s, l, 1.0)
}

/// Chuyển chuỗi mã hex (ví dụ "#0f172a") sang Hsla.
pub fn hex_color(hex: &str) -> Hsla {
    let hex = hex.trim_start_matches('#');
    if hex.len() >= 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
        rgb_color(r, g, b)
    } else {
        hsla(0.0, 0.0, 0.5, 1.0)
    }
}

/// Thông tin một mẫu giao diện có sẵn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThemePreset {
    pub id: &'static str,
    pub name: &'static str,
    pub desc_key: &'static str,
    pub is_dark: bool,
    pub bg: Hsla,
    pub card: Hsla,
    pub border: Hsla,
    pub primary: Hsla,
    pub fg: Hsla,
    pub muted_fg: Hsla,
    pub list_hover: Hsla,
    pub list_active: Hsla,
    pub tab_bar: Hsla,
    pub tab_active: Hsla,
}

pub fn get_theme_presets() -> Vec<ThemePreset> {
    vec![
        ThemePreset {
            id: "system",
            name: "macOS Dark (System)",
            desc_key: "settings.theme.system_desc",
            is_dark: true,
            bg: hex_color("#1c1c1e"),
            card: hex_color("#2c2c30"),
            border: hex_color("#3e3e44"),
            primary: hex_color("#0a84ff"),
            fg: hex_color("#f2f2f7"),
            muted_fg: hex_color("#8e8e93"),
            list_hover: hex_color("#28282b"),
            list_active: hex_color("#0062e3"),
            tab_bar: hex_color("#161618"),
            tab_active: hex_color("#252528"),
        },
        ThemePreset {
            id: "modern-dark",
            name: "macOS Obsidian",
            desc_key: "settings.theme.modern_dark_desc",
            is_dark: true,
            bg: hex_color("#18181a"),
            card: hex_color("#28282d"),
            border: hex_color("#3a3a40"),
            primary: hex_color("#007aff"),
            fg: hex_color("#f5f5f7"),
            muted_fg: hex_color("#8e8e93"),
            list_hover: hex_color("#262629"),
            list_active: hex_color("#0062e3"),
            tab_bar: hex_color("#131315"),
            tab_active: hex_color("#222225"),
        },
        ThemePreset {
            id: "one-dark",
            name: "One Dark Pro",
            desc_key: "settings.theme.one_dark_desc",
            is_dark: true,
            bg: hex_color("#282c34"),
            card: hex_color("#21252b"),
            border: hex_color("#1e2227"),
            primary: hex_color("#61afef"),
            fg: hex_color("#abb2bf"),
            muted_fg: hex_color("#6b7280"),
            list_hover: hex_color("#2c313a"),
            list_active: hex_color("#3e4451"),
            tab_bar: hex_color("#21252b"),
            tab_active: hex_color("#282c34"),
        },
        ThemePreset {
            id: "nord",
            name: "Nord Frost",
            desc_key: "settings.theme.nord_desc",
            is_dark: true,
            bg: hex_color("#2e3440"),
            card: hex_color("#3b4252"),
            border: hex_color("#4c566a"),
            primary: hex_color("#88c0d0"),
            fg: hex_color("#eceff4"),
            muted_fg: hex_color("#d8dee9"),
            list_hover: hex_color("#3b4252"),
            list_active: hex_color("#434c5e"),
            tab_bar: hex_color("#252a34"),
            tab_active: hex_color("#3b4252"),
        },
        ThemePreset {
            id: "tokyo-night",
            name: "Tokyo Night",
            desc_key: "settings.theme.tokyo_night_desc",
            is_dark: true,
            bg: hex_color("#1a1b26"),
            card: hex_color("#24283b"),
            border: hex_color("#2f3549"),
            primary: hex_color("#7aa2f7"),
            fg: hex_color("#c0caf5"),
            muted_fg: hex_color("#565f89"),
            list_hover: hex_color("#24283b"),
            list_active: hex_color("#33384f"),
            tab_bar: hex_color("#16161e"),
            tab_active: hex_color("#24283b"),
        },
        ThemePreset {
            id: "modern-light",
            name: "Modern Light",
            desc_key: "settings.theme.modern_light_desc",
            is_dark: false,
            bg: hex_color("#ffffff"),
            card: hex_color("#f8fafc"),
            border: hex_color("#e2e8f0"),
            primary: hex_color("#2563eb"),
            fg: hex_color("#0f172a"),
            muted_fg: hex_color("#64748b"),
            list_hover: hex_color("#f1f5f9"),
            list_active: hex_color("#e2e8f0"),
            tab_bar: hex_color("#f8fafc"),
            tab_active: hex_color("#ffffff"),
        },
        ThemePreset {
            id: "warm-paper",
            name: "Warm Paper",
            desc_key: "settings.theme.warm_paper_desc",
            is_dark: false,
            bg: hex_color("#fbf8f3"),
            card: hex_color("#f3ede2"),
            border: hex_color("#e3dac9"),
            primary: hex_color("#b45309"),
            fg: hex_color("#292524"),
            muted_fg: hex_color("#78716c"),
            list_hover: hex_color("#f3ede2"),
            list_active: hex_color("#e7ded0"),
            tab_bar: hex_color("#eee7dc"),
            tab_active: hex_color("#fbf8f3"),
        },
    ]
}

/// Hàng danh sách cao `ROW_H`, bo góc nhẹ, đổi nền khi chọn / rê chuột.
pub fn list_row(id: impl Into<ElementId>, selected: bool, cx: &App) -> Stateful<Div> {
    let t = cx.theme();
    let hover = t.list_hover;
    h_flex()
        .id(id)
        .h(ROW_H)
        .px_2()
        .gap_2()
        .w_full()
        .flex_none()
        .text_sm()
        .rounded_sm()
        .overflow_hidden()
        .when(selected, |d| d.bg(t.list_active).text_color(gpui::white()))
        .when(!selected, |d| d.hover(move |s| s.bg(hover)))
}

/// Tiêu đề nhóm trong cột trái (rõ ràng, phân cấp thị giác tinh tế).
pub fn section_title(title: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .px_2()
        .pt_3()
        .pb_1()
        .text_xs()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(cx.theme().muted_foreground)
        .child(title.into())
}

/// Thêm mục menu chuột phải có kèm phím tắt (hiển thị căn phải giống Fork desktop).
pub fn menu_item_with_shortcut(
    menu: gpui_component::menu::PopupMenu,
    label: impl Into<SharedString>,
    shortcut: Option<&'static str>,
    action: Box<dyn gpui::Action>,
) -> gpui_component::menu::PopupMenu {
    let label = label.into();
    if let Some(sc) = shortcut {
        menu.menu_element(action, move |_window, cx| {
            let t = cx.theme();
            h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .gap_4()
                .child(div().child(label.clone()))
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::NORMAL)
                        .text_color(t.muted_foreground)
                        .child(sc),
                )
        })
    } else {
        menu.menu(label, action)
    }
}

/// Look of the confirm button of a modal.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ModalTone {
    /// Normal action (blue).
    Primary,
    /// Destructive action (red): delete, discard, reset…
    Danger,
}

/// Standard in-app modal: title, one line of detail, a row of buttons.
/// `buttons[0]` is the cancel button; `on_answer` gets the index of the button clicked
/// (it is not called on cancel / Escape / click outside). Buttons after the second are always red.
pub fn confirm_with_tone<V: 'static>(
    window: &mut Window,
    cx: &mut Context<V>,
    tone: ModalTone,
    message: &str,
    detail: &str,
    buttons: &[&str],
    on_answer: impl FnOnce(&mut V, usize, &mut Window, &mut Context<V>) + 'static,
) {
    use gpui_component::{
        button::{Button, ButtonVariants},
        v_flex, Icon, IconName, WindowExt,
    };
    use std::{cell::RefCell, rc::Rc};

    let entity = cx.entity();
    let on_answer = Rc::new(RefCell::new(Some(on_answer)));
    let title = SharedString::from(message.to_string());
    let detail = SharedString::from(detail.to_string());
    let labels: Vec<SharedString> = buttons.iter().map(|b| SharedString::from(b.to_string())).collect();
    window.open_dialog(cx, move |dialog, _, cx| {
        let t = cx.theme();
        let (icon, color, category): (Icon, Hsla, &'static str) = match tone {
            ModalTone::Danger => (IconName::CircleX.into(), t.danger, "WARNING / DESTRUCTIVE ACTION"),
            ModalTone::Primary => (IconName::CircleCheck.into(), t.primary, "ACTION CONFIRMATION"),
        };
        let entity = entity.clone();
        let on_answer = on_answer.clone();
        let labels = labels.clone();
        dialog
            .title(title.clone())
            .width(px(480.))
            .close_button(false)
            .overlay_closable(true)
            .child(
                v_flex()
                    .gap_3()
                    .pt_1()
                    .child(
                        h_flex()
                            .gap_3()
                            .items_start()
                            .p(px(12.))
                            .rounded_lg()
                            .bg(t.secondary)
                            .border_1()
                            .border_color(t.border)
                            .child(
                                div()
                                    .size(px(40.))
                                    .flex_none()
                                    .rounded_lg()
                                    .bg(color.opacity(0.18))
                                    .border_1()
                                    .border_color(color.opacity(0.45))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_color(color)
                                    .child(Icon::new(icon).size_5()),
                            )
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap(px(3.))
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(gpui::FontWeight::BOLD)
                                            .text_color(color)
                                            .child(category),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(t.foreground)
                                            .whitespace_normal()
                                            .child(detail.clone()),
                                    ),
                            ),
                    ),
            )
            .footer(move |_, _, _, _| {
                labels
                    .iter()
                    .enumerate()
                    .map(|(i, label)| {
                        let entity = entity.clone();
                        let on_answer = on_answer.clone();
                        let button = Button::new(("modal-button", i)).label(label.clone());
                        let button = match (i, tone) {
                            (0, _) => button,
                            (1, ModalTone::Primary) => button.primary(),
                            _ => button.danger(),
                        };
                        button.on_click(move |_, window, cx| {
                            if i > 0 {
                                if let Some(f) = on_answer.borrow_mut().take() {
                                    entity.update(cx, |this, cx| f(this, i, window, cx));
                                }
                            }
                            window.close_dialog(cx);
                        })
                    })
                    .collect()
            })
    });
}

/// Destructive confirmation (red confirm button).
pub fn confirm<V: 'static>(
    window: &mut Window,
    cx: &mut Context<V>,
    message: &str,
    detail: &str,
    buttons: &[&str],
    on_answer: impl FnOnce(&mut V, usize, &mut Window, &mut Context<V>) + 'static,
) {
    confirm_with_tone(window, cx, ModalTone::Danger, message, detail, buttons, on_answer)
}

/// Rút gọn đường dẫn dài: giữ tên file, cắt bớt thư mục.
pub fn split_path(rel: &str) -> (String, String) {
    match rel.rfind('/') {
        Some(i) => (rel[i + 1..].to_string(), rel[..i].to_string()),
        None => (rel.to_string(), String::new()),
    }
}

/// Bảng màu cú pháp phù hợp với từng chủ đề (Code Syntax Highlighter).
pub fn syntax_theme_for(theme_id: &str, dark: bool) -> HighlightTheme {
    let (kw, ctl, st, num, cm, func, ty, prop, tag, attr, constant) = match theme_id {
        "one-dark" => (
            "#c678dd", "#e06c75", "#98c379", "#d19a66", "#5c6370", "#61afef", "#e5c07b", "#abb2bf", "#e06c75", "#d19a66", "#56b6c2",
        ),
        "nord" => (
            "#81a1c1", "#81a1c1", "#a3be8c", "#b48ead", "#616e88", "#88c0d0", "#8fbcbb", "#d8dee9", "#81a1c1", "#ebcb8b", "#d08770",
        ),
        "tokyo-night" => (
            "#bb9af7", "#bb9af7", "#9ece6a", "#ff9e64", "#565f89", "#7aa2f7", "#2ac3de", "#c0caf5", "#f7768e", "#e0af68", "#7dcfff",
        ),
        "warm-paper" => (
            "#7e22ce", "#9a3412", "#15803d", "#b45309", "#78716c", "#1d4ed8", "#0f766e", "#292524", "#9a3412", "#b45309", "#c2410c",
        ),
        _ if dark => (
            "#569CD6", "#C586C0", "#CE9178", "#B5CEA8", "#6A9955", "#DCDCAA", "#4EC9B0", "#9CDCFE", "#569CD6", "#9CDCFE", "#4FC1FF",
        ),
        _ => (
            "#0000FF", "#AF00DB", "#A31515", "#098658", "#008000", "#795E26", "#267F99", "#001080", "#800000", "#E50000", "#0070C1",
        ),
    };
    let c = |color: &str| serde_json::json!({ "color": color });
    let json = serde_json::json!({
        "name": format!("Branchpad {theme_id}"),
        "appearance": if dark { "dark" } else { "light" },
        "style": {
            "syntax": {
                "keyword": c(kw), "preproc": c(ctl), "boolean": c(kw), "constant": c(constant),
                "number": c(num), "string": c(st), "string.escape": c(constant), "string.regex": c(st),
                "string.special": c(st), "string.special.symbol": c(constant),
                "comment": { "color": cm, "font_style": "italic" },
                "comment.doc": { "color": cm, "font_style": "italic" },
                "function": c(func), "type": c(ty), "constructor": c(ty), "enum": c(ty), "variant": c(constant),
                "property": c(prop), "variable.special": c(kw), "attribute": c(attr), "tag": c(tag),
                "title": { "color": kw, "font_weight": 600 }, "link_uri": c(prop), "link_text": c(st),
                "text.literal": c(st)
            }
        }
    });
    serde_json::from_value(json).expect("valid syntax palette")
}

/// Bảng màu cú pháp tương thích cũ (VS Code Dark+ / Light+).
pub fn syntax_theme(dark: bool) -> HighlightTheme {
    syntax_theme_for("default", dark)
}

/// Áp dụng đầy đủ Cài đặt (Theme, cỡ chữ UI, màu sắc cú pháp) vào cửa sổ và Context.
pub fn apply_settings(settings: &branchpad_core::Settings, window: &mut Window, cx: &mut App) {
    let ui_size = settings.ui_font_size.max(12.0);
    window.set_rem_size(px((ui_size + 1.0).max(14.0)));

    if settings.theme == "system" {
        Theme::sync_system_appearance(Some(window), cx);
        let theme = Theme::global_mut(cx);
        theme.font_size = px(ui_size);
        theme.highlight_theme = std::sync::Arc::new(syntax_theme(theme.mode.is_dark()));
    } else {
        let presets = get_theme_presets();
        if let Some(p) = presets.iter().find(|p| p.id == settings.theme) {
            let theme = Theme::global_mut(cx);
            theme.font_size = px(ui_size);
            theme.background = p.bg;
            theme.foreground = p.fg;
            theme.secondary = p.card;
            theme.border = p.border;
            theme.primary = p.primary;
            theme.muted_foreground = p.muted_fg;
            theme.list_hover = p.list_hover;
            theme.list_active = p.list_active;
            theme.tab_bar = p.tab_bar;
            theme.tab_active = p.tab_active;
            theme.highlight_theme = std::sync::Arc::new(syntax_theme_for(p.id, p.is_dark));
        } else {
            Theme::sync_system_appearance(Some(window), cx);
            let theme = Theme::global_mut(cx);
            theme.font_size = px(ui_size);
            theme.highlight_theme = std::sync::Arc::new(syntax_theme(theme.mode.is_dark()));
        }
    }
    let theme = Theme::global_mut(cx);
    theme.overlay = hsla(0.0, 0.0, 0.0, 0.65);
    window.refresh();
}

/// Sáng/tối theo hệ thống hoặc theo settings đã lưu. Gọi lại khi hệ thống đổi giao diện.
pub fn apply_appearance(window: &mut Window, cx: &mut App) {
    let settings = branchpad_core::Settings::load();
    apply_settings(&settings, window, cx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syntax_theme_has_colors() {
        let t = syntax_theme(true);
        for n in ["keyword", "string", "number", "comment", "function"] {
            let s = t.style(n);
            assert!(s.is_some_and(|s| s.color.is_some()), "missing color for {n}: {s:?}");
        }
    }

    #[test]
    fn rgb_to_hsla_black_and_white() {
        let black = rgb_color(0, 0, 0);
        assert_eq!(black.l, 0.0);
        let white = rgb_color(255, 255, 255);
        assert_eq!(white.l, 1.0);
    }

    #[test]
    fn presets_not_empty() {
        let list = get_theme_presets();
        assert!(list.len() >= 6);
        assert!(list.iter().any(|p| p.id == "modern-dark"));
    }
}
