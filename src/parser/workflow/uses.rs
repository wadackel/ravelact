use crate::ir::*;
use anyhow::{anyhow, Result};

/// Parse the path portion of a `docker://` URI into a [`DockerRef`].
///
/// Format: `[host/]image[:tag]`
///
/// The registry host is detected by the presence of a `.` or `:` in the first
/// path segment, or when the segment is exactly `localhost`. Docker Hub images
/// have no host (e.g. `alpine:3.8`). The tag is separated by the last `:` in
/// the image component and is optional (e.g. `ghcr.io/owner/image` with no tag).
pub(super) fn parse_docker_ref(rest: &str) -> DockerRef {
    // Determine host vs image boundary.
    let (host, image_and_tag) = if let Some(slash_pos) = rest.find('/') {
        let first_seg = &rest[..slash_pos];
        let is_host =
            first_seg.contains('.') || first_seg.contains(':') || first_seg == "localhost";
        if is_host {
            (Some(first_seg.to_string()), &rest[slash_pos + 1..])
        } else {
            (None, rest)
        }
    } else {
        (None, rest)
    };

    // Split tag from the last ':' in the image component.
    let (image, tag) = if let Some(colon_pos) = image_and_tag.rfind(':') {
        // Ensure the colon is not part of a port in the host (already stripped above)
        // and that it belongs to the image/tag boundary. Since `image_and_tag` no
        // longer contains the host, any ':' here is a tag separator.
        (
            image_and_tag[..colon_pos].to_string(),
            Some(image_and_tag[colon_pos + 1..].to_string()),
        )
    } else {
        (image_and_tag.to_string(), None)
    };

    DockerRef { host, image, tag }
}

fn normalize_local_uses_path(rest: &str) -> String {
    let normalized = rest.trim_end_matches('/');
    if normalized.is_empty() {
        ".".to_string()
    } else {
        normalized.to_string()
    }
}

/// Parse a `uses:` string into a [`UsesRef`].
pub(crate) fn parse_uses(s: &str) -> Result<UsesRef> {
    let s = s.trim();

    if let Some(rest) = s.strip_prefix("docker://") {
        return Ok(UsesRef::Docker(parse_docker_ref(rest)));
    }

    if let Some(rest) = s.strip_prefix("./") {
        let normalized = normalize_local_uses_path(rest);
        if normalized.starts_with(".github/workflows/")
            && (normalized.ends_with(".yml") || normalized.ends_with(".yaml"))
        {
            return Ok(UsesRef::LocalWorkflow(WorkflowId(normalized)));
        }
        return Ok(UsesRef::LocalAction(ActionId(normalized)));
    }

    let (path_part, gitref) = s
        .split_once('@')
        .ok_or_else(|| anyhow!("uses: missing @ref in {s}"))?;
    let mut segs = path_part.splitn(3, '/');
    let owner = segs
        .next()
        .ok_or_else(|| anyhow!("uses: missing owner in {s}"))?
        .to_string();
    let repo = segs
        .next()
        .ok_or_else(|| anyhow!("uses: missing repo in {s}"))?
        .to_string();
    let subpath = segs.next().map(|s| s.to_string());
    Ok(UsesRef::External {
        owner,
        repo,
        subpath,
        gitref: gitref.to_string(),
    })
}

pub(super) fn parse_workflow_ref(s: &str) -> Result<WorkflowRef> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("./") {
        let normalized = normalize_local_uses_path(rest);
        return Ok(WorkflowRef::Local(WorkflowId(normalized)));
    }
    let (path_part, gitref) = s
        .split_once('@')
        .ok_or_else(|| anyhow!("workflow uses: missing @ref in {s}"))?;
    let mut segs = path_part.splitn(3, '/');
    let owner = segs
        .next()
        .ok_or_else(|| anyhow!("workflow uses: missing owner in {s}"))?
        .to_string();
    let repo = segs
        .next()
        .ok_or_else(|| anyhow!("workflow uses: missing repo in {s}"))?
        .to_string();
    let path = segs.next().unwrap_or("").to_string();
    Ok(WorkflowRef::External {
        owner,
        repo,
        path,
        gitref: gitref.to_string(),
    })
}
