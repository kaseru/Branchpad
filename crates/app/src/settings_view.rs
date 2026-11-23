//! Màn hình cài đặt: chọn giao diện (theme), chỉnh font chữ, mật độ hiển thị, cấu hình AI.

use branchpad_agent::CliKind;
use gpui::{
    div, prelude::*, px, AnyElement, Context, Entity, Hsla, SharedString, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputState},
    v_flex, ActiveTheme, Disableable, Icon, Sizable,
};

use crate::icons::BpIcon;
use crate::ui::{get_theme_presets, ThemePreset};
use crate::repo_view::{agent_color, agent_icon};
use crate::workspace::{AgentCliInfo, Workspace};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SettingsTab {
    Appearance,
    Editor,
    Agent,
}

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

        let active_tab = self.settings_tab;

        // Modal container căn giữa với backdrop mờ
        div()
            .id("settings-modal-backdrop")
            .absolute()
            .inset_0()
            .bg(gpui::black().opacity(0.65))
            .flex()
            .items_center()
            .justify_center()
            .p_4()
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
                    .max_w_full()
                    .h(px(590.))
                    .max_h(px(620.))
                    .bg(card)
                    .border_1()
                    .border_color(border)
                    .rounded_xl()
                    .shadow_xl()
                    .overflow_hidden()
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    // Header của Modal
                    .child(
                        h_flex()
                            .h(px(52.))
                            .px_5()
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
                                            .p_1p5()
                                            .rounded_lg()
                                            .bg(primary.opacity(0.12))
                                            .text_color(primary)
                                            .child(Icon::new(BpIcon::Settings).size_4()),
                                    )
                                    .child(
                                        v_flex()
                                            .gap_0p5()
                                            .child(
                                                div()
                                                    .text_sm()
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
                    // Thanh Tab danh mục phía trên
                    .child(
                        h_flex()
                            .h(px(46.))
                            .px_5()
                            .gap_2()
                            .flex_none()
                            .border_b_1()
                            .border_color(border)
                            .bg(t.secondary.opacity(0.5))
                            .items_center()
                            .child(self.render_tab_button(
                                SettingsTab::Appearance,
                                Icon::new(BpIcon::Sparkles).size_3p5(),
                                t!("settings.tab.appearance"),
                                active_tab == SettingsTab::Appearance,
                                cx,
                            ))
                            .child(self.render_tab_button(
                                SettingsTab::Editor,
                                Icon::new(BpIcon::FileCode).size_3p5(),
                                t!("settings.tab.editor"),
                                active_tab == SettingsTab::Editor,
                                cx,
                            ))
                            .child(self.render_tab_button(
                                SettingsTab::Agent,
                                Icon::new(BpIcon::Settings).size_3p5(),
                                t!("settings.tab.agent"),
                                active_tab == SettingsTab::Agent,
                                cx,
                            )),
                    )
                    // Nội dung theo Tab đang chọn
                    .child(
                        v_flex()
                            .id("settings-modal-content")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .p_5()
                            .gap_4()
                            .child(match active_tab {
                                SettingsTab::Appearance => self.render_appearance_settings(cx),
                                SettingsTab::Editor => self.render_editor_settings(cx),
                                SettingsTab::Agent => self.render_agent_settings(cx),
                            }),
                    )
                    // Footer của Modal
                    .child(
                        h_flex()
                            .h(px(50.))
                            .px_5()
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

    /// Nút Tab trên thanh điều hướng
    fn render_tab_button(
        &self,
        tab: SettingsTab,
        icon: Icon,
        label: &'static str,
        is_active: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let t = cx.theme();
        let (bg, fg, border_color) = if is_active {
            (t.primary.opacity(0.15), t.primary, t.primary.opacity(0.4))
        } else {
            (gpui::transparent_black(), t.muted_foreground, gpui::transparent_black())
        };

        h_flex()
            .id(SharedString::from(format!("settings-tab-{tab:?}")))
            .px_3()
            .py_1p5()
            .gap_2()
            .items_center()
            .rounded_md()
            .bg(bg)
            .border_1()
            .border_color(border_color)
            .cursor_pointer()
            .hover(|s| if !is_active { s.bg(t.list_hover).text_color(t.foreground) } else { s })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.settings_tab = tab;
                cx.notify();
            }))
            .child(
                div()
                    .flex_none()
                    .text_color(fg)
                    .child(icon),
            )
            .child(
                div()
                    .text_xs()
                    .font_weight(if is_active { gpui::FontWeight::SEMIBOLD } else { gpui::FontWeight::MEDIUM })
                    .text_color(fg)
                    .child(label),
            )
            .into_any_element()
    }

    /// Tab 1: Giao diện & Màu sắc
    fn render_appearance_settings(&self, cx: &Context<Self>) -> AnyElement {
        let t = cx.theme();
        let fg = t.foreground;
        let muted = t.muted_foreground;
        let primary = t.primary;
        let border = t.border;
        let card = t.secondary;

        let current_theme = self.settings.theme.clone();
        let ui_font_size = self.settings.ui_font_size;
        let presets = get_theme_presets();

        v_flex()
            .gap_4()
            // Khối 1: Chọn Themes
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(fg)
                                    .child(t!("settings.section.appearance")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(t!("settings.appearance.desc")),
                            ),
                    )
                    .children(presets.chunks(2).map(|chunk| {
                        h_flex()
                            .gap_3()
                            .w_full()
                            .child(
                                div()
                                    .flex_1()
                                    .child(self.render_theme_card(chunk[0], chunk[0].id == current_theme, cx)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .when(chunk.len() > 1, |d| {
                                        d.child(self.render_theme_card(chunk[1], chunk[1].id == current_theme, cx))
                                    }),
                            )
                    })),
            )
            // Khối 2: Cỡ chữ Giao diện UI
            .child(
                v_flex()
                    .gap_3()
                    .p_4()
                    .rounded_xl()
                    .border_1()
                    .border_color(border)
                    .bg(t.background)
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .child(
                                v_flex()
                                    .gap_0p5()
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
                                            .w(px(56.))
                                            .text_center()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::BOLD)
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
                    // Xem trước cỡ chữ
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
            .into_any_element()
    }

    /// Tab 2: Trình soạn thảo & Font chữ
    fn render_editor_settings(&self, cx: &Context<Self>) -> AnyElement {
        let t = cx.theme();
        let fg = t.foreground;
        let muted = t.muted_foreground;
        let border = t.border;
        let editor_font_size = self.settings.editor_font_size;
        let auto_save = self.settings.auto_save;
        let row_height = self.settings.row_height;
        let mono_font = self.settings.mono_font.clone();

        v_flex()
            .gap_4()
            // Khối 1: Font chữ Monospace
            .child(
                v_flex()
                    .gap_3()
                    .p_4()
                    .rounded_xl()
                    .border_1()
                    .border_color(border)
                    .bg(t.background)
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(fg)
                                    .child(t!("settings.section.mono_font")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(t!("settings.editor.desc")),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(self.render_mono_font_btn(crate::ui::MONO, &mono_font, cx))
                            .child(self.render_mono_font_btn("SF Mono", &mono_font, cx))
                            .child(self.render_mono_font_btn("JetBrains Mono", &mono_font, cx))
                            .child(self.render_mono_font_btn("Fira Code", &mono_font, cx)),
                    )
                    .child(
                        div()
                            .p_3()
                            .rounded_lg()
                            .bg(t.secondary)
                            .border_1()
                            .border_color(border)
                            .text_xs()
                            .font_family(SharedString::from(mono_font.clone()))
                            .text_color(fg)
                            .child("fn main() { println!(\"Branchpad: Code Editor & Git Client\"); }"),
                    ),
            )
            // Khối 2: Cỡ chữ Trình soạn thảo & Mật độ dòng
            .child(
                h_flex()
                    .gap_4()
                    .child(
                        v_flex()
                            .flex_1()
                            .gap_3()
                            .p_4()
                            .rounded_xl()
                            .border_1()
                            .border_color(border)
                            .bg(t.background)
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
                            .gap_3()
                            .p_4()
                            .rounded_xl()
                            .border_1()
                            .border_color(border)
                            .bg(t.background)
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
            // Khối 3: Tự lưu file
            .child(
                v_flex()
                    .gap_2()
                    .p_4()
                    .rounded_xl()
                    .border_1()
                    .border_color(border)
                    .bg(t.background)
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .child(
                                v_flex()
                                    .gap_0p5()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(fg)
                                            .child(t!("settings.section.auto_save")),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(muted)
                                            .child(t!("settings.auto_save.on")),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .gap_1()
                                    .child(
                                        Button::new("auto-save-off")
                                            .label(t!("settings.auto_save.off"))
                                            .xsmall()
                                            .when(!auto_save, |b| b.primary())
                                            .when(auto_save, |b| b.ghost())
                                            .on_click(cx.listener(|this, _, _, cx| this.set_auto_save(false, cx))),
                                    )
                                    .child(
                                        Button::new("auto-save-on")
                                            .label("Bật")
                                            .xsmall()
                                            .when(auto_save, |b| b.primary())
                                            .when(!auto_save, |b| b.ghost())
                                            .on_click(cx.listener(|this, _, _, cx| this.set_auto_save(true, cx))),
                                    ),
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
        let active_bg = if is_active { t.primary.opacity(0.08) } else { t.background };

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
            .rounded_xl()
            .border_1()
            .border_color(border)
            .bg(active_bg)
            .items_center()
            .cursor_pointer()
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
                    .min_w_0()
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
                                        .px_1p5()
                                        .py(px(1.))
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

    /// Mục AI: CLI mặc định, đường dẫn + phiên bản tìm thấy, model, worktree, quyền chạy lệnh.
    fn render_agent_settings(&self, cx: &Context<Self>) -> AnyElement {
        let t = cx.theme();
        let (fg, muted, success) = (t.foreground, t.muted_foreground, t.success);
        let border = t.border;
        let current = CliKind::from_id(&self.settings.agent_cli);
        let cli_button = |id: &'static str, label: &'static str, kind: Option<CliKind>| {
            let selected = current == kind;
            Button::new(id)
                .when_some(kind, |b, k| b.icon(agent_icon(k).text_color(agent_color(k, cx))))
                .label(label)
                .xsmall()
                .when(selected, |b| b.primary())
                .when(!selected, |b| b.ghost())
                .on_click(cx.listener(move |this, _, _, cx| this.set_agent_cli(kind, cx)))
        };
        let cli_block = |kind: CliKind, path: &Entity<InputState>, model: &Entity<InputState>| {
            let info = self.agent_found.iter().find(|i| i.kind == kind);
            let (status, found) = match info {
                None => (t!("settings.agent.checking").to_string(), false),
                Some(AgentCliInfo { path: Some(p), version, .. }) => (
                    t!("settings.agent.found", path = p.display(), version = version.as_deref().unwrap_or("?")),
                    true,
                ),
                Some(_) => (t!("settings.agent.not_found").to_string(), false),
            };
            v_flex()
                .gap_1p5()
                .p_3()
                .rounded_lg()
                .border_1()
                .border_color(border)
                .bg(t.secondary.opacity(0.35))
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(Icon::new(agent_icon(kind)).size_4().text_color(agent_color(kind, cx)))
                        .child(div().flex_none().text_sm().font_weight(gpui::FontWeight::SEMIBOLD).text_color(fg).child(kind.label()))
                        .child(div().flex_1().truncate().text_xs().text_color(if found { success } else { muted }).child(status)),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(div().flex_1().child(Input::new(path).small()))
                        .child(div().w(px(220.)).flex_none().child(Input::new(model).small())),
                )
        };
        let toggle_row = |title: &'static str, desc: &'static str, toggle: AnyElement| {
            h_flex()
                .gap_3()
                .items_center()
                .p_3()
                .rounded_lg()
                .border_1()
                .border_color(border)
                .bg(t.secondary.opacity(0.35))
                .child(
                    v_flex()
                        .flex_1()
                        .gap_1()
                        .child(div().text_sm().font_weight(gpui::FontWeight::MEDIUM).text_color(fg).child(title))
                        .child(div().text_xs().text_color(muted).whitespace_normal().child(desc)),
                )
                .child(toggle)
        };
        let inputs = &self.agent_inputs;
        v_flex()
            .gap_4()
            // Khối 1: Chọn CLI mặc định
            .child(
                v_flex()
                    .gap_3()
                    .p_4()
                    .rounded_xl()
                    .border_1()
                    .border_color(border)
                    .bg(t.background)
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(fg)
                                    .child(t!("settings.agent.default_cli")),
                            )
                            .child(
                                Button::new("agent-recheck")
                                    .label(t!("settings.agent.recheck"))
                                    .xsmall()
                                    .ghost()
                                    .loading(self.agent_checking)
                                    .on_click(cx.listener(|this, _, _, cx| this.check_agent_clis(cx))),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .child(cli_button("agent-cli-auto", t!("settings.agent.auto"), None))
                            .child(cli_button("agent-cli-claude", CliKind::Claude.label(), Some(CliKind::Claude)))
                            .child(cli_button("agent-cli-codex", CliKind::Codex.label(), Some(CliKind::Codex)))
                            .child(cli_button("agent-cli-agy", CliKind::Agy.label(), Some(CliKind::Agy))),
                    )
                    .child(div().text_xs().text_color(muted).child(t!("settings.agent.inputs_hint"))),
            )
            // Khối 2: Cấu hình từng CLI
            .child(
                v_flex()
                    .gap_3()
                    .p_4()
                    .rounded_xl()
                    .border_1()
                    .border_color(border)
                    .bg(t.background)
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(fg)
                            .child("Cấu hình đường dẫn & Model"),
                    )
                    .child(cli_block(CliKind::Claude, &inputs.claude_path, &inputs.claude_model))
                    .child(self.render_claude_token(cx))
                    .child(cli_block(CliKind::Codex, &inputs.codex_path, &inputs.codex_model))
                    .child(cli_block(CliKind::Agy, &inputs.agy_path, &inputs.agy_model)),
            )
            // Khối 3: Quyền thực thi & Môi trường
            .child(
                v_flex()
                    .gap_3()
                    .p_4()
                    .rounded_xl()
                    .border_1()
                    .border_color(border)
                    .bg(t.background)
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(fg)
                            .child("Môi trường & Quyền thực thi"),
                    )
                    .child(toggle_row(
                        t!("settings.agent.worktree"),
                        t!("settings.agent.worktree_desc"),
                        self.render_on_off("agent-worktree", self.settings.agent_use_worktree, Self::set_agent_worktree, cx).into_any_element(),
                    ))
                    .child(toggle_row(
                        t!("settings.agent.shell"),
                        t!("settings.agent.shell_desc"),
                        self.render_on_off("agent-shell", self.settings.agent_allow_shell, Self::set_agent_allow_shell, cx).into_any_element(),
                    )),
            )
            .into_any_element()
    }

    /// Token `CLAUDE_CODE_OAUTH_TOKEN` (từ `claude setup-token`): dán vào, lưu trong Keychain.
    fn render_claude_token(&self, cx: &Context<Self>) -> impl IntoElement {
        let t = cx.theme();
        let (fg, muted, success, danger) = (t.foreground, t.muted_foreground, t.success, t.danger);
        let border = t.border;
        let saved = self.claude_token.is_some();
        let (status, color) = if !self.token_loaded {
            (t!("settings.agent.checking").to_string(), muted)
        } else if saved {
            (t!("settings.agent.token_saved").to_string(), success)
        } else {
            (t!("settings.agent.token_none").to_string(), muted)
        };
        v_flex()
            .gap_1p5()
            .p_3()
            .rounded_lg()
            .border_1()
            .border_color(border)
            .bg(t.secondary.opacity(0.35))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().flex_none().text_xs().font_weight(gpui::FontWeight::SEMIBOLD).text_color(fg).child("CLAUDE_CODE_OAUTH_TOKEN"))
                    .child(div().flex_1().truncate().text_xs().text_color(color).child(status)),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().child(Input::new(&self.agent_inputs.claude_token).small()))
                    .child(
                        Button::new("claude-token-save")
                            .label(t!("settings.agent.token_save"))
                            .xsmall()
                            .primary()
                            .loading(self.token_busy)
                            .disabled(self.token_busy)
                            .on_click(cx.listener(|this, _, window, cx| this.save_token_to_keychain(window, cx))),
                    )
                    .when(saved, |d| {
                        d.child(
                            Button::new("claude-token-delete")
                                .label(t!("settings.agent.token_delete"))
                                .xsmall()
                                .ghost()
                                .disabled(self.token_busy)
                                .on_click(cx.listener(|this, _, _, cx| this.delete_token_from_keychain(cx))),
                        )
                    }),
            )
            .child(div().text_xs().text_color(muted).whitespace_normal().child(t!("settings.agent.token_hint")))
            .when_some(self.token_error.clone(), |d, e| d.child(div().text_xs().text_color(danger).whitespace_normal().child(e)))
    }

    /// Cặp nút Tắt / Bật (giống mục Tự lưu file).
    fn render_on_off(
        &self,
        id: &'static str,
        value: bool,
        set: fn(&mut Self, bool, &mut Context<Self>),
        cx: &Context<Self>,
    ) -> impl IntoElement {
        h_flex()
            .gap_1()
            .flex_none()
            .child(
                Button::new((id, 0usize))
                    .label(t!("settings.auto_save.off"))
                    .xsmall()
                    .when(!value, |b| b.primary())
                    .when(value, |b| b.ghost())
                    .on_click(cx.listener(move |this, _, _, cx| set(this, false, cx))),
            )
            .child(
                Button::new((id, 1usize))
                    .label("Bật")
                    .xsmall()
                    .when(value, |b| b.primary())
                    .when(!value, |b| b.ghost())
                    .on_click(cx.listener(move |this, _, _, cx| set(this, true, cx))),
            )
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
