//! Hàm dùng chung cho giao diện: màu, hàng danh sách, hộp xác nhận.

use gpui::{
    div, hsla, prelude::*, px, App, Context, Div, ElementId, Hsla, Pixels, PromptLevel, SharedString,
    Stateful, Window,
};
use gpui_component::{h_flex, ActiveTheme};

/// Font chữ đều cho diff, blame, đồ thị.
pub const MONO: &str = "Menlo";
/// Chiều cao một dòng trong danh sách.
pub const ROW_H: Pixels = px(24.);
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
