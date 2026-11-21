//! Hàm dùng chung cho giao diện: màu, hàng danh sách, hộp xác nhận.

use gpui::{
    div, hsla, prelude::*, px, App, Context, Div, ElementId, Hsla, Pixels, PromptLevel, SharedString,
    Stateful, Window,
};
use gpui_component::{highlighter::HighlightTheme, h_flex, ActiveTheme, Theme};

/// Font chữ đều cho diff, blame, đồ thị.
pub const MONO: &str = "Menlo";
/// Chiều cao một dòng trong danh sách.
pub const ROW_H: Pixels = px(22.);
/// Độ rộng một làn của đồ thị nhánh.
pub const LANE_W: f32 = 14.;

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

/// Hàng danh sách cao `ROW_H`, đổi nền khi chọn / rê chuột.
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
        .overflow_hidden()
        .when(selected, |d| d.bg(t.list_active))
        .when(!selected, |d| d.hover(move |s| s.bg(hover)))
}

/// Tiêu đề nhóm trong cột trái.
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

/// Hộp xác nhận gốc của macOS. `buttons[0]` là nút huỷ; `on_answer` nhận
/// chỉ số nút được bấm (không gọi khi huỷ).
pub fn confirm<V: 'static>(
    window: &mut Window,
    cx: &mut Context<V>,
    message: &str,
    detail: &str,
    buttons: &[&str],
    on_answer: impl FnOnce(&mut V, usize, &mut Window, &mut Context<V>) + 'static,
) {
    let rx = window.prompt(PromptLevel::Warning, message, Some(detail), buttons, cx);
    cx.spawn_in(window, async move |this, cx| {
        if let Ok(answer) = rx.await {
            if answer > 0 {
                this.update_in(cx, |this, window, cx| on_answer(this, answer, window, cx))
                    .ok();
            }
        }
    })
    .detach();
}

/// Rút gọn đường dẫn dài: giữ tên file, cắt bớt thư mục.
pub fn split_path(rel: &str) -> (String, String) {
    match rel.rfind('/') {
        Some(i) => (rel[i + 1..].to_string(), rel[..i].to_string()),
        None => (rel.to_string(), String::new()),
    }
}

/// Cỡ chữ giao diện (không phải code).
const UI_FONT_SIZE: f32 = 14.;

/// Bảng màu cú pháp (gần VS Code Dark+ / Light+) — màu mặc định của gpui-component quá nhạt.
fn syntax_theme(dark: bool) -> HighlightTheme {
    let (kw, ctl, st, num, cm, func, ty, prop, _op, tag, attr, constant) = if dark {
        ("#569CD6", "#C586C0", "#CE9178", "#B5CEA8", "#6A9955", "#DCDCAA", "#4EC9B0", "#9CDCFE", "#D4D4D4", "#569CD6", "#9CDCFE", "#4FC1FF")
    } else {
        ("#0000FF", "#AF00DB", "#A31515", "#098658", "#008000", "#795E26", "#267F99", "#001080", "#333333", "#800000", "#E50000", "#0070C1")
    };
    let c = |color: &str| serde_json::json!({ "color": color });
    let json = serde_json::json!({
        "name": if dark { "Branchpad Dark" } else { "Branchpad Light" },
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

/// Sáng/tối theo hệ thống + cỡ chữ + màu cú pháp. Gọi lại mỗi khi hệ thống đổi giao diện.
pub fn apply_appearance(window: &mut Window, cx: &mut App) {
    window.set_rem_size(px(16.));
    Theme::sync_system_appearance(Some(window), cx);
    let theme = Theme::global_mut(cx);
    theme.font_size = px(UI_FONT_SIZE);
    theme.highlight_theme = std::sync::Arc::new(syntax_theme(theme.mode.is_dark()));
    window.refresh();
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
}
