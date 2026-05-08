use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Diagnostic emitted by the parser layer. Used to surface non-fatal issues
/// (unknown ravelact verb, dangling reference, trailing comment fallback) without
/// writing to stderr from a library function — the CLI layer prints these.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParseDiagnostic {
    pub file: PathBuf,
    pub line: usize,
    pub message: String,
}

/// Structured comment annotation declaring an implicit dependency that the
/// YAML schema cannot express (e.g. `gh workflow run X` from a `run:` block,
/// `workflow_run` chain target).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Annotation {
    pub verb: AnnotationVerb,
    pub resolution: AnnotationResolution,
    /// 1-based line number of the comment in the source file.
    pub source_line: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AnnotationVerb {
    /// `# ravelact:dispatches <ref>` — same-trigger fan-out (e.g. `gh workflow run`).
    Dispatches,
    /// `# ravelact:triggers <ref>` — `workflow_run`-style chain: this workflow's
    /// completion is the trigger for `<ref>`.
    Triggers,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum AnnotationResolution {
    /// `<ref>` resolved to a known local workflow.
    Resolved { target: WorkflowId },
    /// `<ref>` could not be resolved (file does not exist, malformed path,
    /// unknown verb retained for diagnostics, ...). `raw_target` preserves the
    /// original token from the comment for surfacing to wiring and audit tools.
    Dangling { raw_target: String, reason: String },
}

/// One finding emitted by the `wiring` pass. Display strings are produced by
/// the CLI formatter from `kind` so we don't carry redundant message state.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WiringFinding {
    pub file: PathBuf,
    pub line: usize,
    #[serde(flatten)]
    pub kind: WiringKind,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind")]
pub enum WiringKind {
    /// A `gh workflow run X` invocation was found in a `run:` block but the
    /// step does not carry a matching `# ravelact:dispatches X` annotation.
    UnannotatedDispatch { raw_target: String },
    /// An `# ravelact:` annotation references something that could not be
    /// resolved to a local workflow.
    DanglingAnnotation { raw_target: String, reason: String },
    /// A `workflow_run.workflows: [Name]` entry could not be resolved to any
    /// local workflow by display name or path fallback. The declaring workflow
    /// is reported alongside the unresolvable name for user action.
    ///
    /// Per the GitHub Actions spec, `workflow_run.workflows` matches by the
    /// target workflow's `name:` field (or its path when `name:` is omitted).
    /// Spec source: Events that trigger workflows —
    /// https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows
    DanglingWorkflowRun {
        /// The unresolvable name string from `workflow_run.workflows`.
        raw_name: String,
    },
    /// A local `uses: ./<path>` reference (step-level action or job-level
    /// reusable workflow) was syntactically a local ref but the target
    /// directory / file is not present in the IR. Surfacing this as a
    /// wiring finding keeps the `graph` query deterministic and gives the
    /// user a typo-friendly diagnostic.
    DanglingLocalUses {
        /// `Action` for step-level `uses: ./.github/actions/<name>`,
        /// `Workflow` for job-level `uses: ./.github/workflows/<file>.yml`.
        local_kind: DanglingLocalUsesKind,
        /// The path token as written in YAML, with the leading `./` stripped.
        raw_target: String,
    },
}

/// Distinguishes a missing local action from a missing local reusable workflow.
/// Carried by [`WiringKind::DanglingLocalUses`].
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum DanglingLocalUsesKind {
    Action,
    Workflow,
}

/// Top-level IR. Built once per `ravelact build`, persisted to `${XDG_STATE_HOME}/ravelact/repo-<sha8>/cache.json` (or `$HOME/.local/state/...` when `XDG_STATE_HOME` is unset).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ir {
    pub schema_version: u32,
    pub root: PathBuf,
    pub workflows: Vec<Workflow>,
    pub actions: Vec<LocalAction>,
    pub external_actions: Vec<ExternalActionRef>,
}

/// Identifier of a local workflow file, expressed as a path relative to the IR root.
/// Always uses forward slashes for portability across platforms.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkflowId(pub String);

/// Identifier of a local action directory, expressed as a path relative to the IR root.
/// Always uses forward slashes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ActionId(pub String);

/// Identifier of a job within a workflow, corresponding to the job map key.
/// Must start with a letter or `_` and contain only alphanumeric characters, `-`, or `_`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JobId(pub String);

/// Identifier of a step within a job or composite action, corresponding to the step's `id:` field.
/// Optional in the YAML; absent steps carry no identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StepId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourcePos {
    pub file: PathBuf,
    /// MVP: line is best-effort. `None` until saphyr migration.
    pub line: Option<usize>,
}

/// `defaults:` block at workflow or job level.
/// Ref: Workflow syntax — https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#defaults
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Defaults {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<RunDefaults>,
}

/// `defaults.run:` settings (shell and working-directory).
/// Both fields are optional; when absent the runner picks platform defaults.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct RunDefaults {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<String>,
}

/// Concurrency group configuration.
///
/// Spec: https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-workflow-concurrency
///
/// Both scalar (`concurrency: my-group`) and map forms are supported at parse
/// time; scalar is collapsed to `{ group: "my-group", cancel_in_progress: None }`.
/// `cancel_in_progress: None` means the key was not present in YAML;
/// `Some(false)` means the user explicitly wrote `cancel-in-progress: false`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Concurrency {
    /// Raw group string or expression (e.g. `${{ github.workflow }}-${{ github.ref }}`).
    pub group: String,
    /// `None` = key absent (GitHub Actions default behaviour applies).
    /// `Some(b)` = user wrote an explicit boolean value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel_in_progress: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workflow {
    pub id: WorkflowId,
    pub source: SourcePos,
    pub name: Option<String>,
    /// Workflow-level `run-name:` expression string. Used by GitHub to name
    /// individual workflow runs in the UI and API. May reference `github` and
    /// `inputs` contexts. Additive field — older caches load fine via
    /// `#[serde(default)]`.
    ///
    /// Workflow syntax — https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#run-name
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_name: Option<String>,
    pub triggers: Vec<TriggerSpec>,
    pub jobs: Vec<Job>,
    pub permissions: Option<Permissions>,
    /// Workflow-level `defaults:` (shell / working-directory for all `run:` steps).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defaults: Option<Defaults>,
    /// Workflow-level `env:` map available to all job steps.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Workflow-level `concurrency:` block, if present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<Concurrency>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<Annotation>,
}

impl Workflow {
    /// Locate this workflow's `workflow_call` trigger payload, if any.
    /// `Workflow.inputs/outputs/secrets_required` used to be stored as separate
    /// fields cloned from the trigger; they are now derived directly so the
    /// trigger remains the canonical source.
    pub fn workflow_call_extras(&self) -> Option<(&[InputDecl], &[OutputDecl], &[SecretDecl])> {
        self.triggers.iter().find_map(|t| match &t.extras {
            Some(EventExtras::WorkflowCall {
                inputs,
                outputs,
                secrets,
            }) => Some((inputs.as_slice(), outputs.as_slice(), secrets.as_slice())),
            _ => None,
        })
    }

    pub fn inputs(&self) -> Option<&[InputDecl]> {
        self.workflow_call_extras().map(|(i, _, _)| i)
    }

    pub fn outputs(&self) -> Option<&[OutputDecl]> {
        self.workflow_call_extras().map(|(_, o, _)| o)
    }

    pub fn secrets_required(&self) -> Option<&[SecretDecl]> {
        self.workflow_call_extras().map(|(_, _, s)| s)
    }
}

/// Uniform trigger record. One per entry under `on:`.
///
/// `types: None` means the user omitted `types:`; `Some(vec)` means an explicit
/// list (including `Some(vec![])` for `types: []`). The distinction matters
/// because `pull_request` / `pull_request_target` have a default-active subset
/// when `types:` is omitted; explicit lists override that subset.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TriggerSpec {
    pub event: EventKind,
    #[serde(default, skip_serializing_if = "RefFilter::is_none")]
    pub branches: RefFilter,
    #[serde(default, skip_serializing_if = "RefFilter::is_none")]
    pub tags: RefFilter,
    #[serde(default, skip_serializing_if = "RefFilter::is_none")]
    pub paths: RefFilter,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub types: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extras: Option<EventExtras>,
}

/// Discriminated event identifier. `Other { name }` is preserved for forward
/// compatibility with new GitHub events that this crate has not modeled
/// explicitly. No tuple variants — serde's `tag = "kind"` rejects tuple
/// variants carrying primitives, so the forward-compat case must be a struct
/// variant (`Other { name }`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventKind {
    Push,
    PullRequest,
    PullRequestTarget,
    PullRequestReview,
    PullRequestReviewComment,
    Issues,
    IssueComment,
    Release,
    Discussion,
    DiscussionComment,
    Schedule,
    WorkflowDispatch,
    WorkflowCall,
    WorkflowRun,
    RepositoryDispatch,
    CheckRun,
    CheckSuite,
    MergeGroup,
    Milestone,
    Label,
    RegistryPackage,
    BranchProtectionRule,
    Watch,
    Other { name: String },
}

impl EventKind {
    /// Lowercase event identifier that appears under `on:` in YAML.
    pub fn name(&self) -> &str {
        match self {
            EventKind::Push => "push",
            EventKind::PullRequest => "pull_request",
            EventKind::PullRequestTarget => "pull_request_target",
            EventKind::PullRequestReview => "pull_request_review",
            EventKind::PullRequestReviewComment => "pull_request_review_comment",
            EventKind::Issues => "issues",
            EventKind::IssueComment => "issue_comment",
            EventKind::Release => "release",
            EventKind::Discussion => "discussion",
            EventKind::DiscussionComment => "discussion_comment",
            EventKind::Schedule => "schedule",
            EventKind::WorkflowDispatch => "workflow_dispatch",
            EventKind::WorkflowCall => "workflow_call",
            EventKind::WorkflowRun => "workflow_run",
            EventKind::RepositoryDispatch => "repository_dispatch",
            EventKind::CheckRun => "check_run",
            EventKind::CheckSuite => "check_suite",
            EventKind::MergeGroup => "merge_group",
            EventKind::Milestone => "milestone",
            EventKind::Label => "label",
            EventKind::RegistryPackage => "registry_package",
            EventKind::BranchProtectionRule => "branch_protection_rule",
            EventKind::Watch => "watch",
            EventKind::Other { name } => name.as_str(),
        }
    }

    /// Map an `on:` key string to the matching variant. Falls back to
    /// `Other { name }` for forward compatibility.
    pub fn from_name(name: &str) -> Self {
        match name {
            "push" => EventKind::Push,
            "pull_request" => EventKind::PullRequest,
            "pull_request_target" => EventKind::PullRequestTarget,
            "pull_request_review" => EventKind::PullRequestReview,
            "pull_request_review_comment" => EventKind::PullRequestReviewComment,
            "issues" => EventKind::Issues,
            "issue_comment" => EventKind::IssueComment,
            "release" => EventKind::Release,
            "discussion" => EventKind::Discussion,
            "discussion_comment" => EventKind::DiscussionComment,
            "schedule" => EventKind::Schedule,
            "workflow_dispatch" => EventKind::WorkflowDispatch,
            "workflow_call" => EventKind::WorkflowCall,
            "workflow_run" => EventKind::WorkflowRun,
            "repository_dispatch" => EventKind::RepositoryDispatch,
            "check_run" => EventKind::CheckRun,
            "check_suite" => EventKind::CheckSuite,
            "merge_group" => EventKind::MergeGroup,
            "milestone" => EventKind::Milestone,
            "label" => EventKind::Label,
            "registry_package" => EventKind::RegistryPackage,
            "branch_protection_rule" => EventKind::BranchProtectionRule,
            "watch" => EventKind::Watch,
            other => EventKind::Other {
                name: other.to_string(),
            },
        }
    }

    /// Activity types that fire when the user omits `types:`.
    /// `None` = all types match. `Some(subset)` = only listed activities match.
    /// Currently `pull_request` / `pull_request_target` are the only events
    /// that GitHub Actions defaults to a subset.
    pub fn default_activity_subset(&self) -> Option<&'static [&'static str]> {
        match self {
            EventKind::PullRequest | EventKind::PullRequestTarget => {
                Some(&["opened", "synchronize", "reopened"])
            }
            _ => None,
        }
    }

    /// Closed set of activity types accepted by the `types:` filter for this
    /// event, per the GitHub Actions spec ("Events that trigger workflows" —
    /// https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows).
    ///
    /// Returns `None` when:
    /// - the event has no `types:` support at all (e.g. `push`, `schedule`), or
    /// - the event accepts open / user-defined types (`repository_dispatch`), or
    /// - the event is not modelled explicitly (`Other { name }`).
    ///
    /// Returns `Some(set)` when a known closed set of activity types is defined
    /// by the spec. The parser emits a `ParseDiagnostic` for any value in a
    /// user-supplied `types:` list that is not a member of `set`.
    pub fn allowed_activity_types(&self) -> Option<&'static [&'static str]> {
        match self {
            EventKind::BranchProtectionRule => Some(&["created", "edited", "deleted"]),
            EventKind::CheckRun => {
                Some(&["created", "rerequested", "completed", "requested_action"])
            }
            EventKind::CheckSuite => Some(&["completed"]),
            EventKind::Discussion => Some(&[
                "created",
                "edited",
                "deleted",
                "transferred",
                "pinned",
                "unpinned",
                "labeled",
                "unlabeled",
                "locked",
                "unlocked",
                "category_changed",
                "answered",
                "unanswered",
            ]),
            EventKind::DiscussionComment => Some(&["created", "edited", "deleted"]),
            EventKind::IssueComment => Some(&["created", "edited", "deleted"]),
            EventKind::Issues => Some(&[
                "opened",
                "edited",
                "deleted",
                "transferred",
                "pinned",
                "unpinned",
                "closed",
                "reopened",
                "assigned",
                "unassigned",
                "labeled",
                "unlabeled",
                "locked",
                "unlocked",
                "milestoned",
                "demilestoned",
                "typed",
                "untyped",
            ]),
            EventKind::Label => Some(&["created", "edited", "deleted"]),
            EventKind::MergeGroup => Some(&["checks_requested"]),
            EventKind::Milestone => Some(&["created", "closed", "opened", "edited", "deleted"]),
            EventKind::PullRequest | EventKind::PullRequestTarget => Some(&[
                "assigned",
                "unassigned",
                "labeled",
                "unlabeled",
                "opened",
                "edited",
                "closed",
                "reopened",
                "synchronize",
                "converted_to_draft",
                "locked",
                "unlocked",
                "enqueued",
                "dequeued",
                "milestoned",
                "demilestoned",
                "ready_for_review",
                "review_requested",
                "review_request_removed",
                "auto_merge_enabled",
                "auto_merge_disabled",
            ]),
            EventKind::PullRequestReview => Some(&["submitted", "edited", "dismissed"]),
            EventKind::PullRequestReviewComment => Some(&["created", "edited", "deleted"]),
            EventKind::RegistryPackage => Some(&["published", "updated"]),
            EventKind::Release => Some(&[
                "published",
                "unpublished",
                "created",
                "edited",
                "deleted",
                "prereleased",
                "released",
            ]),
            EventKind::Watch => Some(&["started"]),
            EventKind::WorkflowRun => Some(&["completed", "requested", "in_progress"]),
            // Events without `types:` support, or with open/user-defined types
            // (e.g. `repository_dispatch`), and the catch-all for forward-compat
            // with new GitHub events modelled as `Other { name }`:
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RefFilter {
    #[default]
    None,
    Include {
        patterns: Vec<String>,
    },
    Exclude {
        patterns: Vec<String>,
    },
}

impl RefFilter {
    pub fn is_none(&self) -> bool {
        matches!(self, RefFilter::None)
    }

    /// Returns `true` when this filter would let an event through for the
    /// given short ref name or path. Semantics:
    ///
    /// - `RefFilter::None` always returns `true` (no filter declared = trigger
    ///   fires for every ref / path).
    /// - `RefFilter::Include { patterns }` returns `true` when sequential
    ///   evaluation of the patterns ends in the matched state. `!`-prefixed
    ///   entries subtract from the matched set per the GitHub Actions filter
    ///   pattern cheat sheet.
    /// - `RefFilter::Exclude { patterns }` (parsed from `branches-ignore` /
    ///   `tags-ignore` / `paths-ignore`) is the inverse: it returns `false`
    ///   when sequential evaluation says the value is in the ignore set.
    ///   For `paths-ignore`, callers interpret a single `value` as a
    ///   "changeset of exactly that one file"; the trigger is suppressed when
    ///   the only changed file is ignored, which is what `!sequential_match`
    ///   expresses.
    ///
    /// Glob syntax follows the [`globset`] crate. `*`, `**`, `[abc]`, and the
    /// leading `!` negation match the GitHub cheat sheet exactly. The `?`
    /// quantifier (GitHub: zero-or-one; globset: exactly-one) and `+`
    /// quantifier (GitHub-only) are not faithfully covered; unsupported
    /// patterns surface a warning on stderr and are treated as no-match.
    pub fn matches(&self, value: &str) -> bool {
        match self {
            RefFilter::None => true,
            RefFilter::Include { patterns } => sequential_match(patterns, value),
            RefFilter::Exclude { patterns } => !sequential_match(patterns, value),
        }
    }
}

/// Sequential `!`-aware evaluator for a filter pattern list. Initial state
/// is "not matched"; each positive pattern that matches sets the state to
/// matched, each `!`-prefixed pattern that matches clears it. A leading `!`
/// has no positive set to subtract from and is therefore a no-op — pinned by
/// the `ref_filter_matches_leading_negation_is_noop` test.
fn sequential_match(patterns: &[String], value: &str) -> bool {
    let mut hit = false;
    for pat in patterns {
        if let Some(neg) = pat.strip_prefix('!') {
            if glob_match(neg, value) {
                hit = false;
            }
        } else if glob_match(pat, value) {
            hit = true;
        }
    }
    hit
}

fn glob_match(pattern: &str, value: &str) -> bool {
    match globset::Glob::new(pattern) {
        Ok(g) => g.compile_matcher().is_match(value),
        Err(e) => {
            eprintln!("warn: unsupported filter pattern `{pattern}`: {e}");
            false
        }
    }
}

/// Event-specific payloads not expressible by the common filter fields.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventExtras {
    Schedule {
        entries: Vec<ScheduleEntry>,
    },
    WorkflowDispatch {
        inputs: Vec<InputDecl>,
    },
    WorkflowCall {
        inputs: Vec<InputDecl>,
        outputs: Vec<OutputDecl>,
        secrets: Vec<SecretDecl>,
    },
    WorkflowRun {
        workflows: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduleEntry {
    pub cron: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}

impl TriggerSpec {
    /// Bare trigger with empty filters and the event-appropriate empty
    /// extras payload (so e.g. `Workflow::secrets_required()` for a bare
    /// `on: workflow_call` returns `Some(&[])`, matching parser output).
    pub fn bare(event: EventKind) -> Self {
        let extras = match event {
            EventKind::Schedule => Some(EventExtras::Schedule { entries: vec![] }),
            EventKind::WorkflowDispatch => Some(EventExtras::WorkflowDispatch { inputs: vec![] }),
            EventKind::WorkflowCall => Some(EventExtras::WorkflowCall {
                inputs: vec![],
                outputs: vec![],
                secrets: vec![],
            }),
            EventKind::WorkflowRun => Some(EventExtras::WorkflowRun { workflows: vec![] }),
            _ => None,
        };
        TriggerSpec {
            event,
            branches: RefFilter::None,
            tags: RefFilter::None,
            paths: RefFilter::None,
            types: None,
            extras,
        }
    }

    /// Event identifier as the YAML `on:` key string. Preserved for
    /// existing call sites that previously matched on `event_name()`.
    pub fn event_name(&self) -> &str {
        self.event.name()
    }

    /// Workflows callable from another workflow (`on: workflow_call`) are not
    /// entry points; they must be invoked.
    pub fn is_entry_point(&self) -> bool {
        !matches!(self.event, EventKind::WorkflowCall)
    }

    /// `true` when this trigger fires for the given activity type.
    /// - When `self.types` is `Some(explicit)`, only the listed activities match
    ///   (an explicit empty list matches nothing).
    /// - When `self.types` is `None`, fall back to the event's
    ///   `default_activity_subset()`.
    pub fn matches_activity(&self, activity: &str) -> bool {
        match &self.types {
            Some(explicit) => explicit.iter().any(|x| x == activity),
            None => match self.event.default_activity_subset() {
                None => true,
                Some(subset) => subset.contains(&activity),
            },
        }
    }
}

/// Runner target for a job, parsed from `jobs.<job_id>.runs-on`.
///
/// GitHub Actions supports three forms:
/// - Scalar: `runs-on: ubuntu-latest` → `RunsOn { labels: ["ubuntu-latest"], group: None }`
/// - Sequence: `runs-on: [self-hosted, linux, x64]` → `RunsOn { labels: [...], group: None }`
/// - Mapping: `runs-on: { group: my-runners, labels: [linux] }` → explicit group + optional labels
///
/// Spec: Workflow syntax — https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax
/// (section: `jobs.<job_id>.runs-on`)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunsOn {
    /// Runner labels. For the scalar and sequence forms this holds all values.
    /// For the mapping form this holds the `labels:` value (empty when omitted).
    pub labels: Vec<String>,
    /// Runner group name. Only present in the mapping form when `group:` is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}

/// Structured form of a job-level `environment:` value.
///
/// GitHub Actions accepts both a plain string (`environment: production`) and a
/// mapping (`environment: { name: production, url: https://... }`). The scalar
/// form is normalised to `JobEnvironment { name, url: None }` during parsing.
///
/// Workflow syntax — https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_idenvironment
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobEnvironment {
    /// The environment name, which may contain an expression (`${{ ... }}`).
    pub name: String,
    /// The optional deployment URL, which may contain an expression.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: JobId,
    pub workflow: WorkflowId,
    pub needs: Vec<String>,
    pub permissions: Option<Permissions>,
    pub steps: Vec<Step>,
    /// Set when this job uses `uses:` at the job level (i.e. calls a reusable workflow).
    pub calls_workflow: Option<CallsWorkflow>,
    /// Runner specification. `None` for reusable-workflow caller jobs (`calls_workflow.is_some()`)
    /// because `runs-on` MUST be omitted on those jobs per the GitHub Actions spec.
    /// A `ParseDiagnostic` is emitted when a non-`uses:` job omits `runs-on`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runs_on: Option<RunsOn>,
    /// Job-level `outputs:` map (raw expression strings keyed by output name).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub outputs: BTreeMap<String, String>,
    /// Source position of the job key in the workflow file. The `file` is the
    /// parent workflow file; `line` is the 1-based line of the job key.
    pub source: SourcePos,
    /// Job-level `defaults:` (shell / working-directory for `run:` steps in this job).
    /// Overrides workflow-level `defaults:` per the GitHub Actions spec.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defaults: Option<Defaults>,
    /// Job-level `env:` map available to all steps within this job.
    /// Overrides workflow-level `env:` for the same key.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Job-level `concurrency:` block, if present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<Concurrency>,
    /// Job-level `if:` expression. Kept as a raw string without evaluation.
    /// `None` when the job has no `if:` key.
    ///
    /// Spec: https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax
    /// (section: `jobs.<job_id>.if`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub if_expr: Option<String>,
    /// `strategy:` block. Captured as a raw representation for lazy expansion.
    /// Per the GitHub Actions workflow syntax spec
    /// (Workflow syntax — https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax,
    /// section `jobs.<job_id>.strategy`), `matrix` keys may hold arrays of
    /// scalars or objects; `fail-fast` defaults to `true`; `max-parallel`
    /// defaults to the maximum number of available runners.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategy: Option<Strategy>,
    /// Job-level `container:` definition. Scalar form (`container: alpine:3.20`)
    /// and mapping form are both captured. `None` when the key is absent.
    /// Ref: Workflow syntax — https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_idcontainer
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<JobContainer>,
    /// Job-level `services:` map, keyed by service name.
    /// Ref: Workflow syntax — https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_idservices
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub services: BTreeMap<String, JobContainer>,
    /// Job-level `environment:`. Captured for `check secrets` to detect
    /// silent shadowing of caller-passed secrets in reusable workflow callees
    /// (the env secret is used in place of the caller-passed secret per the
    /// GitHub Actions reusable-workflow spec). Both scalar form (`environment:
    /// production`) and mapping form (`environment: { name: production, url:
    /// https://... }`) are supported; scalar form collapses to
    /// `JobEnvironment { name, url: None }`. Expression-only values and
    /// mapping forms missing a `name:` key collapse to `None`.
    ///
    /// Workflow syntax — https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_idenvironment
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment: Option<JobEnvironment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<Annotation>,
}

/// Raw representation of a `strategy:` block. Expansion is deferred to the
/// query layer; this type preserves the YAML structure without eager cross-
/// products or include/exclude normalization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Strategy {
    /// Raw matrix variable definitions (key → array of values). `include` and
    /// `exclude` entries are stored under their literal keys `"include"` /
    /// `"exclude"` as sequences of `MatrixValue::Object`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matrix: Option<Matrix>,
    /// `fail-fast:` — when `true` (the GA default), GitHub cancels all
    /// in-progress and queued jobs in the matrix when any job fails.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fail_fast: Option<bool>,
    /// `max-parallel:` — maximum number of matrix jobs to run concurrently.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_parallel: Option<u32>,
}

/// Raw matrix definition. Each key maps to an ordered list of [`MatrixValue`]s.
/// `include` and `exclude` are special keys defined by the GA spec but are
/// stored here verbatim alongside user-defined dimension keys.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Matrix {
    pub dimensions: BTreeMap<String, Vec<MatrixValue>>,
}

/// A single cell value within a matrix dimension array. GitHub Actions allows
/// scalars (string, integer, boolean) and mapping objects as matrix values.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum MatrixValue {
    /// String scalar (also used for GA expression strings like `${{ ... }}`).
    String(String),
    /// Integer scalar.
    Int(i64),
    /// Boolean scalar.
    Bool(bool),
    /// Mapping object (e.g. `{ os: ubuntu-latest, node: 18 }`).
    Object(BTreeMap<String, MatrixValue>),
}

/// Container or service container definition for a job.
///
/// Scalar form (`container: alpine:3.20`) sets only `image`; all other fields
/// default. Mapping form allows `credentials`, `env`, `ports`, `volumes`, and
/// `options` to be specified.
///
/// Ref: Workflow syntax — https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#jobsjob_idcontainer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobContainer {
    pub image: String,
    /// Registry credentials (`username` + `password`). `None` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credentials: Option<JobContainerCredentials>,
    /// Environment variables to set inside the container.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Ports to expose on the container host (raw strings, e.g. `"8080:80"`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<String>,
    /// Volume mounts (raw strings, e.g. `"my_docker_volume:/volume_mount"`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub volumes: Vec<String>,
    /// Additional Docker `--options` flags passed to the container.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<String>,
}

/// Registry credentials used by [`JobContainer`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobContainerCredentials {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallsWorkflow {
    pub workflow_ref: WorkflowRef,
    pub with: BTreeMap<String, String>,
    pub secrets: SecretsPass,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkflowRef {
    Local(WorkflowId),
    External {
        owner: String,
        repo: String,
        path: String,
        gitref: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SecretsPass {
    None,
    Inherit,
    Explicit(BTreeMap<String, String>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    pub index: usize,
    pub id: Option<StepId>,
    pub name: Option<String>,
    pub uses: Option<UsesRef>,
    /// `run:` block body. `None` when the step has no `run:`.
    /// Multiline scalars (`|`, `>`, `>-`) are joined into a single string by the YAML parser.
    pub run: Option<String>,
    pub if_expr: Option<String>,
    /// Step-level `with:` (raw scalar strings keyed by input name).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub with: BTreeMap<String, String>,
    /// Step-level `env:` (raw scalar strings keyed by env-var name).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// `shell:` override for `run:` steps (e.g. `bash`, `pwsh`, `python`, `sh`).
    /// Inherits from `defaults.run.shell` when absent (Issue #8).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    /// `working-directory:` override for this step.
    /// Inherits from `defaults.run.working-directory` when absent (Issue #8).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<String>,
    /// `timeout-minutes:` for this step (positive integer per spec).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_minutes: Option<u32>,
    /// `continue-on-error:` stored as a raw string because the spec accepts both a
    /// bool literal (`true`/`false`) and an expression (`${{ inputs.allow_fail }}`).
    /// Source: Workflow syntax — https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continue_on_error: Option<String>,
    /// Source position of the step's mapping start in the file. The `file` is
    /// the parent workflow or composite action file.
    pub source: SourcePos,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<Annotation>,
}

/// `uses:` target classification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UsesRef {
    /// `./.github/workflows/X.yml` — local reusable workflow.
    LocalWorkflow(WorkflowId),
    /// `./path/to/dir` — local action directory (composite/JS/Docker, resolved by IR builder).
    LocalAction(ActionId),
    /// `owner/repo[/subpath]@gitref` — external action (opaque in MVP).
    External {
        owner: String,
        repo: String,
        subpath: Option<String>,
        gitref: String,
    },
    /// `docker://[host/]image[:tag]` — Docker Hub or registry container action.
    Docker(DockerRef),
}

/// A local action manifest (`.github/actions/<id>/action.yml` etc.).
/// Discriminated by [`ActionKind`] into composite / JavaScript / Docker variants.
/// `steps` is populated only for composite actions; empty for JS / Docker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalAction {
    pub id: ActionId,
    pub source: SourcePos,
    pub name: Option<String>,
    pub kind: ActionKind,
    pub inputs: Vec<InputDecl>,
    pub outputs: Vec<OutputDecl>,
    /// Steps under `runs.steps` for composite actions; empty for JS / Docker actions.
    pub steps: Vec<Step>,
    /// Ravelact annotations found in the action source (e.g. `# ravelact:dispatches`).
    /// Mirrors the `annotations` field on `Workflow`, `Job`, and `Step`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionKind {
    Composite,
    JavaScript { node_version: String },
    Docker,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InputDecl {
    pub name: String,
    pub required: bool,
    pub default: Option<String>,
    /// Declared input type. `None` when omitted or unrecognized;
    /// type-mismatch checks skip such inputs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_type: Option<InputType>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InputType {
    String,
    Boolean,
    Number,
    Choice {
        options: Vec<String>,
    },
    /// `type: environment` — value must be a non-empty string naming a GitHub deployment
    /// environment. Full environment-name validation against the repository's configured
    /// environments requires GitHub API access and is out of scope; only the non-empty
    /// constraint is checked here.
    /// Spec: Events that trigger workflows —
    /// https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows
    Environment,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OutputDecl {
    pub name: String,
    /// Raw `value:` expression for callable workflow / composite outputs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecretDecl {
    pub name: String,
    pub required: bool,
}

/// Coarse-level `permissions:` value (string scalar form).
///
/// GA spec: https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#permissions
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoarseKind {
    /// `permissions: read-all` — grants read access across all scopes.
    ReadAll,
    /// `permissions: write-all` — grants write access across all scopes.
    WriteAll,
    /// Any other string value; preserved for diagnostic surfacing rather than
    /// silently normalizing to an empty scope map.
    Unknown(String),
}

/// Per-scope permission key. Closed enumeration per the GA spec with an
/// `Unknown` variant for forward compatibility when the spec adds new scopes.
///
/// GA spec: https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax#permissions
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ScopeKey {
    Actions,
    ArtifactMetadata,
    Attestations,
    Checks,
    Contents,
    Deployments,
    Discussions,
    IdToken,
    Issues,
    Models,
    Packages,
    Pages,
    PullRequests,
    /// `repository-projects` was a recognized scope in earlier spec versions.
    /// Retained for backward compatibility with existing workflows.
    RepositoryProjects,
    SecurityEvents,
    Statuses,
    VulnerabilityAlerts,
    /// Any key not recognized by the current spec; preserved for diagnostics
    /// and forward compatibility.
    Unknown(String),
}

/// Per-scope access level. Values: `read`, `write`, `none`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeAccess {
    Read,
    Write,
    None,
    /// Any access string other than `read`, `write`, `none`; preserved for
    /// diagnostics rather than silently normalizing to `None`.
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Permissions {
    /// String-scalar form: `permissions: read-all` / `permissions: write-all`.
    /// An empty mapping `permissions: {}` parses as `Scopes({})`, not `Coarse`.
    Coarse(CoarseKind),
    /// Per-scope map (`contents: read`, ...). An empty mapping `permissions: {}`
    /// is represented as `Scopes(BTreeMap::new())` — a deliberate declaration
    /// of no permissions.
    Scopes(BTreeMap<ScopeKey, ScopeAccess>),
}

/// Structured form of a `docker://[host/]image[:tag]` action reference.
///
/// - `host` is `None` for Docker Hub images (e.g. `docker://alpine:3.8`).
/// - `tag` is `None` when the URI omits the tag (e.g. `docker://ghcr.io/owner/image`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DockerRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    pub image: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
}

impl DockerRef {
    /// Format as `[host/]image[:tag]` for display purposes.
    pub fn display_str(&self) -> String {
        let host_prefix = self
            .host
            .as_deref()
            .map(|h| format!("{h}/"))
            .unwrap_or_default();
        let tag_suffix = self
            .tag
            .as_deref()
            .map(|t| format!(":{t}"))
            .unwrap_or_default();
        format!("{}{}{}", host_prefix, self.image, tag_suffix)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExternalActionRef {
    pub owner: String,
    pub repo: String,
    pub subpath: Option<String>,
    pub gitref: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn annotation_roundtrip_resolved() {
        let ann = Annotation {
            verb: AnnotationVerb::Dispatches,
            resolution: AnnotationResolution::Resolved {
                target: WorkflowId(".github/workflows/build.yml".into()),
            },
            source_line: 12,
        };
        let json = serde_json::to_string(&ann).unwrap();
        assert!(json.contains("\"verb\":\"dispatches\""), "got: {json}");
        assert!(json.contains("\"kind\":\"resolved\""), "got: {json}");
        let back: Annotation = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ann);
    }

    #[test]
    fn annotation_roundtrip_dangling() {
        let ann = Annotation {
            verb: AnnotationVerb::Triggers,
            resolution: AnnotationResolution::Dangling {
                raw_target: "..\\bad".into(),
                reason: "absolute path or `..` not allowed".into(),
            },
            source_line: 4,
        };
        let json = serde_json::to_string(&ann).unwrap();
        assert!(json.contains("\"verb\":\"triggers\""));
        assert!(json.contains("\"kind\":\"dangling\""));
        let back: Annotation = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ann);
    }

    #[test]
    fn empty_annotations_skip_serialize() {
        // A Step with no annotations should serialize without that field,
        // matching the existing OSS dump snapshots.
        let step = Step {
            index: 0,
            id: None,
            name: None,
            uses: None,
            run: Some("echo hi".into()),
            if_expr: None,
            with: Default::default(),
            env: Default::default(),
            shell: None,
            working_directory: None,
            timeout_minutes: None,
            continue_on_error: None,
            source: SourcePos {
                file: PathBuf::new(),
                line: None,
            },
            annotations: Vec::new(),
        };
        let json = serde_json::to_string(&step).unwrap();
        assert!(
            !json.contains("annotations"),
            "empty annotations Vec should be skipped: {json}"
        );
    }

    #[test]
    fn local_action_empty_annotations_skip_serialize() {
        // A LocalAction with no annotations should serialize without that field.
        let action = LocalAction {
            id: ActionId(".github/actions/setup".into()),
            source: SourcePos {
                file: PathBuf::from(".github/actions/setup/action.yaml"),
                line: Some(1),
            },
            name: Some("Setup".into()),
            kind: ActionKind::Composite,
            inputs: vec![],
            outputs: vec![],
            steps: vec![],
            annotations: Vec::new(),
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(
            !json.contains("annotations"),
            "empty annotations Vec should be skipped on LocalAction: {json}"
        );
    }

    // ----- Test #1: EventKind::default_activity_subset --------------------
    #[test]
    fn default_activity_subset_pull_request_returns_subset() {
        let subset = EventKind::PullRequest.default_activity_subset();
        assert_eq!(subset, Some(&["opened", "synchronize", "reopened"][..]));
        let subset_target = EventKind::PullRequestTarget.default_activity_subset();
        assert_eq!(
            subset_target,
            Some(&["opened", "synchronize", "reopened"][..])
        );
    }

    #[test]
    fn default_activity_subset_other_events_return_none() {
        assert_eq!(EventKind::Issues.default_activity_subset(), None);
        assert_eq!(EventKind::WorkflowRun.default_activity_subset(), None);
        assert_eq!(
            EventKind::RepositoryDispatch.default_activity_subset(),
            None
        );
        assert_eq!(
            EventKind::Other {
                name: "custom".into()
            }
            .default_activity_subset(),
            None
        );
    }

    // ----- Test #2: TriggerSpec::matches_activity -------------------------
    #[test]
    fn matches_activity_pr_default_subset_when_types_omitted() {
        let t = TriggerSpec::bare(EventKind::PullRequest);
        assert!(t.matches_activity("opened"));
        assert!(t.matches_activity("synchronize"));
        assert!(t.matches_activity("reopened"));
        assert!(!t.matches_activity("assigned"));
        assert!(!t.matches_activity("labeled"));
    }

    #[test]
    fn matches_activity_pr_explicit_overrides_default_subset() {
        // user wrote `types: [opened]` — synchronize is now excluded
        let mut t = TriggerSpec::bare(EventKind::PullRequest);
        t.types = Some(vec!["opened".into()]);
        assert!(t.matches_activity("opened"));
        assert!(!t.matches_activity("synchronize"));
        assert!(!t.matches_activity("assigned"));
    }

    #[test]
    fn matches_activity_pr_explicit_assigned_only() {
        let mut t = TriggerSpec::bare(EventKind::PullRequest);
        t.types = Some(vec!["assigned".into()]);
        assert!(t.matches_activity("assigned"));
        assert!(!t.matches_activity("opened"));
    }

    #[test]
    fn matches_activity_explicit_empty_matches_nothing() {
        // user wrote `types: []` — explicit empty, zero match
        let mut t = TriggerSpec::bare(EventKind::PullRequest);
        t.types = Some(vec![]);
        assert!(!t.matches_activity("opened"));
        assert!(!t.matches_activity("synchronize"));
    }

    #[test]
    fn matches_activity_issues_all_by_default() {
        // Issues has no default subset → omitting types = all match
        let t = TriggerSpec::bare(EventKind::Issues);
        assert!(t.matches_activity("opened"));
        assert!(t.matches_activity("closed"));
        assert!(t.matches_activity("labeled"));
    }

    // ----- Test #7: Workflow accessor methods -----------------------------
    #[test]
    fn workflow_accessors_some_for_workflow_call() {
        let wf = Workflow {
            id: WorkflowId("x.yml".into()),
            source: SourcePos {
                file: PathBuf::new(),
                line: None,
            },
            name: None,
            run_name: None,
            triggers: vec![TriggerSpec {
                event: EventKind::WorkflowCall,
                branches: RefFilter::None,
                tags: RefFilter::None,
                paths: RefFilter::None,
                types: None,
                extras: Some(EventExtras::WorkflowCall {
                    inputs: vec![InputDecl {
                        name: "artifact".into(),
                        required: true,
                        default: None,
                        input_type: None,
                    }],
                    outputs: vec![],
                    secrets: vec![],
                }),
            }],
            jobs: vec![],
            permissions: None,
            defaults: None,
            env: BTreeMap::new(),
            concurrency: None,
            annotations: Vec::new(),
        };
        let inputs = wf.inputs().expect("workflow_call trigger has Some inputs");
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].name, "artifact");
        assert_eq!(wf.outputs().map(|o| o.len()), Some(0));
        assert_eq!(wf.secrets_required().map(|s| s.len()), Some(0));
    }

    #[test]
    fn workflow_accessors_none_for_entry_only() {
        let wf = Workflow {
            id: WorkflowId("y.yml".into()),
            source: SourcePos {
                file: PathBuf::new(),
                line: None,
            },
            name: None,
            run_name: None,
            triggers: vec![TriggerSpec::bare(EventKind::Push)],
            jobs: vec![],
            permissions: None,
            defaults: None,
            env: BTreeMap::new(),
            concurrency: None,
            annotations: Vec::new(),
        };
        assert_eq!(wf.inputs(), None);
        assert_eq!(wf.outputs(), None);
        assert_eq!(wf.secrets_required(), None);
    }

    #[test]
    fn workflow_accessors_some_empty_for_bare_workflow_call() {
        // bare-string `on: workflow_call` — extras gets empty payloads, accessors
        // return Some(&[]) (parser- and constructor-consistent).
        let wf = Workflow {
            id: WorkflowId("z.yml".into()),
            source: SourcePos {
                file: PathBuf::new(),
                line: None,
            },
            name: None,
            run_name: None,
            triggers: vec![TriggerSpec::bare(EventKind::WorkflowCall)],
            jobs: vec![],
            permissions: None,
            defaults: None,
            env: BTreeMap::new(),
            concurrency: None,
            annotations: Vec::new(),
        };
        assert_eq!(wf.inputs(), Some(&[][..]));
        assert_eq!(wf.outputs(), Some(&[][..]));
        assert_eq!(wf.secrets_required(), Some(&[][..]));
    }

    // ----- Concurrency serde roundtrip -----------------------------------
    #[test]
    fn concurrency_roundtrip_with_cancel() {
        let c = Concurrency {
            group: "${{ github.workflow }}-${{ github.ref }}".into(),
            cancel_in_progress: Some(true),
        };
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains("cancel_in_progress"), "got: {json}");
        let back: Concurrency = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn concurrency_roundtrip_no_cancel_omits_field() {
        // cancel_in_progress: None must be absent from the JSON output
        // (skip_serializing_if = "Option::is_none")
        let c = Concurrency {
            group: "my-group".into(),
            cancel_in_progress: None,
        };
        let json = serde_json::to_string(&c).unwrap();
        assert!(
            !json.contains("cancel_in_progress"),
            "cancel_in_progress should be absent when None: {json}"
        );
        // Deserializing from a JSON without cancel_in_progress must give None back
        let back: Concurrency = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c);
    }

    // ----- Test #9: RefFilter::Include(vec![]) literal preservation -------
    #[test]
    fn ref_filter_include_empty_preserved_through_roundtrip() {
        let f = RefFilter::Include { patterns: vec![] };
        let json = serde_json::to_string(&f).unwrap();
        let back: RefFilter = serde_json::from_str(&json).unwrap();
        assert_eq!(back, f);
        // Confirm explicit empty is distinguishable from RefFilter::None:
        assert_ne!(RefFilter::Include { patterns: vec![] }, RefFilter::None);
    }

    // ----- Test #10: EventKind::Other serde roundtrip ---------------------
    #[test]
    fn event_kind_other_roundtrip_preserves_case_in_payload() {
        let e = EventKind::Other {
            name: "Custom_EVENT".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        let back: EventKind = serde_json::from_str(&json).unwrap();
        assert_eq!(back, e);
        // The discriminator tag is snake_case ("other"), but the `name` payload
        // is opaque user data and must round-trip verbatim.
        assert!(json.contains("\"kind\":\"other\""), "got: {json}");
        assert!(json.contains("\"name\":\"Custom_EVENT\""), "got: {json}");
    }

    // ----- Test #11: EventKind::allowed_activity_types --------------------

    #[test]
    fn allowed_activity_types_pull_request_contains_expected_types() {
        let allowed = EventKind::PullRequest.allowed_activity_types().unwrap();
        // spot-check a representative subset from the spec
        assert!(allowed.contains(&"opened"));
        assert!(allowed.contains(&"synchronize"));
        assert!(allowed.contains(&"closed"));
        assert!(allowed.contains(&"ready_for_review"));
        assert!(allowed.contains(&"auto_merge_enabled"));
        // must not contain a made-up type
        assert!(!allowed.contains(&"openned"));
    }

    #[test]
    fn allowed_activity_types_pull_request_target_matches_pull_request() {
        assert_eq!(
            EventKind::PullRequest.allowed_activity_types(),
            EventKind::PullRequestTarget.allowed_activity_types(),
        );
    }

    #[test]
    fn allowed_activity_types_merge_group_single_type() {
        let allowed = EventKind::MergeGroup.allowed_activity_types().unwrap();
        assert_eq!(allowed, &["checks_requested"]);
    }

    #[test]
    fn allowed_activity_types_watch_single_type() {
        let allowed = EventKind::Watch.allowed_activity_types().unwrap();
        assert_eq!(allowed, &["started"]);
    }

    #[test]
    fn allowed_activity_types_returns_none_for_push_and_schedule() {
        // push and schedule have no `types:` support
        assert!(EventKind::Push.allowed_activity_types().is_none());
        assert!(EventKind::Schedule.allowed_activity_types().is_none());
    }

    #[test]
    fn allowed_activity_types_returns_none_for_repository_dispatch() {
        // repository_dispatch accepts user-defined types — open set
        assert!(EventKind::RepositoryDispatch
            .allowed_activity_types()
            .is_none());
    }

    #[test]
    fn allowed_activity_types_returns_none_for_workflow_call_and_dispatch() {
        assert!(EventKind::WorkflowCall.allowed_activity_types().is_none());
        assert!(EventKind::WorkflowDispatch
            .allowed_activity_types()
            .is_none());
    }

    #[test]
    fn allowed_activity_types_returns_none_for_other() {
        assert!(EventKind::Other {
            name: "custom".into()
        }
        .allowed_activity_types()
        .is_none());
    }

    #[test]
    fn allowed_activity_types_issues_contains_all_spec_types() {
        let allowed = EventKind::Issues.allowed_activity_types().unwrap();
        for t in &[
            "opened",
            "edited",
            "deleted",
            "transferred",
            "pinned",
            "unpinned",
            "closed",
            "reopened",
            "assigned",
            "unassigned",
            "labeled",
            "unlabeled",
            "locked",
            "unlocked",
            "milestoned",
            "demilestoned",
        ] {
            assert!(allowed.contains(t), "missing type: {t}");
        }
    }

    #[test]
    fn allowed_activity_types_release_full_set() {
        let allowed = EventKind::Release.allowed_activity_types().unwrap();
        for t in &[
            "published",
            "unpublished",
            "created",
            "edited",
            "deleted",
            "prereleased",
            "released",
        ] {
            assert!(allowed.contains(t), "missing type: {t}");
        }
    }

    // ----- Test #12: Permissions typed enum serde roundtrips ---------------

    #[test]
    fn coarse_kind_read_all_roundtrip() {
        let kind = CoarseKind::ReadAll;
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, "\"read_all\"", "got: {json}");
        let back: CoarseKind = serde_json::from_str(&json).unwrap();
        assert_eq!(back, kind);
    }

    #[test]
    fn coarse_kind_write_all_roundtrip() {
        let kind = CoarseKind::WriteAll;
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, "\"write_all\"", "got: {json}");
        let back: CoarseKind = serde_json::from_str(&json).unwrap();
        assert_eq!(back, kind);
    }

    #[test]
    fn coarse_kind_unknown_roundtrip() {
        let kind = CoarseKind::Unknown("read-al".into());
        let json = serde_json::to_string(&kind).unwrap();
        // Unknown tuple variant serializes as {"unknown":"read-al"}
        assert!(
            json.contains("\"unknown\"") && json.contains("\"read-al\""),
            "got: {json}"
        );
        let back: CoarseKind = serde_json::from_str(&json).unwrap();
        assert_eq!(back, kind);
    }

    #[test]
    fn scope_key_pull_requests_roundtrip() {
        let key = ScopeKey::PullRequests;
        let json = serde_json::to_string(&key).unwrap();
        assert_eq!(json, "\"pull-requests\"", "got: {json}");
        let back: ScopeKey = serde_json::from_str(&json).unwrap();
        assert_eq!(back, key);
    }

    #[test]
    fn scope_key_id_token_roundtrip() {
        let key = ScopeKey::IdToken;
        let json = serde_json::to_string(&key).unwrap();
        assert_eq!(json, "\"id-token\"", "got: {json}");
        let back: ScopeKey = serde_json::from_str(&json).unwrap();
        assert_eq!(back, key);
    }

    #[test]
    fn scope_key_unknown_roundtrip() {
        let key = ScopeKey::Unknown("future-scope".into());
        let json = serde_json::to_string(&key).unwrap();
        // With rename_all = "kebab-case", the Unknown tuple variant serializes
        // as {"unknown":"future-scope"} — kebab-case of "Unknown" is "unknown".
        assert!(
            json.contains("\"unknown\"") && json.contains("\"future-scope\""),
            "got: {json}"
        );
        let back: ScopeKey = serde_json::from_str(&json).unwrap();
        assert_eq!(back, key);
    }

    #[test]
    fn scope_access_roundtrip() {
        for (access, expected) in [
            (ScopeAccess::Read, "\"read\""),
            (ScopeAccess::Write, "\"write\""),
            (ScopeAccess::None, "\"none\""),
        ] {
            let json = serde_json::to_string(&access).unwrap();
            assert_eq!(json, expected, "got: {json}");
            let back: ScopeAccess = serde_json::from_str(&json).unwrap();
            assert_eq!(back, access);
        }
    }

    #[test]
    fn scope_access_unknown_roundtrip() {
        let access = ScopeAccess::Unknown("admin".into());
        let json = serde_json::to_string(&access).unwrap();
        assert!(
            json.contains("\"unknown\"") && json.contains("\"admin\""),
            "got: {json}"
        );
        let back: ScopeAccess = serde_json::from_str(&json).unwrap();
        assert_eq!(back, access);
    }

    #[test]
    fn permissions_coarse_roundtrip() {
        let p = Permissions::Coarse(CoarseKind::WriteAll);
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains("write_all"), "got: {json}");
        let back: Permissions = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn permissions_scopes_roundtrip() {
        let mut map = BTreeMap::new();
        map.insert(ScopeKey::Contents, ScopeAccess::Read);
        map.insert(ScopeKey::PullRequests, ScopeAccess::Write);
        let p = Permissions::Scopes(map);
        let json = serde_json::to_string(&p).unwrap();
        assert!(
            json.contains("\"contents\"") && json.contains("\"pull-requests\""),
            "got: {json}"
        );
        let back: Permissions = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
    }

    // ----- Defaults / RunDefaults serde behaviour -------------------------
    #[test]
    fn defaults_fully_populated_roundtrips() {
        let d = Defaults {
            run: Some(RunDefaults {
                shell: Some("bash".into()),
                working_directory: Some("src".into()),
            }),
        };
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("\"shell\":\"bash\""), "got: {json}");
        assert!(
            json.contains("\"working_directory\":\"src\""),
            "got: {json}"
        );
        let back: Defaults = serde_json::from_str(&json).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn defaults_empty_skips_serialize() {
        // An empty Defaults (no run key) should produce `{}` — both
        // `run` is `skip_serializing_if = "Option::is_none"`.
        let d = Defaults { run: None };
        let json = serde_json::to_string(&d).unwrap();
        assert!(
            !json.contains("run"),
            "empty defaults should omit `run`: {json}"
        );
    }

    // ----- RefFilter::matches ---------------------------------------------

    fn include(patterns: &[&str]) -> RefFilter {
        RefFilter::Include {
            patterns: patterns.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn exclude(patterns: &[&str]) -> RefFilter {
        RefFilter::Exclude {
            patterns: patterns.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn ref_filter_matches_none_always_true() {
        assert!(RefFilter::None.matches("main"));
        assert!(RefFilter::None.matches("any/path/at/all.rs"));
    }

    #[test]
    fn ref_filter_matches_include_literal() {
        let f = include(&["main"]);
        assert!(f.matches("main"));
        assert!(!f.matches("feat"));
    }

    #[test]
    fn ref_filter_matches_include_with_negation_subtracts() {
        // GHA docs example: branches: [releases/**, !releases/**-alpha]
        // matches "releases/10" but NOT "releases/10-alpha".
        let f = include(&["releases/**", "!releases/**-alpha"]);
        assert!(f.matches("releases/10"));
        assert!(!f.matches("releases/10-alpha"));
        assert!(f.matches("releases/beta/mona"));
        assert!(!f.matches("releases/beta/3-alpha"));
    }

    #[test]
    fn ref_filter_matches_leading_negation_is_noop() {
        // ['!a', 'b'] — leading `!` has no positive set to subtract from, so
        // it is a no-op. The list reduces to an effective `[b]`.
        let f = include(&["!a", "b"]);
        assert!(f.matches("b"));
        assert!(!f.matches("a"));
        assert!(!f.matches("c"));
    }

    #[test]
    fn ref_filter_matches_exclude_branches_ignore_form() {
        // branches-ignore: [main] — fires for every ref except "main".
        let f = exclude(&["main"]);
        assert!(f.matches("feat"));
        assert!(!f.matches("main"));
    }

    #[test]
    fn ref_filter_matches_exclude_paths_ignore_single_file_changeset() {
        // paths-ignore: [docs/**] — under the single-file changeset
        // interpretation, a push of just "docs/x.md" is fully ignored
        // (filter rejects), while "src/foo.rs" passes through.
        let f = exclude(&["docs/**"]);
        assert!(f.matches("src/foo.rs"));
        assert!(!f.matches("docs/x.md"));
        assert!(!f.matches("docs/sub/y.md"));
    }

    #[test]
    fn ref_filter_matches_malformed_pattern_returns_false_no_panic() {
        // An unmatched bracket is invalid glob syntax. globset rejects it;
        // glob_match emits a stderr warning and returns false rather than
        // panicking, so the trace command keeps running on bad workflow YAML.
        let f = include(&["["]);
        assert!(!f.matches("anything"));
    }
}
