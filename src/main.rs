use std::{
    env,
    path::{Path, PathBuf},
    process::Command,
};

use gpui::{
    actions, div, px, rgb, rgba, size, App, Application, Bounds, Context, IntoElement, KeyBinding,
    Menu,
    MenuItem, Render, SharedString, Window, WindowBounds, WindowOptions,
};
use gpui::prelude::*;
use imara_diff::{Algorithm, Diff, InternedInput};

const CONTEXT_LINES: usize = 3;

// GitHub Dark palette from github.com/PyaeSoneAungRgn/github-zed-theme.
mod github_dark {
    pub const BACKGROUND: u32 = 0x0d1117;
    pub const ELEVATED_SURFACE_BACKGROUND: u32 = 0x010409;
    pub const SUBHEADER_BACKGROUND: u32 = 0x151b23;
    pub const ELEMENT_BACKGROUND: u32 = 0x212830;
    pub const ELEMENT_HOVER: u32 = 0x656c7633;
    pub const BORDER: u32 = 0x3d444d;
    pub const TEXT: u32 = 0xf0f6fc;
    pub const MUTED_TEXT: u32 = 0x9198a1;
    pub const LINE_NUMBER: u32 = 0x9198a1;
    pub const ACCENT: u32 = 0x4493f8;
    pub const ACCENT_SELECTED: u32 = 0x1f6feb;
    pub const CREATED: u32 = 0x3fb950;
    pub const CREATED_BACKGROUND: u32 = 0x2ea04326;
    pub const DELETED: u32 = 0xf85149;
    pub const DELETED_BACKGROUND: u32 = 0xf851491a;
}

actions!(jj_gpui_diff, [Quit, ToggleDiffView]);

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

struct DiffViewer {
    repository: PathBuf,
    snapshot: Snapshot,
    view: DiffView,
}

impl DiffViewer {
    fn reload(&mut self, cx: &mut Context<Self>) {
        self.snapshot = load_snapshot(&self.repository);
        cx.notify();
    }

    fn toggle_view(&mut self, cx: &mut Context<Self>) {
        self.view = match self.view {
            DiffView::Unified => DiffView::Split,
            DiffView::Split => DiffView::Unified,
        };
        cx.notify();
    }
}

impl Render for DiffViewer {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut file_list = div().flex().flex_col().gap_4().p_4();

        if let Some(message) = &self.snapshot.message {
            file_list = file_list.child(
                div()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(github_dark::BORDER))
                    .bg(rgb(github_dark::SUBHEADER_BACKGROUND))
                    .p_4()
                    .text_color(rgb(github_dark::TEXT))
                    .child(message.clone()),
            );
        } else if self.snapshot.files.is_empty() {
            file_list = file_list.child(
                div()
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(github_dark::BORDER))
                    .bg(rgb(github_dark::ELEVATED_SURFACE_BACKGROUND))
                    .p_4()
                    .text_color(rgb(github_dark::MUTED_TEXT))
                    .child("The working-copy changeset has no file changes."),
            );
        } else {
            for file in &self.snapshot.files {
                file_list = file_list.child(file_element(file, self.view));
            }
        }

        let view_button_label = match self.view {
            DiffView::Unified => "Split view",
            DiffView::Split => "Unified view",
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(github_dark::BACKGROUND))
            .text_color(rgb(github_dark::TEXT))
            .on_action(cx.listener(|this, _: &ToggleDiffView, _, cx| this.toggle_view(cx)))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(rgb(github_dark::BORDER))
                    .bg(rgb(github_dark::SUBHEADER_BACKGROUND))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().text_lg().child("Jujutsu changeset diff"))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(github_dark::MUTED_TEXT))
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
                                    .id("toggle-diff-view")
                                    .cursor_pointer()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(rgb(github_dark::BORDER))
                                    .bg(rgb(github_dark::ELEMENT_BACKGROUND))
                                    .px_3()
                                    .py_1()
                                    .text_sm()
                                    .hover(|style| style.bg(rgba(github_dark::ELEMENT_HOVER)))
                                    .on_click(cx.listener(|this, _, _, cx| this.toggle_view(cx)))
                                    .child(view_button_label),
                            )
                            .child(
                                div()
                                    .id("refresh")
                                    .cursor_pointer()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(rgb(github_dark::ACCENT))
                                    .bg(rgb(github_dark::ACCENT_SELECTED))
                                    .px_3()
                                    .py_1()
                                    .text_sm()
                                    .hover(|style| style.bg(rgb(github_dark::ACCENT)))
                                    .on_click(cx.listener(|this, _, _, cx| this.reload(cx)))
                                    .child("Refresh"),
                            ),
                    ),
            )
            .child(
                div()
                    .id("diff-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(file_list),
            )
    }
}

fn file_element(file: &DiffFile, view: DiffView) -> impl IntoElement {
    let rows = match view {
        DiffView::Unified => unified_rows_element(file),
        DiffView::Split => split_rows_element(file),
    };

    div()
        .rounded_lg()
        .overflow_hidden()
        .border_1()
        .border_color(rgb(github_dark::BORDER))
        .bg(rgb(github_dark::BACKGROUND))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .px_3()
                .py_2()
                .bg(rgb(github_dark::SUBHEADER_BACKGROUND))
                .child(div().font_family("Menlo").text_sm().child(file.path.clone()))
                .child(
                    div()
                        .font_family("Menlo")
                        .text_sm()
                        .child(
                            div()
                                .text_color(rgb(github_dark::CREATED))
                                .child(format!("+{}", file.additions)),
                        )
                        .child(
                            div()
                                .text_color(rgb(github_dark::DELETED))
                                .child(format!(" −{}", file.deletions)),
                        ),
                ),
        )
        .child(rows)
}

fn unified_rows_element(file: &DiffFile) -> gpui::AnyElement {
    let mut rows = div()
        .id(SharedString::from(file.path.clone()))
        .font_family("Menlo")
        .text_sm()
        .overflow_x_scroll()
        .border_t_1()
        .border_color(rgb(github_dark::BORDER));

    for row in &file.rows {
        let (background, marker, marker_color) = row_appearance(row.kind);
        let old_number = row.old_line.map_or_else(String::new, |n| n.to_string());
        let new_number = row.new_line.map_or_else(String::new, |n| n.to_string());
        rows = rows.child(
            div()
                .flex()
                .min_w_full()
                .bg(background)
                .child(
                    div()
                        .w(px(46.))
                        .pr_2()
                        .text_right()
                        .text_color(rgb(github_dark::LINE_NUMBER))
                        .child(old_number),
                )
                .child(
                    div()
                        .w(px(46.))
                        .pr_2()
                        .text_right()
                        .text_color(rgb(github_dark::LINE_NUMBER))
                        .child(new_number),
                )
                .child(div().w(px(20.)).text_color(marker_color).child(marker))
                .child(
                    div()
                        .whitespace_nowrap()
                        .text_color(rgb(github_dark::TEXT))
                        .child(row.text.replace('\t', "    ")),
                ),
        );
    }

    rows.into_any_element()
}

fn split_rows_element(file: &DiffFile) -> gpui::AnyElement {
    let mut rows = div()
        .id(SharedString::from(format!("{}-split", file.path)))
        .font_family("Menlo")
        .text_sm()
        .overflow_x_scroll()
        .border_t_1()
        .border_color(rgb(github_dark::BORDER));

    for row in split_rows(&file.rows) {
        rows = rows.child(
            div()
                .flex()
                .min_w(px(800.))
                .child(split_cell(row.before, true))
                .child(split_cell(row.after, false)),
        );
    }

    rows.into_any_element()
}

fn split_cell(row: Option<&DiffRow>, before: bool) -> gpui::Div {
    let Some(row) = row else {
        return div().flex_1().bg(rgb(github_dark::BACKGROUND));
    };

    let (background, marker, marker_color) = row_appearance(row.kind);
    let number = if before { row.old_line } else { row.new_line }
        .map_or_else(String::new, |n| n.to_string());

    div()
        .flex_1()
        .flex()
        .bg(background)
        .when(!before, |element| {
            element
                .border_l_1()
                .border_color(rgb(github_dark::BORDER))
        })
        .child(
            div()
                .w(px(46.))
                .flex_none()
                .pr_2()
                .text_right()
                .text_color(rgb(github_dark::LINE_NUMBER))
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
                .text_color(rgb(github_dark::TEXT))
                .child(row.text.replace('\t', "    ")),
        )
}

fn row_appearance(kind: RowKind) -> (gpui::Rgba, &'static str, gpui::Rgba) {
    match kind {
        RowKind::Context => (
            rgb(github_dark::BACKGROUND),
            " ",
            rgb(github_dark::MUTED_TEXT),
        ),
        RowKind::Added => (
            rgba(github_dark::CREATED_BACKGROUND),
            "+",
            rgb(github_dark::CREATED),
        ),
        RowKind::Deleted => (
            rgba(github_dark::DELETED_BACKGROUND),
            "−",
            rgb(github_dark::DELETED),
        ),
        RowKind::Collapsed => (
            rgb(github_dark::SUBHEADER_BACKGROUND),
            "…",
            rgb(github_dark::MUTED_TEXT),
        ),
    }
}

#[derive(Clone, Copy, Debug)]
struct SplitRow<'a> {
    before: Option<&'a DiffRow>,
    after: Option<&'a DiffRow>,
}

fn split_rows(rows: &[DiffRow]) -> Vec<SplitRow<'_>> {
    let mut split = Vec::new();
    let mut index = 0;

    while index < rows.len() {
        match rows[index].kind {
            RowKind::Context | RowKind::Collapsed => {
                split.push(SplitRow {
                    before: Some(&rows[index]),
                    after: Some(&rows[index]),
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
                let changes = &rows[start..index];
                let deleted: Vec<_> = changes
                    .iter()
                    .filter(|row| row.kind == RowKind::Deleted)
                    .collect();
                let added: Vec<_> = changes
                    .iter()
                    .filter(|row| row.kind == RowKind::Added)
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

        for (offset, line) in before_lines[before_range.clone()].iter().enumerate() {
            rows.push(DiffRow {
                kind: RowKind::Deleted,
                old_line: Some(before_range.start + offset + 1),
                new_line: None,
                text: display_line(line),
            });
            deletions += 1;
        }
        for (offset, line) in after_lines[after_range.clone()].iter().enumerate() {
            rows.push(DiffRow {
                kind: RowKind::Added,
                old_line: None,
                new_line: Some(after_range.start + offset + 1),
                text: display_line(line),
            });
            additions += 1;
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
            });
        }
        rows.push(DiffRow {
            kind: RowKind::Context,
            old_line: Some(before_range.start + offset + 1),
            new_line: Some(after_range.start + offset + 1),
            text: display_line(before[before_range.start + offset]),
        });
    }
}

fn display_line(line: &str) -> String {
    line.trim_end_matches(&['\r', '\n'][..]).to_owned()
}

fn main() {
    let repository = env::current_dir().expect("the current directory must be available");
    let initial_snapshot = load_snapshot(&repository);

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
                items: vec![MenuItem::action("Toggle Diff View", ToggleDiffView)],
            },
        ]);

        let bounds = Bounds::centered(None, size(px(1100.), px(760.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| DiffViewer {
                    repository,
                    snapshot: initial_snapshot,
                    view: DiffView::default(),
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
            row.before.map(|line| line.text.as_str()) == Some("two")
                && row.after.map(|line| line.text.as_str()) == Some("TWO")
        }));
        assert!(split.iter().any(|row| {
            row.before.is_none()
                && row.after.map(|line| line.text.as_str()) == Some("four")
        }));
    }
}
