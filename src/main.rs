use std::{
    collections::HashMap,
    env,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

use gpui::{
    actions, div, px, rgb, rgba, size, uniform_list, App, Application, Bounds, Context,
    HighlightStyle, IntoElement, KeyBinding, ListHorizontalSizingBehavior, Menu, MenuItem, Render,
    StyledText, Window, WindowAppearance, WindowBounds, WindowOptions,
};
use gpui::prelude::*;
use imara_diff::{Algorithm, Diff, InternedInput};

const CONTEXT_LINES: usize = 3;
const DIFF_ROW_HEIGHT: f32 = 22.;

actions!(
    jj_gpui_diff,
    [Quit, ToggleDiffView, SetThemeAuto, SetThemeLight, SetThemeDark]
);

fn quit(_: &Quit, cx: &mut App) {
    cx.quit();
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RowKind {
    Context,
    Added,
    Deleted,
    Collapsed,
}

#[derive(Clone, Debug)]
struct DiffRow {
    kind: RowKind,
    old_line: Option<usize>,
    new_line: Option<usize>,
    text: String,
    word_highlights: Vec<std::ops::Range<usize>>,
}

#[derive(Debug)]
struct DiffFile {
    path: String,
    rows: Vec<DiffRow>,
    additions: usize,
    deletions: usize,
}

#[derive(Debug)]
struct Snapshot {
    change: String,
    files: Vec<DiffFile>,
    message: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum DiffView {
    #[default]
    Unified,
    Split,
}

#[derive(Clone, Copy, Debug)]
enum DiffListItem {
    FileHeader { file: usize },
    UnifiedRow { file: usize, row: usize },
    SplitRow {
        file: usize,
        before: Option<usize>,
        after: Option<usize>,
    },
    FileSeparator,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ThemePreference {
    #[default]
    Auto,
    Light,
    Dark,
}

#[derive(Clone, Copy)]
struct ThemePalette {
    background: gpui::Rgba,
    elevated_surface_background: gpui::Rgba,
    subheader_background: gpui::Rgba,
    element_background: gpui::Rgba,
    element_hover: gpui::Rgba,
    border: gpui::Rgba,
    text: gpui::Rgba,
    muted_text: gpui::Rgba,
    line_number: gpui::Rgba,
    accent: gpui::Rgba,
    accent_selected: gpui::Rgba,
    accent_text: gpui::Rgba,
    created: gpui::Rgba,
    created_background: gpui::Rgba,
    created_emphasis_background: gpui::Rgba,
    deleted: gpui::Rgba,
    deleted_background: gpui::Rgba,
    deleted_emphasis_background: gpui::Rgba,
}

impl ThemePalette {
    // GitHub palettes from github.com/PyaeSoneAungRgn/github-zed-theme.
    fn light() -> Self {
        Self {
            background: rgb(0xffffff),
            elevated_surface_background: rgb(0xffffff),
            subheader_background: rgb(0xf6f8fa),
            element_background: rgba(0x818b981f),
            element_hover: rgba(0x818b981f),
            border: rgb(0xd1d9e0),
            text: rgb(0x1f2328),
            muted_text: rgb(0x59636e),
            line_number: rgb(0x59636e),
            accent: rgb(0x0969da),
            accent_selected: rgb(0x0969da),
            accent_text: rgb(0xffffff),
            created: rgb(0x1a7f37),
            created_background: rgb(0xdafbe1),
            created_emphasis_background: rgb(0xaceebb),
            deleted: rgb(0xd1242f),
            deleted_background: rgb(0xffebe9),
            deleted_emphasis_background: rgb(0xffc1c0),
        }
    }

    fn dark() -> Self {
        Self {
            background: rgb(0x0d1117),
            elevated_surface_background: rgb(0x010409),
            subheader_background: rgb(0x151b23),
            element_background: rgba(0x656c7633),
            element_hover: rgba(0x656c7633),
            border: rgb(0x3d444d),
            text: rgb(0xf0f6fc),
            muted_text: rgb(0x9198a1),
            line_number: rgb(0x9198a1),
            accent: rgb(0x4493f8),
            accent_selected: rgb(0x1f6feb),
            accent_text: rgb(0xffffff),
            created: rgb(0x3fb950),
            created_background: rgba(0x2ea04326),
            created_emphasis_background: rgba(0x2ea04366),
            deleted: rgb(0xf85149),
            deleted_background: rgba(0xf851491a),
            deleted_emphasis_background: rgba(0xf8514966),
        }
    }

    fn resolve(preference: ThemePreference, appearance: WindowAppearance) -> Self {
        match preference {
            ThemePreference::Light => Self::light(),
            ThemePreference::Dark => Self::dark(),
            ThemePreference::Auto => match appearance {
                WindowAppearance::Light | WindowAppearance::VibrantLight => Self::light(),
                WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::dark(),
            },
        }
    }
}

struct DiffViewer {
    repository: PathBuf,
    snapshot: Arc<Snapshot>,
    file_states: Vec<FileViewState>,
    list_items: Arc<Vec<DiffListItem>>,
    widest_item: usize,
    view: DiffView,
    theme: ThemePreference,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FileViewState {
    expanded: bool,
    accepted: bool,
}

impl Default for FileViewState {
    fn default() -> Self {
        Self {
            expanded: true,
            accepted: false,
        }
    }
}

impl FileViewState {
    fn toggle_expanded(&mut self) {
        self.expanded = !self.expanded;
    }

    fn toggle_accepted(&mut self) {
        self.accepted = !self.accepted;
        if self.accepted {
            self.expanded = false;
        }
    }
}

impl DiffViewer {
    fn reload(&mut self, cx: &mut Context<Self>) {
        let states_by_path: HashMap<_, _> = self
            .snapshot
            .files
            .iter()
            .zip(self.file_states.iter().copied())
            .map(|(file, state)| (file.path.clone(), state))
            .collect();
        self.snapshot = Arc::new(load_snapshot(&self.repository));
        self.file_states = self
            .snapshot
            .files
            .iter()
            .map(|file| states_by_path.get(&file.path).copied().unwrap_or_default())
            .collect();
        self.rebuild_list();
        cx.notify();
    }

    fn rebuild_list(&mut self) {
        let (list_items, widest_item) =
            build_list_items(&self.snapshot, self.view, &self.file_states);
        self.list_items = Arc::new(list_items);
        self.widest_item = widest_item;
    }

    fn toggle_view(&mut self, cx: &mut Context<Self>) {
        self.view = match self.view {
            DiffView::Unified => DiffView::Split,
            DiffView::Split => DiffView::Unified,
        };
        self.rebuild_list();
        cx.notify();
    }

    fn toggle_file_expanded(&mut self, file: usize, cx: &mut Context<Self>) {
        if let Some(state) = self.file_states.get_mut(file) {
            state.toggle_expanded();
            self.rebuild_list();
        }
        cx.notify();
    }

    fn toggle_file_accepted(&mut self, file: usize, cx: &mut Context<Self>) {
        if let Some(state) = self.file_states.get_mut(file) {
            state.toggle_accepted();
            self.rebuild_list();
        }
        cx.notify();
    }

    fn set_theme(&mut self, theme: ThemePreference, cx: &mut Context<Self>) {
        self.theme = theme;
        cx.notify();
    }
}

impl Render for DiffViewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = ThemePalette::resolve(self.theme, window.appearance());
        let diff_content = if let Some(message) = &self.snapshot.message {
            div()
                .p_4()
                .child(
                div()
                    .rounded_md()
                    .border_1()
                    .border_color(palette.border)
                    .bg(palette.subheader_background)
                    .p_4()
                    .text_color(palette.text)
                    .child(message.clone()),
                )
                .into_any_element()
        } else if self.snapshot.files.is_empty() {
            div()
                .p_4()
                .child(
                div()
                    .rounded_md()
                    .border_1()
                    .border_color(palette.border)
                    .bg(palette.elevated_surface_background)
                    .p_4()
                    .text_color(palette.muted_text)
                    .child("The working-copy changeset has no file changes."),
                )
                .into_any_element()
        } else {
            // As in Zed's large scrolling views, only build elements for the visible range.
            uniform_list(
                "diff-lines",
                self.list_items.len(),
                cx.processor(move |this, range: std::ops::Range<usize>, _window, cx| {
                    let snapshot = this.snapshot.clone();
                    let list_items = this.list_items.clone();
                    range
                        .map(|index| {
                            render_diff_list_item(
                                &snapshot,
                                list_items[index],
                                &this.file_states,
                                palette,
                                cx,
                            )
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .with_width_from_item(Some(self.widest_item))
            .with_horizontal_sizing_behavior(ListHorizontalSizingBehavior::Unconstrained)
            .size_full()
            .px_4()
            .into_any_element()
        };

        let view_button_label = match self.view {
            DiffView::Unified => "Split view",
            DiffView::Split => "Unified view",
        };
        let auto_theme = self.theme == ThemePreference::Auto;
        let light_theme = self.theme == ThemePreference::Light;
        let dark_theme = self.theme == ThemePreference::Dark;

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(palette.background)
            .text_color(palette.text)
            .on_action(cx.listener(|this, _: &ToggleDiffView, _, cx| this.toggle_view(cx)))
            .on_action(cx.listener(|this, _: &SetThemeAuto, _, cx| {
                this.set_theme(ThemePreference::Auto, cx)
            }))
            .on_action(cx.listener(|this, _: &SetThemeLight, _, cx| {
                this.set_theme(ThemePreference::Light, cx)
            }))
            .on_action(cx.listener(|this, _: &SetThemeDark, _, cx| {
                this.set_theme(ThemePreference::Dark, cx)
            }))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(palette.border)
                    .bg(palette.subheader_background)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().text_lg().child("Jujutsu changeset diff"))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(palette.muted_text)
                                    .child(format!("{}  ·  {}", self.repository.display(), self.snapshot.change)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(palette.muted_text)
                                            .child("Theme"),
                                    )
                                    .child(
                                        div()
                                            .id("theme-auto")
                                            .cursor_pointer()
                                            .rounded_md()
                                            .border_1()
                                            .border_color(palette.border)
                                            .bg(if auto_theme {
                                                palette.accent_selected
                                            } else {
                                                palette.element_background
                                            })
                                            .px_2()
                                            .py_1()
                                            .text_sm()
                                            .text_color(if auto_theme {
                                                palette.accent_text
                                            } else {
                                                palette.text
                                            })
                                            .hover(|style| {
                                                style.bg(if auto_theme {
                                                    palette.accent
                                                } else {
                                                    palette.element_hover
                                                })
                                            })
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.set_theme(ThemePreference::Auto, cx)
                                            }))
                                            .child("Auto"),
                                    )
                                    .child(
                                        div()
                                            .id("theme-light")
                                            .cursor_pointer()
                                            .rounded_md()
                                            .border_1()
                                            .border_color(palette.border)
                                            .bg(if light_theme {
                                                palette.accent_selected
                                            } else {
                                                palette.element_background
                                            })
                                            .px_2()
                                            .py_1()
                                            .text_sm()
                                            .text_color(if light_theme {
                                                palette.accent_text
                                            } else {
                                                palette.text
                                            })
                                            .hover(|style| {
                                                style.bg(if light_theme {
                                                    palette.accent
                                                } else {
                                                    palette.element_hover
                                                })
                                            })
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.set_theme(ThemePreference::Light, cx)
                                            }))
                                            .child("Light"),
                                    )
                                    .child(
                                        div()
                                            .id("theme-dark")
                                            .cursor_pointer()
                                            .rounded_md()
                                            .border_1()
                                            .border_color(palette.border)
                                            .bg(if dark_theme {
                                                palette.accent_selected
                                            } else {
                                                palette.element_background
                                            })
                                            .px_2()
                                            .py_1()
                                            .text_sm()
                                            .text_color(if dark_theme {
                                                palette.accent_text
                                            } else {
                                                palette.text
                                            })
                                            .hover(|style| {
                                                style.bg(if dark_theme {
                                                    palette.accent
                                                } else {
                                                    palette.element_hover
                                                })
                                            })
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.set_theme(ThemePreference::Dark, cx)
                                            }))
                                            .child("Dark"),
                                    ),
                            )
                            .child(
                                div()
                                    .id("toggle-diff-view")
                                    .cursor_pointer()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(palette.border)
                                    .bg(palette.element_background)
                                    .px_3()
                                    .py_1()
                                    .text_sm()
                                    .hover(|style| style.bg(palette.element_hover))
                                    .on_click(cx.listener(|this, _, _, cx| this.toggle_view(cx)))
                                    .child(view_button_label),
                            )
                            .child(
                                div()
                                    .id("refresh")
                                    .cursor_pointer()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(palette.accent)
                                    .bg(palette.accent_selected)
                                    .px_3()
                                    .py_1()
                                    .text_sm()
                                    .text_color(palette.accent_text)
                                    .hover(|style| style.bg(palette.accent))
                                    .on_click(cx.listener(|this, _, _, cx| this.reload(cx)))
                                    .child("Refresh"),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .child(diff_content),
            )
    }
}

fn render_diff_list_item(
    snapshot: &Snapshot,
    item: DiffListItem,
    file_states: &[FileViewState],
    palette: ThemePalette,
    cx: &mut Context<DiffViewer>,
) -> gpui::AnyElement {
    match item {
        DiffListItem::FileHeader { file } => {
            let diff_file = &snapshot.files[file];
            let state = file_states[file];
            div()
                .h(px(DIFF_ROW_HEIGHT))
                .flex_none()
                .flex()
                .min_w_full()
                .items_center()
                .justify_between()
                .px_6()
                .border_b_1()
                .border_color(palette.border)
                .bg(palette.subheader_background)
                .font_family("Menlo")
                .text_sm()
                .child(
                    div()
                        .flex()
                        .flex_1()
                        .items_center()
                        .gap_2()
                        .min_w_0()
                        .child(
                            div()
                                .id(("file-disclosure", file))
                                .w(px(20.))
                                .flex_none()
                                .cursor_pointer()
                                .text_color(palette.muted_text)
                                .hover(|style| style.text_color(palette.text))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.toggle_file_expanded(file, cx)
                                }))
                                .child(if state.expanded { "▾" } else { "▸" }),
                        )
                        .child(
                            div()
                                .truncate()
                                .child(diff_file.path.clone()),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .id(("file-accepted", file))
                                .flex()
                                .items_center()
                                .gap_1()
                                .cursor_pointer()
                                .text_color(if state.accepted {
                                    palette.created
                                } else {
                                    palette.muted_text
                                })
                                .hover(|style| style.text_color(palette.text))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.toggle_file_accepted(file, cx)
                                }))
                                .child(
                                    div()
                                        .w(px(16.))
                                        .h(px(16.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded_sm()
                                        .border_1()
                                        .border_color(if state.accepted {
                                            palette.created
                                        } else {
                                            palette.border
                                        })
                                        .bg(if state.accepted {
                                            palette.created_background
                                        } else {
                                            palette.background
                                        })
                                        .child(if state.accepted { "✓" } else { "" }),
                                )
                                .child("Accepted"),
                        )
                        .child(
                            div()
                                .flex()
                                .child(
                                    div()
                                        .text_color(palette.created)
                                        .child(format!("+{}", diff_file.additions)),
                                )
                                .child(
                                    div()
                                        .text_color(palette.deleted)
                                        .child(format!(" −{}", diff_file.deletions)),
                                ),
                        ),
                )
                .into_any_element()
        }
        DiffListItem::UnifiedRow { file, row } => {
            unified_row_element(&snapshot.files[file].rows[row], palette)
        }
        DiffListItem::SplitRow {
            file,
            before,
            after,
        } => {
            let rows = &snapshot.files[file].rows;
            div()
                .h(px(DIFF_ROW_HEIGHT))
                .flex_none()
                .flex()
                .w_full()
                .min_w(px(800.))
                .font_family("Menlo")
                .text_sm()
                .child(split_cell(before.map(|row| &rows[row]), true, palette))
                .child(split_cell(after.map(|row| &rows[row]), false, palette))
                .into_any_element()
        }
        DiffListItem::FileSeparator => div()
            .h(px(DIFF_ROW_HEIGHT))
            .flex_none()
            .bg(palette.background)
            .into_any_element(),
    }
}

fn unified_row_element(row: &DiffRow, palette: ThemePalette) -> gpui::AnyElement {
    let (background, marker, marker_color) = row_appearance(row.kind, palette);
    let old_number = row.old_line.map_or_else(String::new, |n| n.to_string());
    let new_number = row.new_line.map_or_else(String::new, |n| n.to_string());

    div()
        .h(px(DIFF_ROW_HEIGHT))
        .flex_none()
        .flex()
        .min_w_full()
        .bg(background)
        .font_family("Menlo")
        .text_sm()
        .child(
            div()
                .w(px(46.))
                .flex_none()
                .pr_2()
                .text_right()
                .text_color(palette.line_number)
                .child(old_number),
        )
        .child(
            div()
                .w(px(46.))
                .flex_none()
                .pr_2()
                .text_right()
                .text_color(palette.line_number)
                .child(new_number),
        )
        .child(
            div()
                .w(px(20.))
                .flex_none()
                .text_color(marker_color)
                .child(marker),
        )
        .child(
            div()
                .whitespace_nowrap()
                .text_color(palette.text)
                .child(highlighted_row_text(row, palette)),
        )
        .into_any_element()
}

fn split_cell(row: Option<&DiffRow>, before: bool, palette: ThemePalette) -> gpui::Div {
    let Some(row) = row else {
        return div()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .bg(palette.background);
    };

    let (background, marker, marker_color) = row_appearance(row.kind, palette);
    let number = if before { row.old_line } else { row.new_line }
        .map_or_else(String::new, |n| n.to_string());

    div()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .flex()
        .bg(background)
        .when(!before, |element| {
            element
                .border_l_1()
                .border_color(palette.border)
        })
        .child(
            div()
                .w(px(46.))
                .flex_none()
                .pr_2()
                .text_right()
                .text_color(palette.line_number)
                .child(number),
        )
        .child(
            div()
                .w(px(20.))
                .flex_none()
                .text_color(marker_color)
                .child(marker),
        )
        .child(
            div()
                .whitespace_nowrap()
                .text_color(palette.text)
                .child(highlighted_row_text(row, palette)),
        )
}

fn highlighted_row_text(row: &DiffRow, palette: ThemePalette) -> StyledText {
    let background = match row.kind {
        RowKind::Added => palette.created_emphasis_background,
        RowKind::Deleted => palette.deleted_emphasis_background,
        RowKind::Context | RowKind::Collapsed => palette.background,
    };
    let style = HighlightStyle {
        background_color: Some(background.into()),
        ..Default::default()
    };

    StyledText::new(row.text.clone()).with_highlights(
        row.word_highlights
            .iter()
            .cloned()
            .map(|range| (range, style)),
    )
}

fn row_appearance(
    kind: RowKind,
    palette: ThemePalette,
) -> (gpui::Rgba, &'static str, gpui::Rgba) {
    match kind {
        RowKind::Context => (palette.background, " ", palette.muted_text),
        RowKind::Added => (palette.created_background, "+", palette.created),
        RowKind::Deleted => (palette.deleted_background, "−", palette.deleted),
        RowKind::Collapsed => (palette.subheader_background, "…", palette.muted_text),
    }
}

#[derive(Clone, Copy, Debug)]
struct SplitRow {
    before: Option<usize>,
    after: Option<usize>,
}

fn split_rows(rows: &[DiffRow]) -> Vec<SplitRow> {
    let mut split = Vec::new();
    let mut index = 0;

    while index < rows.len() {
        match rows[index].kind {
            RowKind::Context | RowKind::Collapsed => {
                split.push(SplitRow {
                    before: Some(index),
                    after: Some(index),
                });
                index += 1;
            }
            RowKind::Added | RowKind::Deleted => {
                let start = index;
                while index < rows.len()
                    && matches!(rows[index].kind, RowKind::Added | RowKind::Deleted)
                {
                    index += 1;
                }
                let deleted: Vec<_> = (start..index)
                    .filter(|row| rows[*row].kind == RowKind::Deleted)
                    .collect();
                let added: Vec<_> = (start..index)
                    .filter(|row| rows[*row].kind == RowKind::Added)
                    .collect();
                for offset in 0..deleted.len().max(added.len()) {
                    split.push(SplitRow {
                        before: deleted.get(offset).copied(),
                        after: added.get(offset).copied(),
                    });
                }
            }
        }
    }

    split
}

fn build_list_items(
    snapshot: &Snapshot,
    view: DiffView,
    file_states: &[FileViewState],
) -> (Vec<DiffListItem>, usize) {
    let mut items = Vec::new();

    for (file_index, file) in snapshot.files.iter().enumerate() {
        items.push(DiffListItem::FileHeader { file: file_index });
        if file_states.get(file_index).is_none_or(|state| state.expanded) {
            match view {
                DiffView::Unified => {
                    items.extend((0..file.rows.len()).map(|row| DiffListItem::UnifiedRow {
                        file: file_index,
                        row,
                    }));
                }
                DiffView::Split => {
                    items.extend(split_rows(&file.rows).into_iter().map(|row| {
                        DiffListItem::SplitRow {
                            file: file_index,
                            before: row.before,
                            after: row.after,
                        }
                    }));
                }
            }
        }
        items.push(DiffListItem::FileSeparator);
    }

    let widest_item = items
        .iter()
        .enumerate()
        .max_by_key(|(_, item)| list_item_width_score(snapshot, **item))
        .map_or(0, |(index, _)| index);

    (items, widest_item)
}

fn list_item_width_score(snapshot: &Snapshot, item: DiffListItem) -> usize {
    match item {
        DiffListItem::FileHeader { file } => snapshot.files[file].path.chars().count() + 36,
        DiffListItem::UnifiedRow { file, row } => {
            snapshot.files[file].rows[row].text.chars().count() + 16
        }
        DiffListItem::SplitRow {
            file,
            before,
            after,
        } => {
            let rows = &snapshot.files[file].rows;
            before.map_or(0, |row| rows[row].text.chars().count())
                + after.map_or(0, |row| rows[row].text.chars().count())
                + 20
        }
        DiffListItem::FileSeparator => 0,
    }
}

fn load_snapshot(repository: &Path) -> Snapshot {
    let root = match jj(repository, ["root"]) {
        Ok(root) => root,
        Err(error) => {
            return Snapshot {
                change: "no Jujutsu repository".into(),
                files: vec![],
                message: Some(format!(
                    "Could not open a Jujutsu repository from {}.\n\n{}",
                    repository.display(),
                    error
                )),
            };
        }
    };
    let root = PathBuf::from(root.trim());

    let change = jj(&root, ["log", "-r", "@", "-T", "change_id.short() ++ \"  \" ++ description.first_line()"])
        .unwrap_or_else(|_| "@".into())
        .trim()
        .to_owned();

    let paths = match jj(&root, ["diff", "-r", "@", "--name-only"]) {
        Ok(paths) => paths,
        Err(error) => {
            return Snapshot {
                change,
                files: vec![],
                message: Some(error),
            };
        }
    };

    let files = paths
        .lines()
        .filter(|path| !path.is_empty())
        .map(|path| build_file_diff(&root, path))
        .collect();

    Snapshot {
        change,
        files,
        message: None,
    }
}

fn build_file_diff(repository: &Path, path: &str) -> DiffFile {
    // A missing file on one side is an addition/deletion, so it is intentionally empty.
    let before = jj_file(repository, "@-", path);
    let after = jj_file(repository, "@", path);
    if matches!(&before, Err(error) if error == "Binary files are not displayed.")
        || matches!(&after, Err(error) if error == "Binary files are not displayed.")
    {
        return DiffFile {
            path: path.into(),
            rows: vec![DiffRow {
                kind: RowKind::Collapsed,
                old_line: None,
                new_line: None,
                text: "Binary or non-UTF-8 file — no text diff available".into(),
                word_highlights: vec![],
            }],
            additions: 0,
            deletions: 0,
        };
    }

    let (rows, additions, deletions) = diff_rows(
        &before.unwrap_or_default(),
        &after.unwrap_or_default(),
    );
    DiffFile {
        path: path.into(),
        rows,
        additions,
        deletions,
    }
}

fn jj_file(repository: &Path, revision: &str, path: &str) -> Result<String, String> {
    let output = Command::new("jj")
        .current_dir(repository)
        .args(["file", "show", "-r", revision, "--", path])
        .output()
        .map_err(|error| format!("Unable to run jj: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    String::from_utf8(output.stdout).map_err(|_| "Binary files are not displayed.".into())
}

fn jj<const N: usize>(repository: &Path, arguments: [&str; N]) -> Result<String, String> {
    let output = Command::new("jj")
        .current_dir(repository)
        .args(arguments)
        .output()
        .map_err(|error| format!("Unable to run jj: {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

fn word_token_ranges(text: &str) -> Vec<std::ops::Range<usize>> {
    fn token_class(character: char) -> u8 {
        if character.is_whitespace() {
            0
        } else if character.is_alphanumeric() || character == '_' {
            1
        } else {
            2
        }
    }

    let mut ranges = Vec::new();
    let mut current: Option<(usize, u8)> = None;

    for (index, character) in text.char_indices() {
        let class = token_class(character);
        if class == 2 {
            if let Some((start, _)) = current.take() {
                ranges.push(start..index);
            }
            ranges.push(index..index + character.len_utf8());
        } else if let Some((start, current_class)) = current {
            if class != current_class {
                ranges.push(start..index);
                current = Some((index, class));
            }
        } else {
            current = Some((index, class));
        }
    }
    if let Some((start, _)) = current {
        ranges.push(start..text.len());
    }
    ranges
}

fn token_range_to_byte_range(
    tokens: &[std::ops::Range<usize>],
    range: std::ops::Range<u32>,
) -> Option<std::ops::Range<usize>> {
    if range.is_empty() {
        return None;
    }
    let start = range.start as usize;
    let end = range.end as usize;
    Some(tokens[start].start..tokens[end - 1].end)
}

fn word_diff_ranges(
    before: &str,
    after: &str,
) -> (
    Vec<std::ops::Range<usize>>,
    Vec<std::ops::Range<usize>>,
) {
    let before_tokens = word_token_ranges(before);
    let after_tokens = word_token_ranges(after);
    let mut input = InternedInput::<String>::default();
    input.update_before(
        before_tokens
            .iter()
            .map(|range| before[range.clone()].to_owned()),
    );
    input.update_after(
        after_tokens
            .iter()
            .map(|range| after[range.clone()].to_owned()),
    );
    let diff = Diff::compute(Algorithm::Histogram, &input);
    let mut before_highlights = Vec::new();
    let mut after_highlights = Vec::new();

    for hunk in diff.hunks() {
        if let Some(range) = token_range_to_byte_range(&before_tokens, hunk.before) {
            before_highlights.push(range);
        }
        if let Some(range) = token_range_to_byte_range(&after_tokens, hunk.after) {
            after_highlights.push(range);
        }
    }

    (before_highlights, after_highlights)
}

fn diff_rows(before: &str, after: &str) -> (Vec<DiffRow>, usize, usize) {
    let before_lines: Vec<&str> = before.split_inclusive('\n').collect();
    let after_lines: Vec<&str> = after.split_inclusive('\n').collect();
    let input = InternedInput::new(before, after);
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    // This is the same line-oriented post-processing Zed applies after imara-diff.
    diff.postprocess_lines(&input);

    let mut rows = Vec::new();
    let mut previous_before = 0usize;
    let mut previous_after = 0usize;
    let mut additions = 0usize;
    let mut deletions = 0usize;

    for hunk in diff.hunks() {
        let before_range = hunk.before.start as usize..hunk.before.end as usize;
        let after_range = hunk.after.start as usize..hunk.after.end as usize;
        append_context(
            &mut rows,
            &before_lines,
            previous_before..before_range.start,
            previous_after..after_range.start,
        );

        let deleted_row_start = rows.len();
        for (offset, line) in before_lines[before_range.clone()].iter().enumerate() {
            let text = display_line(line);
            let word_highlights = (!text.is_empty()).then_some(0..text.len()).into_iter().collect();
            rows.push(DiffRow {
                kind: RowKind::Deleted,
                old_line: Some(before_range.start + offset + 1),
                new_line: None,
                text,
                word_highlights,
            });
            deletions += 1;
        }
        let added_row_start = rows.len();
        for (offset, line) in after_lines[after_range.clone()].iter().enumerate() {
            let text = display_line(line);
            let word_highlights = (!text.is_empty()).then_some(0..text.len()).into_iter().collect();
            rows.push(DiffRow {
                kind: RowKind::Added,
                old_line: None,
                new_line: Some(after_range.start + offset + 1),
                text,
                word_highlights,
            });
            additions += 1;
        }
        for offset in 0..before_range.len().min(after_range.len()) {
            let deleted_row = deleted_row_start + offset;
            let added_row = added_row_start + offset;
            let (deleted_highlights, added_highlights) =
                word_diff_ranges(&rows[deleted_row].text, &rows[added_row].text);
            rows[deleted_row].word_highlights = deleted_highlights;
            rows[added_row].word_highlights = added_highlights;
        }
        previous_before = before_range.end;
        previous_after = after_range.end;
    }

    append_context(
        &mut rows,
        &before_lines,
        previous_before..before_lines.len(),
        previous_after..after_lines.len(),
    );
    (rows, additions, deletions)
}

fn append_context(
    rows: &mut Vec<DiffRow>,
    before: &[&str],
    before_range: std::ops::Range<usize>,
    after_range: std::ops::Range<usize>,
) {
    let count = before_range.len();
    let keep_all = count <= CONTEXT_LINES * 2;
    let mut indices: Vec<usize> = if keep_all {
        (0..count).collect()
    } else {
        (0..CONTEXT_LINES)
            .chain((count - CONTEXT_LINES)..count)
            .collect()
    };

    for (position, offset) in indices.drain(..).enumerate() {
        if !keep_all && position == CONTEXT_LINES {
            rows.push(DiffRow {
                kind: RowKind::Collapsed,
                old_line: None,
                new_line: None,
                text: format!("{} unchanged lines", count - CONTEXT_LINES * 2),
                word_highlights: vec![],
            });
        }
        rows.push(DiffRow {
            kind: RowKind::Context,
            old_line: Some(before_range.start + offset + 1),
            new_line: Some(after_range.start + offset + 1),
            text: display_line(before[before_range.start + offset]),
            word_highlights: vec![],
        });
    }
}

fn display_line(line: &str) -> String {
    line.trim_end_matches(&['\r', '\n'][..])
        .replace('\t', "    ")
}

fn main() {
    let repository = env::current_dir().expect("the current directory must be available");
    let initial_snapshot = Arc::new(load_snapshot(&repository));
    let initial_file_states = vec![FileViewState::default(); initial_snapshot.files.len()];
    let (initial_list_items, widest_item) =
        build_list_items(&initial_snapshot, DiffView::default(), &initial_file_states);
    let initial_list_items = Arc::new(initial_list_items);

    Application::new().run(move |cx: &mut App| {
        cx.on_action(quit);
        cx.bind_keys([KeyBinding::new("cmd-q", Quit, None)]);
        cx.bind_keys([KeyBinding::new("cmd-shift-d", ToggleDiffView, None)]);
        cx.set_menus(vec![
            Menu {
                name: "jj-gpui-diff".into(),
                items: vec![MenuItem::action("Quit jj-gpui-diff", Quit)],
            },
            Menu {
                name: "View".into(),
                items: vec![
                    MenuItem::action("Toggle Diff View", ToggleDiffView),
                    MenuItem::separator(),
                    MenuItem::submenu(Menu {
                        name: "Theme".into(),
                        items: vec![
                            MenuItem::action("Auto", SetThemeAuto),
                            MenuItem::action("Light", SetThemeLight),
                            MenuItem::action("Dark", SetThemeDark),
                        ],
                    }),
                ],
            },
        ]);

        let bounds = Bounds::centered(None, size(px(1100.), px(760.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |window, cx| {
                cx.new(|cx| {
                    cx.observe_window_appearance(window, |_, window, _| window.refresh())
                        .detach();
                    DiffViewer {
                        repository,
                        snapshot: initial_snapshot,
                        file_states: initial_file_states,
                        list_items: initial_list_items,
                        widest_item,
                        view: DiffView::default(),
                        theme: ThemePreference::default(),
                    }
                })
            },
        )
        .expect("could not open macOS window");
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn produces_colored_rows_from_imara_hunks() {
        let (rows, additions, deletions) = diff_rows("one\ntwo\nthree\n", "one\nTWO\nthree\nfour\n");
        assert_eq!((additions, deletions), (2, 1));
        assert!(rows.iter().any(|row| row.kind == RowKind::Deleted && row.text == "two"));
        assert!(rows.iter().any(|row| row.kind == RowKind::Added && row.text == "TWO"));
        assert!(rows.iter().any(|row| row.kind == RowKind::Added && row.text == "four"));
    }

    #[test]
    fn aligns_changed_lines_in_split_rows() {
        let (rows, _, _) = diff_rows("one\ntwo\nthree\n", "one\nTWO\nthree\nfour\n");
        let split = split_rows(&rows);

        assert!(split.iter().any(|row| {
            row.before.map(|line| rows[line].text.as_str()) == Some("two")
                && row.after.map(|line| rows[line].text.as_str()) == Some("TWO")
        }));
        assert!(split.iter().any(|row| {
            row.before.is_none()
                && row.after.map(|line| rows[line].text.as_str()) == Some("four")
        }));
    }

    #[test]
    fn highlights_only_changed_words_on_paired_lines() {
        let before = "let color = red;";
        let after = "let color = blue;";
        let (deleted, added) = word_diff_ranges(before, after);

        assert_eq!(
            deleted
                .iter()
                .map(|range| &before[range.clone()])
                .collect::<Vec<_>>(),
            vec!["red"]
        );
        assert_eq!(
            added
                .iter()
                .map(|range| &after[range.clone()])
                .collect::<Vec<_>>(),
            vec!["blue"]
        );
    }

    #[test]
    fn highlights_the_whole_text_of_an_unpaired_added_line() {
        let (rows, _, _) = diff_rows("", "entirely new line\n");
        let added = rows
            .iter()
            .find(|row| row.kind == RowKind::Added)
            .expect("an added row");

        assert_eq!(added.word_highlights, vec![0..added.text.len()]);
    }

    #[test]
    fn auto_theme_follows_window_appearance() {
        let light = ThemePalette::resolve(ThemePreference::Auto, WindowAppearance::Light);
        let dark = ThemePalette::resolve(ThemePreference::Auto, WindowAppearance::Dark);

        assert_eq!(light.background, rgb(0xffffff));
        assert_eq!(dark.background, rgb(0x0d1117));
    }

    #[test]
    fn accepting_a_file_collapses_it_but_does_not_lock_disclosure() {
        let mut state = FileViewState::default();

        state.toggle_accepted();
        assert!(state.accepted);
        assert!(!state.expanded);

        state.toggle_expanded();
        assert!(state.accepted);
        assert!(state.expanded);

        state.toggle_accepted();
        assert!(!state.accepted);
        assert!(state.expanded);
    }

    #[test]
    fn collapsed_files_only_contribute_their_header_and_separator() {
        let snapshot = Snapshot {
            change: "test".into(),
            files: vec![DiffFile {
                path: "src/main.rs".into(),
                rows: vec![
                    DiffRow {
                        kind: RowKind::Deleted,
                        old_line: Some(1),
                        new_line: None,
                        text: "before".into(),
                        word_highlights: vec![0..6],
                    },
                    DiffRow {
                        kind: RowKind::Added,
                        old_line: None,
                        new_line: Some(1),
                        text: "after".into(),
                        word_highlights: vec![0..5],
                    },
                ],
                additions: 1,
                deletions: 1,
            }],
            message: None,
        };

        let expanded = build_list_items(
            &snapshot,
            DiffView::Unified,
            &[FileViewState::default()],
        )
        .0;
        let collapsed = build_list_items(
            &snapshot,
            DiffView::Unified,
            &[FileViewState {
                expanded: false,
                accepted: false,
            }],
        )
        .0;

        assert_eq!(expanded.len(), 4);
        assert_eq!(collapsed.len(), 2);
        assert!(matches!(collapsed[0], DiffListItem::FileHeader { file: 0 }));
        assert!(matches!(collapsed[1], DiffListItem::FileSeparator));
    }
}
