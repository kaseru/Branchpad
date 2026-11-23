//! Màn hình cài đặt: chọn giao diện (theme), chỉnh font chữ, mật độ hiển thị, cấu hình AI.
//! Thiết kế chuẩn macOS Preferences Split-View (Raycast / Apple Settings).

use branchpad_agent::CliKind;
use gpui::{
    div, hsla, prelude::*, px, AnyElement, Context, Entity, SharedString, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputState},
    v_flex, ActiveTheme, Disableable, Icon, Sizable,
};

use crate::icons::BpIcon;
use crate::repo_view::{agent_color, agent_icon};
use crate::ui::{get_theme_presets, ThemePreset};
use crate::workspace::{AgentCliInfo, Workspace};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SettingsTab {
    Appearance,
    Editor,
    Agent,
}

impl Workspace {
    /// Vẽ hộp thoại Modal Cài đặt theo chuẩn Apple macOS Preferences Split-View.
    pub(crate) fn render_settings_modal(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = cx.theme();
        let border = t.border;
        let active_tab = self.settings_tab;

        // Modal backdrop mờ tối tinh tế
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
                // Cửa sổ Split View chính: rộng 860px, cao 580px
                h_flex()
                    .id("settings-modal-dialog")
                    .w(px(860.))
                    .max_w_full()
                    .h(px(580.))
                    .max_h(px(640.))
                    .bg(t.background)
                    .border_1()
                    .border_color(border.opacity(0.7))
                    .rounded_2xl()
                    .shadow_2xl()
                    .overflow_hidden()
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    // ==========================================
                    // 1. SIDEBAR BÊN TRÁI
                    // ==========================================
                    .child(
                        v_flex()
                            .w(px(210.))
                            .flex_none()
                            .h_full()
                            .bg(t.secondary.opacity(0.45))
                            .border_r_1()
                            .border_color(border.opacity(0.5))
                            .justify_between()
                            // Phần trên Sidebar: Header + Nav list
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_h_0()
                                    .p_3()
                                    .gap_3()
                                    // Header Sidebar
                                    .child(
                                        h_flex()
                                            .px_2()
                                            .py_1p5()
                                            .gap_2p5()
                                            .items_center()
                                            .child(
                                                div()
                                                    .size(px(24.))
                                                    .rounded_md()
                                                    .bg(t.primary.opacity(0.15))
                                                    .text_color(t.primary)
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .child(Icon::new(BpIcon::Settings).size_3p5()),
                                            )
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_weight(gpui::FontWeight::BOLD)
                                                    .text_color(t.foreground)
                                                    .child(t!("settings.title")),
                                            ),
                                    )
                                    .child(div().h(px(1.)).bg(border.opacity(0.3)).mx_1())
                                    // Danh sách Tab Sidebar chuẩn Apple
                                    .child(
                                        v_flex()
                                            .gap_1()
                                            .child(self.render_sidebar_nav_item(
                                                SettingsTab::Appearance,
                                                Icon::new(BpIcon::Sparkles).size_4(),
                                                t!("settings.tab.appearance"),
                                                active_tab == SettingsTab::Appearance,
                                                cx,
                                            ))
                                            .child(self.render_sidebar_nav_item(
                                                SettingsTab::Editor,
                                                Icon::new(BpIcon::FileCode).size_4(),
                                                t!("settings.tab.editor"),
                                                active_tab == SettingsTab::Editor,
                                                cx,
                                            ))
                                            .child(self.render_sidebar_nav_item(
                                                SettingsTab::Agent,
                                                Icon::new(BpIcon::Claude).size_4(),
                                                t!("settings.tab.agent"),
                                                active_tab == SettingsTab::Agent,
                                                cx,
                                            )),
                                    ),
                            )
                            // Phần dưới Sidebar: Reset to Defaults
                            .child(
                                v_flex()
                                    .p_3()
                                    .border_t_1()
                                    .border_color(border.opacity(0.35))
                                    .child(
                                        Button::new("reset-settings-sidebar")
                                            .label(t!("settings.reset_default"))
                                            .ghost()
                                            .xsmall()
                                            .w_full()
                                            .on_click(cx.listener(|this, _, w, cx| {
                                                this.reset_settings_to_default(w, cx);
                                            })),
                                    ),
                            ),
                    )
                    // ==========================================
                    // 2. MAIN CONTENT BÊN PHẢI
                    // ==========================================
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .bg(t.background)
                            // Content Header
                            .child(
                                h_flex()
                                    .h(px(54.))
                                    .px_6()
                                    .flex_none()
                                    .border_b_1()
                                    .border_color(border.opacity(0.4))
                                    .justify_between()
                                    .items_center()
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w_0()
                                            .gap_0()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_weight(gpui::FontWeight::BOLD)
                                                    .text_color(t.foreground)
                                                    .child(match active_tab {
                                                        SettingsTab::Appearance => t!("settings.tab.appearance"),
                                                        SettingsTab::Editor => t!("settings.tab.editor"),
                                                        SettingsTab::Agent => t!("settings.tab.agent"),
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(t.muted_foreground)
                                                    .truncate()
                                                    .child(match active_tab {
                                                        SettingsTab::Appearance => t!("settings.appearance.desc"),
                                                        SettingsTab::Editor => t!("settings.editor.desc"),
                                                        SettingsTab::Agent => t!("settings.agent.desc"),
                                                    }),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex_none()
                                            .child(
                                                Button::new("close-settings-button")
                                                    .label("✕")
                                                    .ghost()
                                                    .small()
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.show_settings = false;
                                                        cx.notify();
                                                    })),
                                            ),
                                    ),
                            )
                            // Content Body: Cuộn mượt mà
                            .child(
                                v_flex()
                                    .id("settings-body-scroll")
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_y_scroll()
                                    .px_6()
                                    .py_5()
                                    .gap_6()
                                    .child(match active_tab {
                                        SettingsTab::Appearance => self.render_appearance_settings(cx),
                                        SettingsTab::Editor => self.render_editor_settings(cx),
                                        SettingsTab::Agent => self.render_agent_settings(cx),
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// Mục điều hướng Sidebar chuẩn Apple Settings
    fn render_sidebar_nav_item(
        &self,
        tab: SettingsTab,
        icon: Icon,
        label: &'static str,
        is_active: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let t = cx.theme();
        let (bg, text_color, icon_color) = if is_active {
            (
                t.primary.opacity(0.12),
                t.primary,
                t.primary,
            )
        } else {
            (
                gpui::transparent_black(),
                t.foreground,
                t.muted_foreground,
            )
        };

        h_flex()
            .id(SharedString::from(format!("nav-tab-{tab:?}")))
            .px_3()
            .py_2()
            .gap_2p5()
            .items_center()
            .rounded_lg()
            .bg(bg)
            .cursor_pointer()
            .hover(|s| {
                if !is_active {
                    s.bg(t.secondary.opacity(0.5))
                } else {
                    s
                }
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.settings_tab = tab;
                if tab == SettingsTab::Agent && this.agent_found.is_empty() && !this.agent_checking {
                    this.check_agent_clis(cx);
                }
                cx.notify();
            }))
            .child(
                div()
                    .flex_none()
                    .text_color(icon_color)
                    .child(icon),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .font_weight(if is_active {
                        gpui::FontWeight::SEMIBOLD
                    } else {
                        gpui::FontWeight::MEDIUM
                    })
                    .text_color(text_color)
                    .truncate()
                    .child(label),
            )
            .into_any_element()
    }

    /// Tiêu đề nhóm cài đặt mỏng nhẹ, tinh tế
    fn render_group_heading(&self, title: &'static str, cx: &Context<Self>) -> AnyElement {
        div()
            .text_xs()
            .font_weight(gpui::FontWeight::BOLD)
            .text_color(cx.theme().muted_foreground)
            .child(title)
            .into_any_element()
    }

    /// Đường kẻ mảnh tinh tế giữa các nhóm
    fn render_divider(&self, cx: &Context<Self>) -> AnyElement {
        div()
            .h(px(1.))
            .bg(cx.theme().border.opacity(0.35))
            .w_full()
            .into_any_element()
    }

    // =========================================================================
    // TAB 1: GIAO DIỆN & CHỦ ĐỀ (APPEARANCE)
    // =========================================================================
    fn render_appearance_settings(&self, cx: &Context<Self>) -> AnyElement {
        let t = cx.theme();
        let fg = t.foreground;
        let muted = t.muted_foreground;
        let current_theme = self.settings.theme.clone();
        let ui_font_size = self.settings.ui_font_size;
        let presets = get_theme_presets();

        v_flex()
            .gap_5()
            // 1. Theme Swatches Grid
            .child(
                v_flex()
                    .gap_3()
                    .child(self.render_group_heading(t!("settings.section.appearance"), cx))
                    .children(presets.chunks(3).map(|chunk| {
                        h_flex()
                            .gap_3()
                            .w_full()
                            .children(chunk.iter().map(|preset| {
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(self.render_theme_compact_card(*preset, preset.id == current_theme, cx))
                            }))
                    })),
            )
            .child(self.render_divider(cx))
            // 2. UI Typography
            .child(
                v_flex()
                    .gap_3()
                    .child(self.render_group_heading(t!("settings.section.typography"), cx))
                    .child(
                        h_flex()
                            .w_full()
                            .justify_between()
                            .items_center()
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .pr_4()
                                    .gap_0p5()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(fg)
                                            .child(t!("settings.ui_font_size")),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(muted)
                                            .whitespace_normal()
                                            .child(t!("settings.ui_font_desc")),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .child(self.render_stepper_pill(
                                        ui_font_size,
                                        12.0,
                                        20.0,
                                        "px",
                                        |this, val, w, cx| this.set_ui_font_size(val, w, cx),
                                        cx,
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .px_3p5()
                            .py_2p5()
                            .rounded_xl()
                            .bg(t.secondary.opacity(0.35))
                            .border_1()
                            .border_color(t.border.opacity(0.3))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(fg)
                                    .child(t!("settings.preview_text")),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// Thẻ Theme thu nhỏ (Compact Theme Swatch)
    fn render_theme_compact_card(
        &self,
        p: ThemePreset,
        is_active: bool,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let t = cx.theme();
        let border_color = if is_active {
            t.primary
        } else {
            t.border.opacity(0.5)
        };

        v_flex()
            .id(SharedString::from(format!("theme-card-{}", p.id)))
            .rounded_xl()
            .border_2()
            .border_color(border_color)
            .when(is_active, |d| d.shadow_sm())
            .bg(t.background)
            .overflow_hidden()
            .cursor_pointer()
            .hover(|s| {
                if !is_active {
                    s.border_color(t.primary.opacity(0.5))
                } else {
                    s
                }
            })
            .on_click(cx.listener(move |this, _, w, cx| {
                this.set_theme(p.id.to_string(), w, cx);
            }))
            // Mockup IDE thu nhỏ
            .child(
                v_flex()
                    .h(px(50.))
                    .bg(p.bg)
                    .child(
                        h_flex()
                            .h(px(14.))
                            .px_1p5()
                            .gap_1()
                            .items_center()
                            .bg(p.tab_bar)
                            .child(div().size(px(3.5)).rounded_full().bg(hsla(0.0, 0.8, 0.6, 1.)))
                            .child(div().size(px(3.5)).rounded_full().bg(hsla(0.12, 0.8, 0.6, 1.)))
                            .child(div().size(px(3.5)).rounded_full().bg(hsla(0.35, 0.8, 0.5, 1.))),
                    )
                    .child(
                        h_flex()
                            .flex_1()
                            .child(
                                div()
                                    .w(px(26.))
                                    .h_full()
                                    .bg(p.tab_active)
                                    .border_r_1()
                                    .border_color(p.border),
                            )
                            .child(
                                v_flex()
                                    .flex_1()
                                    .p_1()
                                    .gap_1()
                                    .child(div().w(px(42.)).h(px(2.5)).rounded_full().bg(p.primary))
                                    .child(div().w(px(64.)).h(px(2.5)).rounded_full().bg(p.fg.opacity(0.35)))
                                    .child(div().w(px(32.)).h(px(2.5)).rounded_full().bg(p.fg.opacity(0.2))),
                            ),
                    ),
            )
            // Tên Theme + Dấu tích
            .child(
                h_flex()
                    .h(px(30.))
                    .px_2p5()
                    .justify_between()
                    .items_center()
                    .border_t_1()
                    .border_color(t.border.opacity(0.4))
                    .bg(if is_active {
                        t.primary.opacity(0.1)
                    } else {
                        t.secondary.opacity(0.3)
                    })
                    .child(
                        div()
                            .text_xs()
                            .font_weight(if is_active {
                                gpui::FontWeight::SEMIBOLD
                            } else {
                                gpui::FontWeight::MEDIUM
                            })
                            .text_color(if is_active {
                                t.primary
                            } else {
                                t.foreground
                            })
                            .truncate()
                            .child(p.name),
                    )
                    .when(is_active, |d| {
                        d.child(
                            div()
                                .size(px(14.))
                                .rounded_full()
                                .bg(t.primary)
                                .text_color(t.primary_foreground)
                                .text_xs()
                                .font_weight(gpui::FontWeight::BOLD)
                                .flex()
                                .items_center()
                                .justify_center()
                                .child("✓"),
                        )
                    }),
            )
    }

    // =========================================================================
    // TAB 2: TRÌNH SOẠN THẢO & PHÔNG CHỮ (EDITOR & FONTS)
    // =========================================================================
    fn render_editor_settings(&self, cx: &Context<Self>) -> AnyElement {
        let t = cx.theme();
        let fg = t.foreground;
        let muted = t.muted_foreground;
        let editor_font_size = self.settings.editor_font_size;
        let auto_save = self.settings.auto_save;
        let row_height = self.settings.row_height;
        let mono_font = self.settings.mono_font.clone();

        v_flex()
            .gap_5()
            // 1. Phông chữ Monospace & Cỡ chữ
            .child(
                v_flex()
                    .gap_3()
                    .child(self.render_group_heading(t!("settings.section.editor_font"), cx))
                    // Monospace Font
                    .child(
                        h_flex()
                            .w_full()
                            .justify_between()
                            .items_center()
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .pr_4()
                                    .gap_0p5()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(fg)
                                            .child(t!("settings.section.mono_font")),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(muted)
                                            .whitespace_normal()
                                            .child("Code Editor, Git Diff, Graph and Blame"),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .child(
                                        h_flex()
                                            .p(px(2.5))
                                            .gap_1()
                                            .rounded_lg()
                                            .bg(t.secondary.opacity(0.6))
                                            .border_1()
                                            .border_color(t.border.opacity(0.5))
                                            .child(self.render_mono_segmented_item(crate::ui::MONO, "System", &mono_font, cx))
                                            .child(self.render_mono_segmented_item("SF Mono", "SF Mono", &mono_font, cx))
                                            .child(self.render_mono_segmented_item("JetBrains Mono", "JetBrains", &mono_font, cx))
                                            .child(self.render_mono_segmented_item("Fira Code", "Fira Code", &mono_font, cx)),
                                    ),
                            ),
                    )
                    // Code Preview Box
                    .child(
                        div()
                            .p_3()
                            .rounded_xl()
                            .bg(t.secondary.opacity(0.4))
                            .border_1()
                            .border_color(t.border.opacity(0.3))
                            .text_xs()
                            .font_family(SharedString::from(mono_font.clone()))
                            .text_color(fg)
                            .child("fn main() {\n    let client = \"Branchpad\"; // Native Git & Code Editor\n    println!(\"Running with {client}\");\n}"),
                    )
                    // Editor Font Size
                    .child(
                        h_flex()
                            .w_full()
                            .justify_between()
                            .items_center()
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .pr_4()
                                    .gap_0p5()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(fg)
                                            .child(t!("settings.editor_font_size")),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(muted)
                                            .whitespace_normal()
                                            .child(t!("settings.editor_font_desc")),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .child(self.render_stepper_pill(
                                        editor_font_size,
                                        11.0,
                                        22.0,
                                        "px",
                                        |this, val, w, cx| this.set_editor_font_size(val, w, cx),
                                        cx,
                                    )),
                            ),
                    )
                    // Row Density
                    .child(
                        h_flex()
                            .w_full()
                            .justify_between()
                            .items_center()
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .pr_4()
                                    .gap_0p5()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(fg)
                                            .child(t!("settings.section.density")),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(muted)
                                            .whitespace_normal()
                                            .child("Line height in repository tree and commit lists"),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .child(
                                        h_flex()
                                            .p(px(2.5))
                                            .gap_1()
                                            .rounded_lg()
                                            .bg(t.secondary.opacity(0.6))
                                            .border_1()
                                            .border_color(t.border.opacity(0.5))
                                            .child(self.render_density_segmented_item(22.0, "Compact", row_height, cx))
                                            .child(self.render_density_segmented_item(26.0, "Standard", row_height, cx))
                                            .child(self.render_density_segmented_item(30.0, "Relaxed", row_height, cx)),
                                    ),
                            ),
                    ),
            )
            .child(self.render_divider(cx))
            // 2. Editor Behavior
            .child(
                v_flex()
                    .gap_3()
                    .child(self.render_group_heading(t!("settings.section.editor_behavior"), cx))
                    .child(
                        h_flex()
                            .w_full()
                            .justify_between()
                            .items_center()
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .pr_4()
                                    .gap_0p5()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(fg)
                                            .child(t!("settings.section.auto_save")),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(muted)
                                            .whitespace_normal()
                                            .child("Automatically save file 1 second after typing stops"),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .child(self.render_toggle_pill("auto-save", auto_save, Self::set_auto_save, cx)),
                            ),
                    ),
            )
            .into_any_element()
    }

    // =========================================================================
    // TAB 3: CẤU HÌNH TRỢ LÝ AI (AGENT)
    // =========================================================================
    fn render_agent_settings(&self, cx: &Context<Self>) -> AnyElement {
        let t = cx.theme();
        let (fg, muted, success) = (t.foreground, t.muted_foreground, t.success);
        let current = CliKind::from_id(&self.settings.agent_cli);

        // Helper render mỗi CLI card gọn gàng
        let cli_card = |kind: CliKind, path: &Entity<InputState>, model: &Entity<InputState>, token_widget: Option<AnyElement>, cx: &Context<Self>| {
            let info = self.agent_found.iter().find(|i| i.kind == kind);
            let (status_text, found) = match info {
                None => {
                    if self.agent_checking {
                        (t!("settings.agent.checking").to_string(), false)
                    } else {
                        (t!("settings.agent.not_found").to_string(), false)
                    }
                }
                Some(AgentCliInfo { path: Some(_), version, .. }) => (
                    t!("settings.agent.found", version = version.as_deref().unwrap_or("?")),
                    true,
                ),
                Some(_) => (t!("settings.agent.not_found").to_string(), false),
            };

            v_flex()
                .p_3()
                .gap_2p5()
                .rounded_xl()
                .bg(t.secondary.opacity(0.35))
                .border_1()
                .border_color(t.border.opacity(0.4))
                .child(
                    h_flex()
                        .justify_between()
                        .items_center()
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(
                                    div()
                                        .size(px(22.))
                                        .rounded_md()
                                        .bg(agent_color(kind, cx).opacity(0.15))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(Icon::new(agent_icon(kind)).size_3p5().text_color(agent_color(kind, cx))),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(fg)
                                        .child(kind.label()),
                                ),
                        )
                        .child(
                            div()
                                .px_2()
                                .py_0p5()
                                .rounded_full()
                                .bg(if found { success.opacity(0.12) } else { muted.opacity(0.12) })
                                .text_xs()
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(if found { success } else { muted })
                                .child(status_text),
                        ),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(Input::new(path).small()),
                        )
                        .child(
                            div()
                                .w(px(210.))
                                .flex_none()
                                .child(Input::new(model).small()),
                        ),
                )
                .when_some(token_widget, |d, w| d.child(w))
        };

        let inputs = &self.agent_inputs;

        v_flex()
            .gap_5()
            // 1. Default Assistant Picker
            .child(
                v_flex()
                    .gap_3()
                    .child(self.render_group_heading(t!("settings.section.agent_default"), cx))
                    .child(
                        h_flex()
                            .w_full()
                            .justify_between()
                            .items_center()
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .pr_4()
                                    .gap_0p5()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(fg)
                                            .child(t!("settings.agent.default_cli")),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(muted)
                                            .whitespace_normal()
                                            .child(t!("settings.agent.default_cli_desc")),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .child(
                                        h_flex()
                                            .p(px(2.5))
                                            .gap_1()
                                            .rounded_lg()
                                            .bg(t.secondary.opacity(0.6))
                                            .border_1()
                                            .border_color(t.border.opacity(0.5))
                                            .child(self.render_cli_segmented_item(None, t!("settings.agent.auto"), current == None, cx))
                                            .child(self.render_cli_segmented_item(Some(CliKind::Claude), "Claude", current == Some(CliKind::Claude), cx))
                                            .child(self.render_cli_segmented_item(Some(CliKind::Codex), "Codex", current == Some(CliKind::Codex), cx))
                                            .child(self.render_cli_segmented_item(Some(CliKind::Agy), "Antigravity", current == Some(CliKind::Agy), cx)),
                                    ),
                            ),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .pr_4()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(t!("settings.agent.recheck_desc")),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .child(
                                        Button::new("agent-recheck")
                                            .label(t!("settings.agent.recheck"))
                                            .small()
                                            .ghost()
                                            .loading(self.agent_checking)
                                            .on_click(cx.listener(|this, _, _, cx| this.check_agent_clis(cx))),
                                    ),
                            ),
                    ),
            )
            .child(self.render_divider(cx))
            // 2. Installed Integrations
            .child(
                v_flex()
                    .gap_3()
                    .child(self.render_group_heading(t!("settings.section.agent_clis"), cx))
                    .child(cli_card(
                        CliKind::Claude,
                        &inputs.claude_path,
                        &inputs.claude_model,
                        Some(self.render_claude_inline_token(cx)),
                        cx,
                    ))
                    .child(cli_card(
                        CliKind::Codex,
                        &inputs.codex_path,
                        &inputs.codex_model,
                        None,
                        cx,
                    ))
                    .child(cli_card(
                        CliKind::Agy,
                        &inputs.agy_path,
                        &inputs.agy_model,
                        None,
                        cx,
                    )),
            )
            .child(self.render_divider(cx))
            // 3. Environment & Permissions
            .child(
                v_flex()
                    .gap_3()
                    .child(self.render_group_heading(t!("settings.section.agent_permissions"), cx))
                    .child(
                        h_flex()
                            .w_full()
                            .justify_between()
                            .items_center()
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .pr_4()
                                    .gap_0p5()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(fg)
                                            .child(t!("settings.agent.worktree")),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(muted)
                                            .whitespace_normal()
                                            .child(t!("settings.agent.worktree_desc")),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .child(self.render_toggle_pill(
                                        "agent-worktree",
                                        self.settings.agent_use_worktree,
                                        Self::set_agent_worktree,
                                        cx,
                                    )),
                            ),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .justify_between()
                            .items_center()
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .pr_4()
                                    .gap_0p5()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(fg)
                                            .child(t!("settings.agent.shell")),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(muted)
                                            .whitespace_normal()
                                            .child(t!("settings.agent.shell_desc")),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .child(self.render_toggle_pill(
                                        "agent-shell",
                                        self.settings.agent_allow_shell,
                                        Self::set_agent_allow_shell,
                                        cx,
                                    )),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// Thẻ nhập Token Claude tích hợp gọn bên trong Card Claude Code
    fn render_claude_inline_token(&self, cx: &Context<Self>) -> AnyElement {
        let t = cx.theme();
        let (fg, muted, success, danger) = (t.foreground, t.muted_foreground, t.success, t.danger);
        let saved = self.claude_token.is_some();
        let (status, color) = if !self.token_loaded {
            (t!("settings.agent.checking").to_string(), muted)
        } else if saved {
            (t!("settings.agent.token_saved").to_string(), success)
        } else {
            (t!("settings.agent.token_none").to_string(), muted)
        };

        v_flex()
            .pt_2()
            .gap_1p5()
            .border_t_1()
            .border_color(t.border.opacity(0.3))
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .child(Icon::new(BpIcon::FileLock).size_3().text_color(t.primary))
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(fg)
                                    .child("CLAUDE_CODE_OAUTH_TOKEN"),
                            ),
                    )
                    .child(div().text_xs().text_color(color).child(status)),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&self.agent_inputs.claude_token).small()),
                    )
                    .child(
                        Button::new("claude-token-save")
                            .label(t!("settings.agent.token_save"))
                            .small()
                            .primary()
                            .loading(self.token_busy)
                            .disabled(self.token_busy)
                            .on_click(cx.listener(|this, _, window, cx| this.save_token_to_keychain(window, cx))),
                    )
                    .when(saved, |d| {
                        d.child(
                            Button::new("claude-token-delete")
                                .label(t!("settings.agent.token_delete"))
                                .small()
                                .ghost()
                                .disabled(self.token_busy)
                                .on_click(cx.listener(|this, _, _, cx| this.delete_token_from_keychain(cx))),
                        )
                    }),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(t!("settings.agent.token_hint")),
            )
            .when_some(self.token_error.clone(), |d, e| {
                d.child(div().text_xs().text_color(danger).child(e))
            })
            .into_any_element()
    }

    // =========================================================================
    // CÁC COMPONENT CONTROL ĐẶC THÙ (STEPPER PILL, SEGMENTED CONTROLS)
    // =========================================================================

    /// Cụm Stepper Pill Apple-style: [ − ]  14 px  [ + ]
    fn render_stepper_pill(
        &self,
        current: f32,
        min: f32,
        max: f32,
        unit: &'static str,
        on_change: fn(&mut Self, f32, &mut Window, &mut Context<Self>),
        cx: &Context<Self>,
    ) -> AnyElement {
        let t = cx.theme();

        h_flex()
            .p(px(2.5))
            .gap(px(2.))
            .rounded_lg()
            .bg(t.secondary.opacity(0.6))
            .border_1()
            .border_color(t.border.opacity(0.5))
            .items_center()
            // Nút Giảm
            .child(
                div()
                    .id(SharedString::from(format!("stepper-dec-{current}")))
                    .size(px(24.))
                    .rounded_md()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .text_sm()
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(t.foreground)
                    .hover(|s| s.bg(t.background).shadow_sm())
                    .on_click(cx.listener(move |this, _, w, cx| {
                        let new_val = (current - 1.0).max(min);
                        on_change(this, new_val, w, cx);
                    }))
                    .child("−"),
            )
            // Nhãn hiển thị giá trị
            .child(
                div()
                    .w(px(56.))
                    .text_center()
                    .text_xs()
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(t.primary)
                    .child(format!("{:.0} {}", current, unit)),
            )
            // Nút Tăng
            .child(
                div()
                    .id(SharedString::from(format!("stepper-inc-{current}")))
                    .size(px(24.))
                    .rounded_md()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .text_sm()
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(t.foreground)
                    .hover(|s| s.bg(t.background).shadow_sm())
                    .on_click(cx.listener(move |this, _, w, cx| {
                        let new_val = (current + 1.0).min(max);
                        on_change(this, new_val, w, cx);
                    }))
                    .child("+"),
            )
            .into_any_element()
    }

    /// Mục Segmented Monospace Font
    fn render_mono_segmented_item(
        &self,
        font_id: &'static str,
        label: &'static str,
        current: &str,
        cx: &Context<Self>,
    ) -> AnyElement {
        let t = cx.theme();
        let is_active = current == font_id;

        div()
            .id(SharedString::from(format!("mono-seg-{font_id}")))
            .px_2p5()
            .py_1()
            .rounded_md()
            .cursor_pointer()
            .when(is_active, |d| {
                d.bg(t.background)
                    .shadow_sm()
                    .text_color(t.primary)
                    .font_weight(gpui::FontWeight::SEMIBOLD)
            })
            .when(!is_active, |d| {
                d.text_color(t.muted_foreground)
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .hover(|s| s.text_color(t.foreground))
            })
            .text_xs()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.set_mono_font(font_id.to_string(), cx);
            }))
            .child(label)
            .into_any_element()
    }

    /// Mục Segmented Mật độ danh sách
    fn render_density_segmented_item(
        &self,
        height: f32,
        label: &'static str,
        current: f32,
        cx: &Context<Self>,
    ) -> AnyElement {
        let t = cx.theme();
        let is_active = (current - height).abs() < 0.2;

        div()
            .id(SharedString::from(format!("density-seg-{}", height as i32)))
            .px_3()
            .py_1()
            .rounded_md()
            .cursor_pointer()
            .when(is_active, |d| {
                d.bg(t.background)
                    .shadow_sm()
                    .text_color(t.primary)
                    .font_weight(gpui::FontWeight::SEMIBOLD)
            })
            .when(!is_active, |d| {
                d.text_color(t.muted_foreground)
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .hover(|s| s.text_color(t.foreground))
            })
            .text_xs()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.set_row_height(height, cx);
            }))
            .child(label)
            .into_any_element()
    }

    /// Mục Segmented Agent CLI
    fn render_cli_segmented_item(
        &self,
        kind: Option<CliKind>,
        label: &'static str,
        is_active: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let t = cx.theme();

        h_flex()
            .id(SharedString::from(format!("cli-seg-{:?}", kind)))
            .px_3()
            .py_1()
            .gap_1p5()
            .items_center()
            .rounded_md()
            .cursor_pointer()
            .when(is_active, |d| {
                d.bg(t.background)
                    .shadow_sm()
                    .text_color(t.primary)
                    .font_weight(gpui::FontWeight::SEMIBOLD)
            })
            .when(!is_active, |d| {
                d.text_color(t.muted_foreground)
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .hover(|s| s.text_color(t.foreground))
            })
            .when_some(kind, |d, k| {
                d.child(Icon::new(agent_icon(k)).size_3p5().text_color(agent_color(k, cx)))
            })
            .child(div().text_xs().child(label))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.set_agent_cli(kind, cx);
            }))
            .into_any_element()
    }

    /// Công tắc Toggle Pill kiểu Apple: [ Off | On ] đồng bộ
    fn render_toggle_pill(
        &self,
        id: &'static str,
        value: bool,
        set: fn(&mut Self, bool, &mut Context<Self>),
        cx: &Context<Self>,
    ) -> AnyElement {
        let t = cx.theme();

        h_flex()
            .id(SharedString::from(format!("toggle-pill-{id}")))
            .p(px(2.5))
            .gap_1()
            .rounded_lg()
            .bg(t.secondary.opacity(0.6))
            .border_1()
            .border_color(t.border.opacity(0.5))
            .items_center()
            .child(
                div()
                    .id(SharedString::from(format!("toggle-pill-{id}-off")))
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .when(!value, |d| {
                        d.bg(t.background)
                            .shadow_sm()
                            .text_color(t.foreground)
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                    })
                    .when(value, |d| {
                        d.text_color(t.muted_foreground)
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .hover(|s| s.text_color(t.foreground))
                    })
                    .text_xs()
                    .on_click(cx.listener(move |this, _, _, cx| set(this, false, cx)))
                    .child(t!("settings.toggle.off")),
            )
            .child(
                div()
                    .id(SharedString::from(format!("toggle-pill-{id}-on")))
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .when(value, |d| {
                        d.bg(t.primary)
                            .shadow_sm()
                            .text_color(t.primary_foreground)
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                    })
                    .when(!value, |d| {
                        d.text_color(t.muted_foreground)
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .hover(|s| s.text_color(t.foreground))
                    })
                    .text_xs()
                    .on_click(cx.listener(move |this, _, _, cx| set(this, true, cx)))
                    .child(t!("settings.toggle.on")),
            )
            .into_any_element()
    }
}
