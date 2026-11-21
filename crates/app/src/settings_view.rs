//! Màn hình cài đặt: chọn giao diện (theme), chỉnh font chữ, mật độ hiển thị.

use gpui::{
    div, prelude::*, px, AnyElement, Context, Hsla, SharedString, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex, v_flex, ActiveTheme, Icon, Sizable,
};

use crate::icons::BpIcon;
use crate::ui::{get_theme_presets, ThemePreset};
use crate::workspace::Workspace;

impl Workspace {
    /// Vẽ hộp thoại Modal Cài đặt (phủ lên trên workspace).
    pub(crate) fn render_settings_modal(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = cx.theme();
        let card = t.secondary;
        let border = t.border;
        let fg = t.foreground;
        let muted = t.muted_foreground;
        let primary = t.primary;

        let current_theme = self.settings.theme.clone();
        let ui_font_size = self.settings.ui_font_size;
        let editor_font_size = self.settings.editor_font_size;
        let row_height = self.settings.row_height;
        let mono_font = self.settings.mono_font.clone();

        let presets = get_theme_presets();

        // Modal container căn giữa với backdrop mờ
        div()
            .id("settings-modal-backdrop")
            .absolute()
            .inset_0()
            .bg(gpui::black().opacity(0.65))
            .flex()
            .items_center()
            .justify_center()
            .p_6()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.show_settings = false;
                    cx.notify();
                }),
            )
            .child(
                v_flex()
                    .id("settings-modal-dialog")
                    .w(px(720.))
                    .max_h(px(620.))
                    .bg(card)
                    .border_1()
                    .border_color(border)
                    .rounded_lg()
                    .shadow_lg()
                    .overflow_hidden()
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    // Header của Modal
                    .child(
                        h_flex()
                            .h(px(52.))
                            .px_4()
                            .flex_none()
                            .border_b_1()
                            .border_color(border)
                            .justify_between()
                            .items_center()
                            .child(
                                h_flex()
                                    .gap_3()
                                    .items_center()
                                    .child(
                                        div()
                                            .p_1()
                                            .rounded_md()
                                            .bg(primary.opacity(0.12))
                                            .text_color(primary)
                                            .child(Icon::new(BpIcon::Settings).size_4()),
                                    )
                                    .child(
                                        v_flex()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_base()
                                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                                    .text_color(fg)
                                                    .child(t!("settings.title")),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(muted)
                                                    .child(t!("settings.subtitle")),
                                            ),
                                    ),
                            )
                            .child(
                                Button::new("close-settings-x")
                                    .label("✕")
                                    .ghost()
                                    .small()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.show_settings = false;
                                        cx.notify();
                                    })),
                            ),
                    )
                    // Nội dung các thiết lập (scrollable)
                    .child(
                        v_flex()
                            .id("settings-modal-content")
                            .flex_1()
                            .overflow_y_scroll()
                            .p_4()
                            .gap_4()
                            // Section 1: Mẫu Giao diện (Themes) - xếp 2 cột
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(fg)
                                            .child(t!("settings.section.appearance")),
                                    )
                                    .children(presets.chunks(2).map(|chunk| {
                                        h_flex()
                                            .gap_3()
                                            .w_full()
                                            .children(chunk.iter().map(|&p| {
                                                let is_active = p.id == current_theme;
                                                div().flex_1().child(self.render_theme_card(p, is_active, cx))
                                            }))
                                    })),
                            )
                            // Section 2: Cỡ chữ Giao diện UI (Typography)
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(
                                        h_flex()
                                            .justify_between()
                                            .items_center()
                                            .child(
                                                v_flex()
                                                    .gap_1()
                                                    .child(
                                                        div()
                                                            .text_sm()
                                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                                            .text_color(fg)
                                                            .child(t!("settings.ui_font_size")),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(muted)
                                                            .child(t!("settings.ui_font_desc")),
                                                    ),
                                            )
                                            .child(
                                                h_flex()
                                                    .gap_2()
                                                    .items_center()
                                                    .child(
                                                        Button::new("dec-ui-font")
                                                            .label("−")
                                                            .small()
                                                            .ghost()
                                                            .on_click(cx.listener(|this, _, w, cx| {
                                                                let new_size = (this.settings.ui_font_size - 1.0).max(12.0);
                                                                this.set_ui_font_size(new_size, w, cx);
                                                            })),
                                                    )
                                                    .child(
                                                        div()
                                                            .w(px(52.))
                                                            .text_center()
                                                            .text_sm()
                                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                                            .text_color(primary)
                                                            .child(format!("{:.0} px", ui_font_size)),
                                                    )
                                                    .child(
                                                        Button::new("inc-ui-font")
                                                            .label("+")
                                                            .small()
                                                            .ghost()
                                                            .on_click(cx.listener(|this, _, w, cx| {
                                                                let new_size = (this.settings.ui_font_size + 1.0).min(20.0);
                                                                this.set_ui_font_size(new_size, w, cx);
                                                            })),
                                                    ),
                                            ),
                                    )
                                    // Quick buttons cho UI font size
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .child(self.render_font_size_preset(13.0, "13px (Nhỏ)", ui_font_size, cx))
                                            .child(self.render_font_size_preset(14.0, "14px (Cũ)", ui_font_size, cx))
                                            .child(self.render_font_size_preset(15.0, "15px (Mặc định)", ui_font_size, cx))
                                            .child(self.render_font_size_preset(16.0, "16px (Rõ nét)", ui_font_size, cx))
                                            .child(self.render_font_size_preset(17.0, "17px (Lớn)", ui_font_size, cx)),
                                    )
                                    // Hộp xem trước cỡ chữ trực tiếp
                                    .child(
                                        div()
                                            .p_3()
                                            .rounded_lg()
                                            .bg(card)
                                            .border_1()
                                            .border_color(border)
                                            .child(
                                                div()
                                                    .text_color(fg)
                                                    .child(t!("settings.preview_text")),
                                            ),
                                    ),
                            )
                            // Section 3: Cỡ chữ Trình soạn thảo & Mật độ dòng
                            .child(
                                h_flex()
                                    .gap_4()
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                                    .text_color(fg)
                                                    .child(t!("settings.editor_font_size")),
                                            )
                                            .child(
                                                h_flex()
                                                    .gap_1()
                                                    .child(self.render_editor_size_preset(12.0, "12px", editor_font_size, cx))
                                                    .child(self.render_editor_size_preset(13.0, "13px", editor_font_size, cx))
                                                    .child(self.render_editor_size_preset(14.0, "14px", editor_font_size, cx))
                                                    .child(self.render_editor_size_preset(15.0, "15px", editor_font_size, cx))
                                                    .child(self.render_editor_size_preset(16.0, "16px", editor_font_size, cx)),
                                            ),
                                    )
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                                    .text_color(fg)
                                                    .child(t!("settings.section.density")),
                                            )
                                            .child(
                                                h_flex()
                                                    .gap_1()
                                                    .child(self.render_density_preset(22.0, t!("settings.row_height.compact"), row_height, cx))
                                                    .child(self.render_density_preset(26.0, t!("settings.row_height.standard"), row_height, cx))
                                                    .child(self.render_density_preset(30.0, t!("settings.row_height.relaxed"), row_height, cx)),
                                            ),
                                    ),
                            )
                            // Section 4: Font chữ Monospace
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(fg)
                                            .child(t!("settings.section.mono_font")),
                                    )
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .child(self.render_mono_font_btn(crate::ui::MONO, &mono_font, cx))
                                            .child(self.render_mono_font_btn("SF Mono", &mono_font, cx))
                                            .child(self.render_mono_font_btn("JetBrains Mono", &mono_font, cx))
                                            .child(self.render_mono_font_btn("Fira Code", &mono_font, cx)),
                                    ),
                            ),
                    )
                    // Footer của Modal
                    .child(
                        h_flex()
                            .h(px(50.))
                            .px_4()
                            .flex_none()
                            .border_t_1()
                            .border_color(border)
                            .justify_between()
                            .items_center()
                            .child(
                                Button::new("reset-settings")
                                    .label(t!("settings.reset_default"))
                                    .ghost()
                                    .small()
                                    .on_click(cx.listener(|this, _, w, cx| {
                                        this.reset_settings_to_default(w, cx);
                                    })),
                            )
                            .child(
                                Button::new("done-settings")
                                    .label(t!("settings.close"))
                                    .primary()
                                    .small()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.show_settings = false;
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// Thẻ hiển thị một mẫu giao diện (Theme card)
    fn render_theme_card(
        &self,
        p: ThemePreset,
        is_active: bool,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let t = cx.theme();
        let border = if is_active { t.primary } else { t.border };
        let active_bg = if is_active { t.primary.opacity(0.08) } else { t.secondary };

        let dot = |c: Hsla| {
            div()
                .size(px(14.))
                .rounded_full()
                .bg(c)
                .border_1()
                .border_color(t.border)
        };

        h_flex()
            .id(SharedString::from(format!("theme-card-{}", p.id)))
            .p_3()
            .gap_3()
            .rounded_lg()
            .border_1()
            .border_color(border)
            .bg(active_bg)
            .items_center()
            .hover(|s| s.bg(t.list_hover))
            .on_click(cx.listener(move |this, _, w, cx| {
                this.set_theme(p.id.to_string(), w, cx);
            }))
            .child(
                v_flex()
                    .gap_1()
                    .flex_none()
                    .child(
                        h_flex()
                            .gap_1()
                            .child(dot(p.bg))
                            .child(dot(p.card)),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .child(dot(p.primary))
                            .child(dot(p.fg)),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(t.foreground)
                                    .child(p.name),
                            )
                            .when(is_active, |d| {
                                d.child(
                                    div()
                                        .px_1()
                                        .py(px(2.))
                                        .rounded_full()
                                        .bg(t.primary)
                                        .text_color(t.primary_foreground)
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .child("✓"),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(t.muted_foreground)
                            .truncate()
                            .child(crate::i18n::tr(p.desc_key)),
                    ),
            )
    }

    fn render_font_size_preset(
        &self,
        size: f32,
        label: &'static str,
        current: f32,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let is_sel = (current - size).abs() < 0.2;
        Button::new(SharedString::from(format!("ui-font-preset-{}", size as i32)))
            .label(label)
            .small()
            .when(is_sel, |b| b.primary())
            .when(!is_sel, |b| b.ghost())
            .on_click(cx.listener(move |this, _, w, cx| {
                this.set_ui_font_size(size, w, cx);
            }))
    }

    fn render_editor_size_preset(
        &self,
        size: f32,
        label: &'static str,
        current: f32,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let is_sel = (current - size).abs() < 0.2;
        Button::new(SharedString::from(format!("ed-font-preset-{}", size as i32)))
            .label(label)
            .xsmall()
            .when(is_sel, |b| b.primary())
            .when(!is_sel, |b| b.ghost())
            .on_click(cx.listener(move |this, _, w, cx| {
                this.set_editor_font_size(size, w, cx);
            }))
    }

    fn render_density_preset(
        &self,
        height: f32,
        label: &'static str,
        current: f32,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let is_sel = (current - height).abs() < 0.2;
        Button::new(SharedString::from(format!("density-preset-{}", height as i32)))
            .label(label)
            .xsmall()
            .when(is_sel, |b| b.primary())
            .when(!is_sel, |b| b.ghost())
            .on_click(cx.listener(move |this, _, _, cx| {
                this.set_row_height(height, cx);
            }))
    }

    fn render_mono_font_btn(
        &self,
        name: &'static str,
        current: &str,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let is_sel = current == name;
        Button::new(SharedString::from(format!("mono-font-{}", name)))
            .label(name)
            .xsmall()
            .when(is_sel, |b| b.primary())
            .when(!is_sel, |b| b.ghost())
            .on_click(cx.listener(move |this, _, _, cx| {
                this.set_mono_font(name.to_string(), cx);
            }))
    }
}
