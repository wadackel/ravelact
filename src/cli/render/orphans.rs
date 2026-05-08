use crate::cache;
use crate::query::{self, orphans::OrphanResult};
use crate::ui::{self, Status, Ui};
use anyhow::Result;
use globset::GlobSet;
use std::fmt::Write as _;

use crate::cli::{action_kind_label, build_or_load, OutputFormat};

pub(in crate::cli) fn run(
    root: &std::path::Path,
    cache_mode: cache::CacheMode,
    excludes: &GlobSet,
    format: &OutputFormat,
    ui: &Ui,
) -> Result<()> {
    let ir = build_or_load(root, cache_mode, excludes)?;
    let result = query::orphans::orphans(&ir);
    print!("{}", render_result(result, format, ui)?);
    Ok(())
}

fn render_result(result: OrphanResult, format: &OutputFormat, ui: &Ui) -> Result<String> {
    let OrphanResult {
        unused_workflows,
        unused_actions,
        unreferenced_inputs,
        unused_outputs,
    } = result;
    let mut out = String::new();
    match format {
        OutputFormat::Markdown => {
            writeln!(out, "### Orphans")?;
            writeln!(out)?;
            if unused_workflows.is_empty()
                && unused_actions.is_empty()
                && unreferenced_inputs.is_empty()
                && unused_outputs.is_empty()
            {
                writeln!(out, "No unused declarations found.")?;
            } else {
                let total = unused_workflows.len()
                    + unused_actions.len()
                    + unreferenced_inputs.len()
                    + unused_outputs.len();
                writeln!(
                    out,
                    "{} found: {}, {}, {}, {}.",
                    ui::plural(total, "unused declaration", "unused declarations"),
                    ui::plural(unused_workflows.len(), "workflow", "workflows"),
                    ui::plural(unused_actions.len(), "local action", "local actions"),
                    ui::plural(unreferenced_inputs.len(), "input", "inputs"),
                    ui::plural(unused_outputs.len(), "output", "outputs"),
                )?;
                writeln!(out)?;
                writeln!(out, "| Kind | Target | Detail |")?;
                writeln!(out, "|---|---|---|")?;
                for wf in &unused_workflows {
                    writeln!(out, "| reusable-workflow | `{}` | unused |", wf.0)?;
                }
                for (id, kind) in &unused_actions {
                    writeln!(
                        out,
                        "| local-action-{} | `{}` | unused |",
                        action_kind_label(kind),
                        id.0
                    )?;
                }
                for (target, name) in &unreferenced_inputs {
                    writeln!(out, "| input | `{target}` | `{name}` |")?;
                }
                for (target, name) in &unused_outputs {
                    writeln!(out, "| output | `{target}` | `{name}` |")?;
                }
            }
        }
        OutputFormat::Json => {
            // shape: {"workflows": [...],
            //         "actions": [{"id": "...", "kind": "composite|javascript|docker"}, ...],
            //         "unreferenced_inputs": [[target, name], ...],
            //         "unused_outputs":      [[target, name], ...]}
            // All four arrays are always present (empty when no findings).
            let actions_json: Vec<serde_json::Value> = unused_actions
                .iter()
                .map(|(id, kind)| {
                    serde_json::json!({
                        "id": &id.0,
                        "kind": action_kind_label(kind),
                    })
                })
                .collect();
            let payload = serde_json::json!({
                "workflows": unused_workflows.iter().map(|w| &w.0).collect::<Vec<_>>(),
                "actions": actions_json,
                "unreferenced_inputs": &unreferenced_inputs,
                "unused_outputs": &unused_outputs,
            });
            writeln!(out, "{}", serde_json::to_string_pretty(&payload)?)?;
        }
        OutputFormat::Text => {
            if unused_workflows.is_empty()
                && unused_actions.is_empty()
                && unreferenced_inputs.is_empty()
                && unused_outputs.is_empty()
            {
                writeln!(
                    out,
                    "{}",
                    ui.status_header("orphans", Status::Clean, "no unused declarations", &[])
                )?;
                return Ok(out);
            }
            let total = unused_workflows.len()
                + unused_actions.len()
                + unreferenced_inputs.len()
                + unused_outputs.len();
            let mut summary: Vec<String> = Vec::new();
            if !unused_workflows.is_empty() {
                summary.push(format!("{} workflows", unused_workflows.len()));
            }
            if !unused_actions.is_empty() {
                summary.push(format!("{} actions", unused_actions.len()));
            }
            if !unreferenced_inputs.is_empty() {
                summary.push(format!("{} inputs", unreferenced_inputs.len()));
            }
            if !unused_outputs.is_empty() {
                summary.push(format!("{} outputs", unused_outputs.len()));
            }
            writeln!(
                out,
                "{}",
                ui.status_header(
                    "orphans",
                    Status::Found,
                    ui::plural(total, "unused declaration", "unused declarations"),
                    &summary,
                )
            )?;
            writeln!(out)?;
            if !unused_workflows.is_empty() {
                writeln!(out, "{}", ui.section("Workflows"))?;
                for wf in unused_workflows {
                    writeln!(out, "{}", ui.item(wf.0))?;
                }
            }
            if !unused_actions.is_empty() {
                writeln!(out, "{}", ui.section("Actions"))?;
                let rows: Vec<Vec<String>> = unused_actions
                    .into_iter()
                    .map(|(id, kind)| vec![action_kind_label(&kind).into(), id.0])
                    .collect();
                write!(out, "{}", ui.table(&["kind", "target"], &rows))?;
            }
            if !unreferenced_inputs.is_empty() {
                writeln!(out, "{}", ui.section("Inputs"))?;
                let rows: Vec<Vec<String>> = unreferenced_inputs
                    .into_iter()
                    .map(|(target, input)| vec![target, input])
                    .collect();
                write!(out, "{}", ui.table(&["target", "input"], &rows))?;
            }
            if !unused_outputs.is_empty() {
                writeln!(out, "{}", ui.section("Outputs"))?;
                let rows: Vec<Vec<String>> = unused_outputs
                    .into_iter()
                    .map(|(target, output)| vec![target, output])
                    .collect();
                write!(out, "{}", ui.table(&["target", "output"], &rows))?;
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{ActionId, ActionKind, WorkflowId};
    use crate::ui::ColorMode;
    use std::path::Path;

    fn ui() -> Ui {
        Ui::from_env(ColorMode::Never, Path::new("."))
    }

    fn empty_result() -> OrphanResult {
        OrphanResult {
            unused_workflows: vec![],
            unused_actions: vec![],
            unreferenced_inputs: vec![],
            unused_outputs: vec![],
        }
    }

    fn full_result() -> OrphanResult {
        OrphanResult {
            unused_workflows: vec![WorkflowId(".github/workflows/reuse.yml".into())],
            unused_actions: vec![
                (
                    ActionId(".github/actions/build".into()),
                    ActionKind::Composite,
                ),
                (
                    ActionId(".github/actions/notify".into()),
                    ActionKind::JavaScript {
                        node_version: "20".into(),
                    },
                ),
            ],
            unreferenced_inputs: vec![(
                ".github/workflows/reuse.yml".to_string(),
                "config-path".to_string(),
            )],
            unused_outputs: vec![(
                ".github/actions/build".to_string(),
                "artifact-path".to_string(),
            )],
        }
    }

    #[test]
    fn markdown_empty_result_renders_no_findings_message() {
        let out =
            render_result(empty_result(), &OutputFormat::Markdown, &ui()).expect("render markdown");

        assert_eq!(out, "### Orphans\n\nNo unused declarations found.\n");
    }

    #[test]
    fn markdown_non_empty_result_renders_summary_and_all_kinds() {
        let out =
            render_result(full_result(), &OutputFormat::Markdown, &ui()).expect("render markdown");

        assert_eq!(
            out,
            "\
### Orphans

5 unused declarations found: 1 workflow, 2 local actions, 1 input, 1 output.

| Kind | Target | Detail |
|---|---|---|
| reusable-workflow | `.github/workflows/reuse.yml` | unused |
| local-action-composite | `.github/actions/build` | unused |
| local-action-javascript | `.github/actions/notify` | unused |
| input | `.github/workflows/reuse.yml` | `config-path` |
| output | `.github/actions/build` | `artifact-path` |
"
        );
    }

    #[test]
    fn json_empty_result_emits_all_four_arrays() {
        let out = render_result(empty_result(), &OutputFormat::Json, &ui()).expect("render json");

        assert_eq!(
            out,
            "\
{
  \"actions\": [],
  \"unreferenced_inputs\": [],
  \"unused_outputs\": [],
  \"workflows\": []
}
"
        );
    }

    #[test]
    fn json_non_empty_result_emits_all_four_arrays() {
        let out = render_result(full_result(), &OutputFormat::Json, &ui()).expect("render json");

        assert_eq!(
            out,
            "\
{
  \"actions\": [
    {
      \"id\": \".github/actions/build\",
      \"kind\": \"composite\"
    },
    {
      \"id\": \".github/actions/notify\",
      \"kind\": \"javascript\"
    }
  ],
  \"unreferenced_inputs\": [
    [
      \".github/workflows/reuse.yml\",
      \"config-path\"
    ]
  ],
  \"unused_outputs\": [
    [
      \".github/actions/build\",
      \"artifact-path\"
    ]
  ],
  \"workflows\": [
    \".github/workflows/reuse.yml\"
  ]
}
"
        );
    }

    #[test]
    fn text_empty_result_renders_clean_status() {
        let out = render_result(empty_result(), &OutputFormat::Text, &ui()).expect("render text");

        assert_eq!(out, "orphans  no unused declarations\n");
    }

    #[test]
    fn text_non_empty_result_renders_each_optional_section() {
        let out = render_result(full_result(), &OutputFormat::Text, &ui()).expect("render text");

        assert_eq!(
            out,
            "\
orphans  5 unused declarations  (1 workflows, 2 actions, 1 inputs, 1 outputs)

WORKFLOWS
  - .github/workflows/reuse.yml
ACTIONS
kind        target
composite   .github/actions/build
javascript  .github/actions/notify
INPUTS
target                       input
.github/workflows/reuse.yml  config-path
OUTPUTS
target                 output
.github/actions/build  artifact-path
"
        );
    }
}
