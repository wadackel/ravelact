use crate::ir::*;
use crate::parser::annotations::{
    attach_annotations, collect_block_scalar_ranges, scan_ravelact_comments,
};
use anyhow::{anyhow, Context, Result};
use saphyr::{LoadableYamlNode, MarkedYaml};
use std::path::Path;

mod helpers;
mod jobs_steps;
mod triggers;
mod uses;

#[cfg(test)]
mod tests;

// Re-export the items consumed outside `parser::workflow` (currently
// `parser::action` and `ir::build`) so external callers keep a stable path.
pub(crate) use helpers::{as_str, get_field, path_to_forward};
pub(crate) use jobs_steps::parse_steps;
pub(crate) use triggers::{parse_input_decls, parse_output_decls};

use helpers::{
    parse_concurrency, parse_defaults, parse_permissions, parse_string_map, rel_id, string_field,
};
use jobs_steps::parse_jobs;
use triggers::parse_on;

/// Parse a workflow YAML file at `path` into a [`Workflow`] IR node along with
/// any non-fatal diagnostics surfaced during parsing.
pub fn parse_workflow(path: &Path, root: &Path) -> Result<(Workflow, Vec<ParseDiagnostic>)> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("read workflow {}", path.display()))?;
    let mut docs = MarkedYaml::load_from_str(&raw)
        .with_context(|| format!("parse YAML {}", path.display()))?;
    let doc = docs
        .pop()
        .ok_or_else(|| anyhow!("empty YAML document: {}", path.display()))?;

    let id = WorkflowId(rel_id(path, root)?);
    let source = SourcePos {
        file: path.to_path_buf(),
        line: Some(doc.span.start.line()),
    };

    let name = string_field(&doc, "name");
    let defaults = parse_defaults(get_field(&doc, "defaults"));
    let env = parse_string_map(get_field(&doc, "env"));

    let concurrency = parse_concurrency(get_field(&doc, "concurrency"));
    let run_name = string_field(&doc, "run-name");

    let mut diagnostics: Vec<ParseDiagnostic> = Vec::new();
    let permissions = parse_permissions(get_field(&doc, "permissions"), &mut diagnostics, path);
    let jobs = parse_jobs(get_field(&doc, "jobs"), &id, &mut diagnostics, path)?;

    let triggers = parse_on(get_field(&doc, "on"), path, &mut diagnostics);

    let mut wf = Workflow {
        id,
        source,
        name,
        run_name,
        triggers,
        jobs,
        permissions,
        defaults,
        env,
        concurrency,
        annotations: Vec::new(),
    };

    let mut scalar_ranges: Vec<(usize, usize)> = Vec::new();
    collect_block_scalar_ranges(&doc, &mut scalar_ranges);
    let raws = scan_ravelact_comments(&raw, path, &scalar_ranges, &mut diagnostics);
    attach_annotations(&mut wf, raws, &mut diagnostics);

    Ok((wf, diagnostics))
}
