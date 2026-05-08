use std::borrow::Cow;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use anstyle::{AnsiColor, Style};
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, Copy)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone)]
pub struct Ui {
    color: bool,
    rich: bool,
    root_abs: PathBuf,
    root_canonical: Option<PathBuf>,
}

impl Ui {
    pub fn from_env(mode: ColorMode, root: &Path) -> Self {
        let tty = std::io::stdout().is_terminal();
        let no_color = std::env::var_os("NO_COLOR").is_some();
        let ci = std::env::var_os("CI").is_some();
        let color = match mode {
            ColorMode::Auto => tty && !no_color,
            ColorMode::Always => !no_color,
            ColorMode::Never => false,
        };
        let root_abs = absolutize(root);
        let root_canonical = root_abs.canonicalize().ok();
        Self {
            color,
            rich: tty && !ci,
            root_abs,
            root_canonical,
        }
    }

    pub fn color_enabled(&self) -> bool {
        self.color
    }

    pub fn rich_enabled(&self) -> bool {
        self.rich
    }

    /// Render the unified status header line.
    ///
    /// Rich mode (TTY, non-CI):
    ///   `<state-colored glyph>  <bold cmd>   <primary>   <muted (s[0], s[1], ...)>`
    /// Plain mode (CI, NO_COLOR, pipe):
    ///   `<cmd>  <primary>  (s[0], s[1], ...)`
    ///
    /// Glyph is suppressed in plain mode so CI consumers can grep
    /// `<cmd>  <primary>` reliably. The summary is rendered as parens-wrapped
    /// comma-separated entries — empty `summary` omits the parens entirely.
    pub fn status_header(
        &self,
        command: &str,
        state: Status,
        primary: impl AsRef<str>,
        summary: &[String],
    ) -> String {
        let primary_str = primary.as_ref();
        let mut text = String::new();

        if self.rich {
            let glyph = self.state_glyph(state);
            text.push_str(&self.colorize_state(state, glyph));
            text.push_str("  ");
        }
        text.push_str(&self.strong(command));
        if self.rich {
            text.push_str("   ");
        } else {
            text.push_str("  ");
        }
        text.push_str(primary_str);

        if !summary.is_empty() {
            if self.rich {
                text.push_str("   ");
            } else {
                text.push_str("  ");
            }
            let joined = summary.join(", ");
            let body = format!("({joined})");
            text.push_str(&self.muted(&body));
        }
        text
    }

    /// Render a finding detail block (severity row + path row + paragraph body).
    ///
    /// Rich mode:
    ///   `  <state-colored glyph>  <severity-colored label>  <bold code>`
    ///   `  <muted path>`
    ///   ``
    ///   `    <message>` (4-space indent; multi-line input is preserved verbatim,
    ///   trailing newline is normalized; callers do not need to indent themselves)
    ///   ``
    ///
    /// `severity == None` (wiring-style severity-less findings) drops the label
    /// and uses the warning glyph (`⚠` rich / `!` plain). The trailing blank
    /// line is included so callers can `print!` blocks back-to-back.
    pub fn detail_block(
        &self,
        severity: Option<Severity>,
        code: &str,
        path: &str,
        message: &str,
    ) -> String {
        let mut out = String::new();

        // Line 1: glyph + (severity label) + code
        out.push_str("  ");
        let (glyph_state, glyph) = match severity {
            Some(Severity::High) => (Status::Error, self.state_glyph(Status::Error)),
            Some(Severity::Medium) | None => (Status::Warning, self.state_glyph(Status::Warning)),
        };
        out.push_str(&self.colorize_state(glyph_state, glyph));
        out.push_str("  ");
        if let Some(severity) = severity {
            out.push_str(&self.severity_label(severity));
            out.push_str("  ");
        }
        out.push_str(&self.strong(code));
        out.push('\n');

        // Line 2: path (muted, clickable)
        out.push_str("  ");
        out.push_str(&self.muted(path));
        out.push('\n');

        // Body block: leading blank separator + indented message, only when non-empty.
        if !message.is_empty() {
            out.push('\n');
            for line in message.lines() {
                out.push_str("    ");
                out.push_str(line);
                out.push('\n');
            }
        }

        // Trailing blank line so adjacent blocks visually separate.
        out.push('\n');
        out
    }

    /// Render a section heading for a fixed English title (Workflows / Actions
    /// / Findings / Members / etc.). Title is uppercased + bold so multiple
    /// blocks in the same command output get visual rhythm without competing
    /// with `status_header` glyphs or the trace tree's `╭─` connectors.
    ///
    /// For path-shaped titles (e.g. `.github/workflows/ci.yaml` flowing in
    /// from `callers`'s dynamic input), use `section_path` instead — it
    /// preserves the original casing because uppercasing a workflow path
    /// looks visually wrong and conflicts with the project's lowercase
    /// `.yaml` convention.
    pub fn section(&self, title: &str) -> String {
        let upper = title.to_uppercase();
        if self.color {
            self.strong(&upper).into_owned()
        } else {
            upper
        }
    }

    /// Render a section heading for a path or other identifier where the
    /// original casing must be preserved. Bold-only — no uppercasing. See
    /// `section` for fixed-title usage.
    pub fn section_path(&self, path: &str) -> String {
        if self.color {
            self.strong(path).into_owned()
        } else {
            path.to_string()
        }
    }

    /// Render a bullet item under a section.
    ///
    /// Rich mode: `  <muted ·> <text>` (2-space indent; `·` lands directly
    /// under the bold title's first letter when the section above is rendered
    /// at col 0).
    /// Plain mode: `  - <text>`
    pub fn item(&self, text: impl AsRef<str>) -> String {
        if self.rich {
            format!("  {} {}", self.muted("·"), text.as_ref())
        } else {
            format!("  - {}", text.as_ref())
        }
    }

    /// Render a `[kind]` tag bracket used in tree label tag columns.
    /// Brackets stay dim; the kind token itself takes a per-category accent
    /// color so a tree with mixed local / external / docker / cycle nodes
    /// gains visual rhythm beyond the monochrome connector grid.
    pub fn tag_bracket(&self, kind: KindTag<'_>) -> String {
        let bracket_style = Style::new().dimmed();
        let kind_style = kind_color(kind)
            .map(|color| bracket_style.fg_color(Some(color.into())))
            .unwrap_or(bracket_style);
        let kind = kind.as_str();
        format!(
            "{}{}{}",
            self.style_text(bracket_style, "["),
            self.style_text(kind_style, kind),
            self.style_text(bracket_style, "]"),
        )
    }

    /// Render a tree label name with bold + the kind's accent color. Dangling
    /// annotations and cycle nodes override the kind color with `danger` (red)
    /// so the hazard remains visually loud regardless of the kind palette.
    pub fn kind_styled_name(&self, name: &str, kind: KindTag<'_>, danger: bool) -> String {
        if danger {
            return self.danger(name).into_owned();
        }
        match kind_color(kind) {
            Some(color) => {
                let style = Style::new().bold().fg_color(Some(color.into()));
                self.style_text(style, name).into_owned()
            }
            None => self.strong(name).into_owned(),
        }
    }

    pub fn path(&self, root: &Path, path: &Path) -> String {
        let root_abs = absolutize(root);
        let path_abs = absolutize(path);
        let root_canonical_fallback = if root_abs == self.root_abs {
            None
        } else {
            root_abs.canonicalize().ok()
        };
        let root_canonical = if root_abs == self.root_abs {
            self.root_canonical.as_deref()
        } else {
            root_canonical_fallback.as_deref()
        };
        let rel = path
            .strip_prefix(root)
            .ok()
            .or_else(|| path_abs.strip_prefix(&root_abs).ok())
            .or_else(|| root_canonical.and_then(|root| path_abs.strip_prefix(root).ok()))
            .unwrap_or(path);
        normalize_path(rel)
    }

    pub fn table(&self, headers: &[&str], rows: &[Vec<String>]) -> String {
        if rows.is_empty() {
            return String::new();
        }
        let mut widths: Vec<usize> = headers.iter().map(|h| display_width(h)).collect();
        for row in rows {
            debug_assert_eq!(
                row.len(),
                headers.len(),
                "table row width must match header width"
            );
            for (i, cell) in row.iter().enumerate() {
                if let Some(width) = widths.get_mut(i) {
                    *width = (*width).max(display_width(cell));
                }
            }
        }

        let mut out = String::new();
        for (i, header) in headers.iter().enumerate() {
            if i > 0 {
                out.push_str("  ");
            }
            if i == headers.len() - 1 {
                out.push_str(&self.table_header(header));
            } else {
                out.push_str(&self.table_header(&pad(header, widths[i])));
            }
        }
        out.push('\n');

        for row in rows {
            for (i, cell) in row.iter().enumerate() {
                if i > 0 {
                    out.push_str("  ");
                }
                if i == row.len() - 1 {
                    out.push_str(cell);
                } else {
                    out.push_str(&pad(cell, widths[i]));
                }
            }
            out.push('\n');
        }
        out
    }

    /// State glyph for the unified header. Rich mode uses Unicode
    /// (`✓ / • / ⚠ / ✗`); plain mode falls back to ASCII (`OK / * / ! / X`)
    /// so the glyph survives terminals that mangle Unicode. Plain mode does
    /// not currently emit a glyph in `status_header` (kept for grep stability),
    /// but the helper is shared with `detail_block` which always emits one.
    fn state_glyph(&self, state: Status) -> &'static str {
        match (state, self.rich) {
            (Status::Clean, true) => "✓",
            (Status::Clean, false) => "OK",
            (Status::Found, true) => "•",
            (Status::Found, false) => "*",
            (Status::Warning, true) => "⚠",
            (Status::Warning, false) => "!",
            (Status::Error, true) => "✗",
            (Status::Error, false) => "X",
        }
    }

    fn colorize_state<'a>(&self, state: Status, text: &'a str) -> Cow<'a, str> {
        match state {
            Status::Clean => self.success(text),
            Status::Found => self.strong(text),
            Status::Warning => self.warning(text),
            Status::Error => self.danger(text),
        }
    }

    pub fn severity_label(&self, severity: Severity) -> String {
        let label = match severity {
            Severity::High => "high",
            Severity::Medium => "medium",
        };
        match severity {
            Severity::High => self.danger(label).into_owned(),
            Severity::Medium => self.warning(label).into_owned(),
        }
    }

    pub fn muted<'a>(&self, text: &'a str) -> Cow<'a, str> {
        self.style_text(Style::new().dimmed(), text)
    }

    pub fn strong<'a>(&self, text: &'a str) -> Cow<'a, str> {
        self.style_text(Style::new().bold(), text)
    }

    pub fn danger<'a>(&self, text: &'a str) -> Cow<'a, str> {
        self.style_text(
            Style::new().fg_color(Some(AnsiColor::Red.into())).bold(),
            text,
        )
    }

    pub fn warning<'a>(&self, text: &'a str) -> Cow<'a, str> {
        self.style_text(
            Style::new().fg_color(Some(AnsiColor::Yellow.into())).bold(),
            text,
        )
    }

    pub fn success<'a>(&self, text: &'a str) -> Cow<'a, str> {
        self.style_text(AnsiColor::Green.on_default(), text)
    }

    pub fn table_header<'a>(&self, text: &'a str) -> Cow<'a, str> {
        self.muted(text)
    }

    pub fn style_text<'a>(&self, style: Style, text: &'a str) -> Cow<'a, str> {
        if self.color {
            Cow::Owned(format!("{style}{text}{style:#}"))
        } else {
            Cow::Borrowed(text)
        }
    }
}

#[cfg(test)]
impl Ui {
    pub(crate) fn plain_for_test() -> Self {
        Self {
            color: false,
            rich: false,
            root_abs: PathBuf::from("."),
            root_canonical: None,
        }
    }

    pub(crate) fn color_for_test() -> Self {
        Self {
            color: true,
            rich: false,
            root_abs: PathBuf::from("."),
            root_canonical: None,
        }
    }

    pub(crate) fn rich_color_for_test() -> Self {
        Self {
            color: true,
            rich: true,
            root_abs: PathBuf::from("."),
            root_canonical: None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Status {
    Clean,
    Found,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    High,
    Medium,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KindTag<'a> {
    Workflow,
    Action,
    ExternalWorkflow,
    ExternalAction,
    Docker,
    Annotation,
    Cycle,
    Unknown(&'a str),
}

impl<'a> KindTag<'a> {
    pub fn as_str(self) -> &'a str {
        match self {
            Self::Workflow => "wf",
            Self::Action => "ac",
            Self::ExternalWorkflow => "ext-wf",
            Self::ExternalAction => "ext-ac",
            Self::Docker => "docker",
            Self::Annotation => "ann",
            Self::Cycle => "cyc",
            Self::Unknown(kind) => kind,
        }
    }
}

pub fn plural(count: usize, singular: &str, plural: &str) -> String {
    if count == 1 {
        format!("1 {singular}")
    } else {
        format!("{count} {plural}")
    }
}

pub fn severity_breakdown(high: usize, medium: usize) -> Vec<String> {
    let mut summary = Vec::new();
    if high > 0 {
        summary.push(format!("{high} high"));
    }
    if medium > 0 {
        summary.push(format!("{medium} medium"));
    }
    summary
}

pub fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn absolutize(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

fn pad(text: &str, width: usize) -> String {
    let padding = width.saturating_sub(display_width(text));
    format!("{text}{}", " ".repeat(padding))
}

fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

/// Map a tree-label kind tag (`wf` / `ext-wf` / `ac` / `ext-ac` / `docker` /
/// `ann` / `cyc`) to its 8-color ANSI accent. Local entities and annotations
/// share cyan; external entities share magenta; docker images get yellow; and
/// the cycle hazard is red. Unknown kinds return `None` so callers fall back
/// to the default dim/bold styling.
fn kind_color(kind: KindTag<'_>) -> Option<AnsiColor> {
    match kind {
        KindTag::Workflow | KindTag::Action | KindTag::Annotation => Some(AnsiColor::Cyan),
        KindTag::ExternalWorkflow | KindTag::ExternalAction => Some(AnsiColor::Magenta),
        KindTag::Docker => Some(AnsiColor::Yellow),
        KindTag::Cycle => Some(AnsiColor::Red),
        KindTag::Unknown(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_ansi(text: &str) -> bool {
        text.contains("\u{1b}[")
    }

    #[test]
    fn status_header_plain_mode_omits_glyph_and_separator() {
        let ui = Ui::plain_for_test();
        let out = ui.status_header(
            "permissions",
            Status::Error,
            "15 findings",
            &["0 high".into(), "15 medium".into()],
        );
        assert!(!has_ansi(&out), "must not contain ANSI: {out:?}");
        for glyph in ["✓", "•", "⚠", "✗"] {
            assert!(
                !out.contains(glyph),
                "plain mode must not contain glyph {glyph}: {out:?}"
            );
        }
        assert!(
            !out.contains('·'),
            "plain mode must not contain `·` separator: {out:?}"
        );
        assert!(
            !out.contains('='),
            "plain mode must not contain `=` separator (descriptors live inside parens summary now): {out:?}"
        );
        assert!(
            out.contains("permissions  15 findings"),
            "expected command + primary in 2-space form: {out:?}"
        );
        assert!(
            out.contains("(0 high, 15 medium)"),
            "expected parens summary: {out:?}"
        );
    }

    #[test]
    fn status_header_rich_mode_emits_glyph_at_line_head() {
        let ui = Ui::rich_color_for_test();
        let out = ui.status_header("build", Status::Clean, "done", &[]);
        // Rich format starts with `<glyph>  <bold cmd>   <primary>`. After
        // stripping ANSI escape sequences (CSI form `\x1b[<params>m`), the
        // very first character must be the glyph — the glyph appears before
        // the command name.
        let stripped = strip_ansi(&out);
        let glyph_idx = stripped.find('✓').expect("glyph present");
        let build_idx = stripped.find("build").expect("command present");
        assert!(
            glyph_idx < build_idx,
            "glyph must appear before command name: stripped={stripped:?} raw={out:?}"
        );
        assert!(out.contains("done"), "expected primary: {out:?}");
        assert!(has_ansi(&out), "expected ANSI styling: {out:?}");
    }

    fn strip_ansi(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\u{1b}' {
                // CSI sequence: skip until and including the final `m`.
                if chars.peek() == Some(&'[') {
                    chars.next();
                    while let Some(&nc) = chars.peek() {
                        chars.next();
                        if nc == 'm' {
                            break;
                        }
                    }
                }
                continue;
            }
            out.push(c);
        }
        out
    }

    #[test]
    fn status_header_rich_mode_wraps_summary_in_parens() {
        let ui = Ui::rich_color_for_test();
        let out = ui.status_header(
            "permissions",
            Status::Error,
            "1 finding",
            &["1 medium".into()],
        );
        assert!(
            out.contains("(1 medium)"),
            "rich mode summary must be parens-wrapped: {out:?}"
        );
        assert!(
            !out.contains('·'),
            "rich mode must not use the `·` separator anymore: {out:?}"
        );
        assert!(
            !out.contains("medium=1"),
            "rich mode must not use `=` k/v form anymore: {out:?}"
        );
    }

    #[test]
    fn status_header_empty_summary_omits_parens() {
        let plain = Ui::plain_for_test();
        let out = plain.status_header("build", Status::Clean, "done", &[]);
        assert!(
            !out.contains('('),
            "empty summary must omit parens entirely: {out:?}"
        );
        assert!(
            !out.contains(')'),
            "empty summary must omit parens entirely: {out:?}"
        );
    }

    #[test]
    fn status_header_emits_correct_glyph_per_state() {
        let ui = Ui::rich_color_for_test();
        for (state, glyph) in [
            (Status::Clean, "✓"),
            (Status::Found, "•"),
            (Status::Warning, "⚠"),
            (Status::Error, "✗"),
        ] {
            let out = ui.status_header("cmd", state, "primary", &[]);
            assert!(
                out.contains(glyph),
                "expected glyph {glyph} for {state:?}: {out:?}"
            );
        }
    }

    #[test]
    fn semantic_styles_emit_ansi_only_when_color_enabled() {
        let plain = Ui::plain_for_test();
        assert_eq!(plain.muted("meta"), "meta");
        assert_eq!(plain.danger("bad"), "bad");

        let color = Ui::color_for_test();
        for styled in [
            color.muted("meta"),
            color.strong("target"),
            color.danger("bad"),
            color.warning("warn"),
            color.success("ok"),
        ] {
            assert!(has_ansi(&styled), "expected ANSI in {styled:?}");
            assert!(
                styled.ends_with("\u{1b}[0m"),
                "expected reset after styled text: {styled:?}"
            );
        }
    }

    #[test]
    fn severity_breakdown_omits_zero_count_tiers() {
        assert_eq!(severity_breakdown(0, 0), Vec::<String>::new());
        assert_eq!(severity_breakdown(2, 0), vec!["2 high"]);
        assert_eq!(severity_breakdown(0, 3), vec!["3 medium"]);
        assert_eq!(severity_breakdown(1, 4), vec!["1 high", "4 medium"]);
    }

    #[test]
    fn table_styles_headers_after_width_calculation() {
        let ui = Ui::color_for_test();
        let out = ui.table(
            &["kind", "target"],
            &[vec!["workflow".into(), ".github/workflows/ci.yml".into()]],
        );
        let mut lines = out.lines();
        let header = lines.next().expect("header");
        let row = lines.next().expect("row");
        assert!(has_ansi(header), "expected styled header: {header:?}");
        assert!(
            !has_ansi(row),
            "rows should remain raw unless callers style them deliberately: {row:?}"
        );
        assert!(
            row.starts_with("workflow  .github/workflows/ci.yml"),
            "row alignment should use raw cell widths: {row:?}"
        );
    }

    #[test]
    fn table_aligns_columns_by_unicode_display_width() {
        let ui = Ui::plain_for_test();
        let out = ui.table(
            &["target", "note"],
            &[
                vec!["界".into(), "wide".into()],
                vec!["ascii".into(), "plain".into()],
            ],
        );
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "target  note");
        assert_eq!(lines[1], "界      wide");
        assert_eq!(lines[2], "ascii   plain");
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "table row width must match header width")]
    fn table_debug_asserts_row_width_contract() {
        let ui = Ui::plain_for_test();
        let _ = ui.table(&["one"], &[vec!["one".into(), "two".into()]]);
    }

    #[test]
    fn section_plain_mode_uppercases_title() {
        let ui = Ui::plain_for_test();
        assert_eq!(ui.section("Workflows"), "WORKFLOWS");
        assert_eq!(ui.section("Findings"), "FINDINGS");
    }

    #[test]
    fn section_rich_mode_uppercases_and_bolds() {
        let ui = Ui::rich_color_for_test();
        let out = ui.section("Workflows");
        assert!(
            out.contains("WORKFLOWS"),
            "expected uppercased title: {out:?}"
        );
        // Old left-marker codepoint U+258E was used by the previous design.
        assert!(
            !out.contains('\u{258E}'),
            "rich section must not contain the old left-marker glyph: {out:?}"
        );
        assert!(
            !out.starts_with(' '),
            "rich section must start at column 0 (no leading space): {out:?}"
        );
        assert!(has_ansi(&out), "expected styled rich section: {out:?}");
    }

    #[test]
    fn section_path_preserves_original_casing() {
        let plain = Ui::plain_for_test();
        assert_eq!(
            plain.section_path(".github/workflows/ci.yaml"),
            ".github/workflows/ci.yaml",
            "path-shaped titles must NOT be uppercased"
        );
        let color = Ui::color_for_test();
        let styled = color.section_path(".github/workflows/ci.yaml");
        assert!(
            styled.contains(".github/workflows/ci.yaml"),
            "path text preserved: {styled:?}"
        );
        assert!(
            !styled.contains(".GITHUB"),
            "path must not be uppercased: {styled:?}"
        );
        assert!(has_ansi(&styled), "expected bold ANSI styling: {styled:?}");
    }

    #[test]
    fn item_plain_mode_uses_dash_marker() {
        let ui = Ui::plain_for_test();
        assert_eq!(
            ui.item(".github/workflows/ci.yml"),
            "  - .github/workflows/ci.yml"
        );
    }

    #[test]
    fn item_rich_mode_uses_dot_marker_with_2_space_indent() {
        let ui = Ui::rich_color_for_test();
        let out = ui.item(".github/workflows/ci.yml");
        assert!(out.starts_with("  "), "expected 2-space indent: {out:?}");
        assert!(
            !out.starts_with("    "),
            "must not have 4-space indent (regression to old layout): {out:?}"
        );
        assert!(out.contains("·"), "expected `·` marker: {out:?}");
        assert!(
            out.contains(".github/workflows/ci.yml"),
            "expected text: {out:?}"
        );
    }

    #[test]
    fn tag_bracket_emits_brackets_around_kind() {
        let plain = Ui::plain_for_test();
        assert_eq!(plain.tag_bracket(KindTag::Workflow), "[wf]");
        assert_eq!(plain.tag_bracket(KindTag::ExternalWorkflow), "[ext-wf]");

        let color = Ui::color_for_test();
        let styled = color.tag_bracket(KindTag::Action);
        assert!(styled.contains("ac"), "expected kind text: {styled:?}");
        assert!(styled.contains('['), "expected open bracket: {styled:?}");
        assert!(styled.contains(']'), "expected close bracket: {styled:?}");
        assert!(has_ansi(&styled), "expected ANSI styling: {styled:?}");
    }

    #[test]
    fn detail_blocks_two_line_layout_with_severity() {
        let plain = Ui::plain_for_test();
        let out = plain.detail_block(
            Some(Severity::High),
            "overly-broad-coarse",
            ".github/workflows/ci.yml",
            "workflow grants write-all",
        );
        assert!(
            !has_ansi(&out),
            "plain detail block must not contain ANSI: {out:?}"
        );
        let lines: Vec<&str> = out.lines().collect();
        // Line 0: "  X  high  overly-broad-coarse"
        // Line 1: "  .github/workflows/ci.yml"
        // Line 2: "" (blank)
        // Line 3: "    workflow grants write-all"
        // Line 4: "" (trailing blank)
        assert!(
            lines[0].starts_with("  X  high  overly-broad-coarse"),
            "severity row format unexpected: {:?}",
            lines.first()
        );
        assert_eq!(
            lines[1],
            "  .github/workflows/ci.yml",
            "path row format unexpected: {:?}",
            lines.get(1)
        );
        assert_eq!(lines[2], "", "expected blank separator: {:?}", lines.get(2));
        assert_eq!(
            lines[3],
            "    workflow grants write-all",
            "message indent unexpected: {:?}",
            lines.get(3)
        );
    }

    #[test]
    fn detail_blocks_severity_less_uses_warning_glyph() {
        let plain = Ui::plain_for_test();
        let out = plain.detail_block(
            None,
            "dangling-local-uses",
            ".github/workflows/ci.yml:12",
            "`uses` target is missing",
        );
        let lines: Vec<&str> = out.lines().collect();
        assert!(
            lines[0].starts_with("  !  dangling-local-uses"),
            "severity-less row should use warning glyph and skip label: {:?}",
            lines.first()
        );
        assert_eq!(
            lines[1],
            "  .github/workflows/ci.yml:12",
            "path row format unexpected: {:?}",
            lines.get(1)
        );
    }

    #[test]
    fn detail_blocks_color_keeps_readable_tokens() {
        let color = Ui::color_for_test();
        let out = color.detail_block(
            Some(Severity::Medium),
            "implicit-repo-default",
            ".github/workflows/ci.yml",
            "jobs [test] inherit repo default",
        );
        assert!(has_ansi(&out), "expected ANSI styling: {out:?}");
        for token in [
            "implicit-repo-default",
            ".github/workflows/ci.yml",
            "jobs [test] inherit repo default",
        ] {
            assert!(
                out.contains(token),
                "expected token {token} to remain readable: {out:?}"
            );
        }
    }

    #[test]
    fn kind_color_maps_categories() {
        // Local entities (workflow / local action / annotation) → cyan.
        assert_eq!(kind_color(KindTag::Workflow), Some(AnsiColor::Cyan));
        assert_eq!(kind_color(KindTag::Action), Some(AnsiColor::Cyan));
        assert_eq!(kind_color(KindTag::Annotation), Some(AnsiColor::Cyan));
        // External entities → magenta.
        assert_eq!(
            kind_color(KindTag::ExternalWorkflow),
            Some(AnsiColor::Magenta)
        );
        assert_eq!(
            kind_color(KindTag::ExternalAction),
            Some(AnsiColor::Magenta)
        );
        // Special: docker yellow, cycle red.
        assert_eq!(kind_color(KindTag::Docker), Some(AnsiColor::Yellow));
        assert_eq!(kind_color(KindTag::Cycle), Some(AnsiColor::Red));
        // Unknown kinds fall back to None (caller uses default dim/bold).
        assert_eq!(kind_color(KindTag::Unknown("")), None);
        assert_eq!(kind_color(KindTag::Unknown("future-kind")), None);
    }

    #[test]
    fn tag_bracket_colors_kind_token_per_category() {
        let color = Ui::color_for_test();
        // Cyan ANSI prefix is "\u{1b}[36m" — appears on the kind token, not on
        // the surrounding brackets (those stay dimmed).
        let wf = color.tag_bracket(KindTag::Workflow);
        assert!(
            wf.contains("\u{1b}[36m") || wf.contains("\u{1b}[2;36m"),
            "expected cyan ANSI on `wf` kind: {wf:?}"
        );
        let ext_wf = color.tag_bracket(KindTag::ExternalWorkflow);
        assert!(
            ext_wf.contains("\u{1b}[35m") || ext_wf.contains("\u{1b}[2;35m"),
            "expected magenta ANSI on `ext-wf` kind: {ext_wf:?}"
        );
        let cyc = color.tag_bracket(KindTag::Cycle);
        assert!(
            cyc.contains("\u{1b}[31m") || cyc.contains("\u{1b}[2;31m"),
            "expected red ANSI on `cyc` kind: {cyc:?}"
        );
        let unknown = color.tag_bracket(KindTag::Unknown("future-kind"));
        // No category color, but brackets are still dimmed.
        assert!(unknown.contains("future-kind"), "kind token preserved");
        assert!(has_ansi(&unknown), "ANSI dimming still applied");
    }

    #[test]
    fn tag_bracket_plain_mode_keeps_literal_brackets() {
        let plain = Ui::plain_for_test();
        // Plain mode strips all ANSI; the bracket form must still be readable
        // verbatim.
        assert_eq!(plain.tag_bracket(KindTag::Workflow), "[wf]");
        assert_eq!(plain.tag_bracket(KindTag::ExternalWorkflow), "[ext-wf]");
        assert_eq!(plain.tag_bracket(KindTag::Cycle), "[cyc]");
        assert_eq!(
            plain.tag_bracket(KindTag::Unknown("future-kind")),
            "[future-kind]"
        );
    }

    #[test]
    fn kind_styled_name_applies_bold_plus_kind_color() {
        let color = Ui::color_for_test();
        let wf_name = color.kind_styled_name(".github/workflows/ci.yaml", KindTag::Workflow, false);
        assert!(
            wf_name.contains("\u{1b}[1") && wf_name.contains("36"),
            "expected bold + cyan on workflow name: {wf_name:?}"
        );
        let ext = color.kind_styled_name(
            "acme/shared/.github/workflows/x.yml",
            KindTag::ExternalWorkflow,
            false,
        );
        assert!(
            ext.contains("35"),
            "expected magenta on external workflow name: {ext:?}"
        );
        let docker = color.kind_styled_name("alpine", KindTag::Docker, false);
        assert!(
            docker.contains("33"),
            "expected yellow on docker image name: {docker:?}"
        );
    }

    #[test]
    fn kind_styled_name_danger_overrides_kind_color() {
        let color = Ui::color_for_test();
        // dangling annotation: kind = "ann", danger = true. Must render red.
        let dangling = color.kind_styled_name("missing.yml", KindTag::Annotation, true);
        assert!(
            dangling.contains("31"),
            "danger=true must paint name red regardless of kind: {dangling:?}"
        );
        // cycle: kind = "cyc" (already red mapping), danger = true. Stays red.
        let cycle = color.kind_styled_name(".github/workflows/loop.yml", KindTag::Cycle, true);
        assert!(cycle.contains("31"), "cycle danger stays red: {cycle:?}");
    }

    #[test]
    fn kind_styled_name_unknown_kind_falls_back_to_bold_only() {
        let color = Ui::color_for_test();
        let out = color.kind_styled_name("name", KindTag::Unknown("future-kind"), false);
        assert!(out.contains("\u{1b}[1m"), "expected bold ANSI: {out:?}");
        // No category color codes (cyan / magenta / yellow / red).
        for code in ["36m", "35m", "33m", "31m"] {
            assert!(
                !out.contains(code),
                "unexpected color {code} for unknown kind: {out:?}"
            );
        }
    }

    #[test]
    fn kind_styled_name_plain_mode_drops_ansi() {
        let plain = Ui::plain_for_test();
        let out = plain.kind_styled_name(".github/workflows/ci.yaml", KindTag::Workflow, false);
        assert_eq!(out, ".github/workflows/ci.yaml");
        assert!(!has_ansi(&out));
    }
}
