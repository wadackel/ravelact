//! `ravelact skill [<topic>]`: agent-facing guidance embedded in the binary,
//! so it always matches the installed version. The published
//! `skills/ravelact/SKILL.md` is a thin wrapper that points agents here.

use clap::ValueEnum;

const OVERVIEW: &str = include_str!("skill/overview.md");

/// A page printed by `ravelact skill <topic>`. Command topics are named after
/// their subcommand (a test in `cli/mod.rs` keeps the two sets equal); guide
/// topics cover material that spans commands. Each variant's doc comment is
/// its one-line summary in the topic list, `ravelact skill --help`, and shell
/// completion.
#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Topic {
    /// Forward walk from a trigger event (push, pull_request, ...)
    Trace,
    /// Summarize trigger events declared by workflows
    Triggers,
    /// List call sites that reference a workflow / action
    Callers,
    /// Reverse impact: which entry-points are affected by changed files
    Impact,
    /// Declared-but-unused report (workflows / actions / inputs / outputs)
    Orphans,
    /// Effective permissions scope across caller -> callee chains
    Permissions,
    /// Secrets propagation across reusable-workflow chains
    Secrets,
    /// Workflow dependency wiring consistency
    Wiring,
    /// Composite-action extraction candidates from duplicated step sequences
    Extract,
    /// Near-duplicate workflow clusters
    Dedup,
    /// Print IR as JSON
    Dump,
    /// Render the call graph as Mermaid
    Graph,
    /// Launch a local server that renders the workflow graph in a browser
    Browse,
    /// Build the IR and persist the cache
    Build,
    /// Generate a shell completion setup snippet
    Completion,
    /// Overlay SARIF findings (zizmor / actionlint) onto the call graph
    Findings,
    /// CI recipes that combine several commands
    Recipes,
}

impl Topic {
    pub(super) fn is_guide(self) -> bool {
        matches!(self, Topic::Findings | Topic::Recipes)
    }

    fn body(self) -> &'static str {
        match self {
            Topic::Trace => include_str!("skill/trace.md"),
            Topic::Triggers => include_str!("skill/triggers.md"),
            Topic::Callers => include_str!("skill/callers.md"),
            Topic::Impact => include_str!("skill/impact.md"),
            Topic::Orphans => include_str!("skill/orphans.md"),
            Topic::Permissions => include_str!("skill/permissions.md"),
            Topic::Secrets => include_str!("skill/secrets.md"),
            Topic::Wiring => include_str!("skill/wiring.md"),
            Topic::Extract => include_str!("skill/extract.md"),
            Topic::Dedup => include_str!("skill/dedup.md"),
            Topic::Dump => include_str!("skill/dump.md"),
            Topic::Graph => include_str!("skill/graph.md"),
            Topic::Browse => include_str!("skill/browse.md"),
            Topic::Build => include_str!("skill/build.md"),
            Topic::Completion => include_str!("skill/completion.md"),
            Topic::Findings => include_str!("skill/findings.md"),
            Topic::Recipes => include_str!("skill/recipes.md"),
        }
    }
}

pub(super) fn run(topic: Option<Topic>) {
    print!("{}", render(topic));
}

fn render(topic: Option<Topic>) -> String {
    match topic {
        Some(topic) => topic.body().to_string(),
        None => overview(),
    }
}

/// The topic list is generated from `Topic` rather than written into
/// `overview.md`, so adding a topic cannot leave the list stale.
fn overview() -> String {
    let mut out = String::from(OVERVIEW);
    out.push_str("\n## Topics\n\nRun `ravelact skill <topic>` to read one.\n");
    for (heading, guides) in [("Commands", false), ("Guides", true)] {
        out.push_str(&format!("\n### {heading}\n\n"));
        for value in Topic::value_variants()
            .iter()
            .filter(|t| t.is_guide() == guides)
            .filter_map(|t| t.to_possible_value())
        {
            let help = value
                .get_help()
                .map(ToString::to_string)
                .unwrap_or_default();
            out.push_str(&format!("- `{}` — {help}\n", value.get_name()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_topic_body_starts_with_a_top_level_heading() {
        for topic in Topic::value_variants() {
            let body = render(Some(*topic));
            assert!(body.starts_with("# "), "{topic:?}: {body}");
            assert!(body.ends_with('\n') && !body.ends_with("\n\n"), "{topic:?}");
        }
    }

    #[test]
    fn overview_lists_every_topic_with_its_summary() {
        let out = render(None);
        assert!(out.starts_with(OVERVIEW));
        for topic in Topic::value_variants() {
            let value = topic.to_possible_value().unwrap();
            let help = value.get_help().unwrap();
            let line = format!("- `{}` — {help}\n", value.get_name());
            assert!(out.contains(&line), "missing {line:?}");
        }
        let commands = out.find("### Commands").unwrap();
        let guides = out.find("### Guides").unwrap();
        assert!(commands < out.find("- `trace`").unwrap());
        assert!(guides < out.find("- `findings`").unwrap());
    }
}
