use anyhow::{Context, Result};
use etcetera::base_strategy::{choose_base_strategy, BaseStrategy};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const MANAGED_SNIPPET_NAME: &str = "yeet-and-yoink.conf";
pub const MANAGED_SNIPPET: &str = "\
# yeet-and-yoink kitty integration
allow_remote_control socket-only
listen_on unix:@kitty-{kitty_pid}
";

#[derive(Debug, Clone)]
pub struct KittySetupPlan {
    pub kitty_conf_path: PathBuf,
    pub snippet_path: PathBuf,
    pub include_line: String,
    pub manual_command: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KittyIncludeStatus {
    Added,
    AlreadyPresent,
}

pub fn plan(kitty_conf_override: Option<&Path>) -> Result<KittySetupPlan> {
    let kitty_conf_path = resolve_kitty_conf_path(kitty_conf_override)?;
    let snippet_dir = kitty_conf_path
        .parent()
        .context("kitty.conf path has no parent directory")?;
    let snippet_path = snippet_dir.join(MANAGED_SNIPPET_NAME);
    let include_line = format!("include {}", snippet_path.display());
    let manual_command = manual_append_command(&kitty_conf_path, &include_line);
    Ok(KittySetupPlan {
        kitty_conf_path,
        snippet_path,
        include_line,
        manual_command,
    })
}

pub fn write_managed_snippet(plan: &KittySetupPlan) -> Result<()> {
    if let Some(parent) = plan.snippet_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create kitty snippet directory {}",
                parent.display()
            )
        })?;
    }
    fs::write(&plan.snippet_path, MANAGED_SNIPPET).with_context(|| {
        format!(
            "failed to write kitty setup snippet {}",
            plan.snippet_path.display()
        )
    })?;
    Ok(())
}

pub fn include_present(plan: &KittySetupPlan) -> Result<bool> {
    if !plan.kitty_conf_path.exists() {
        return Ok(false);
    }
    let contents = fs::read_to_string(&plan.kitty_conf_path).with_context(|| {
        format!(
            "failed to read kitty config {}",
            plan.kitty_conf_path.display()
        )
    })?;
    Ok(contents.lines().any(|line| line == plan.include_line))
}

pub fn append_include(plan: &KittySetupPlan) -> Result<KittyIncludeStatus> {
    if include_present(plan)? {
        return Ok(KittyIncludeStatus::AlreadyPresent);
    }

    if let Some(parent) = plan.kitty_conf_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("failed to create kitty config directory {}", parent.display())
        })?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&plan.kitty_conf_path)
        .with_context(|| {
            format!("failed to open kitty config {}", plan.kitty_conf_path.display())
        })?;
    write!(file, "\n{}\n", plan.include_line).with_context(|| {
        format!(
            "failed to append kitty include line to {}",
            plan.kitty_conf_path.display()
        )
    })?;
    Ok(KittyIncludeStatus::Added)
}

pub fn explanation(plan: &KittySetupPlan) -> String {
    format!(
        "Kitty needs a remote-control socket that detached `yny` invocations can reach.\n\n\
Add this snippet:\n\n\
{snippet}\n\n\
`yny setup kitty` wrote that snippet to:\n  {snippet_path}\n\n\
and kitty still needs this include line in:\n  {kitty_conf_path}\n\n\
{include_line}",
        snippet = indent_block(MANAGED_SNIPPET.trim_end(), "  "),
        snippet_path = plan.snippet_path.display(),
        kitty_conf_path = plan.kitty_conf_path.display(),
        include_line = indent_block(&plan.include_line, "  "),
    )
}

fn resolve_kitty_conf_path(kitty_conf_override: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = kitty_conf_override {
        return resolve_output_path(path, "--kitty-conf");
    }

    let strategy = choose_base_strategy().context("failed to resolve kitty config directory")?;
    Ok(strategy.config_dir().join("kitty").join("kitty.conf"))
}

fn resolve_output_path(path: &Path, flag_name: &str) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()
            .with_context(|| format!("failed to resolve current directory for {flag_name}"))?
            .join(path))
    }
}

fn manual_append_command(kitty_conf_path: &Path, include_line: &str) -> String {
    let parent = kitty_conf_path.parent().unwrap_or_else(|| Path::new("."));
    format!(
        "grep -Fqx {include_line} {kitty_conf} || {{ mkdir -p {kitty_dir} && touch {kitty_conf} && printf '\\n%s\\n' {include_line} >> {kitty_conf}; }}",
        include_line = shell_single_quote_str(include_line),
        kitty_conf = shell_single_quote(kitty_conf_path),
        kitty_dir = shell_single_quote(parent),
    )
}

fn shell_single_quote(path: &Path) -> String {
    shell_single_quote_str(&path.display().to_string())
}

fn shell_single_quote_str(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for ch in value.chars() {
        if ch == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(ch);
        }
    }
    quoted.push('\'');
    quoted
}

fn indent_block(value: &str, prefix: &str) -> String {
    value.lines()
        .map(|line| format!("{prefix}{line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::{append_include, explanation, include_present, plan, write_managed_snippet};
    use super::{KittyIncludeStatus, MANAGED_SNIPPET, MANAGED_SNIPPET_NAME};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_ID: AtomicU64 = AtomicU64::new(1);

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "yeet-and-yoink-kitty-setup-{prefix}-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("temp dir should be created");
        path
    }

    #[test]
    fn plan_uses_managed_snippet_next_to_kitty_conf() {
        let root = unique_temp_dir("plan");
        let kitty_conf = root.join("kitty").join("kitty.conf");
        let plan = plan(Some(&kitty_conf)).expect("kitty setup plan should be created");

        assert_eq!(plan.kitty_conf_path, kitty_conf);
        assert_eq!(
            plan.snippet_path,
            root.join("kitty").join(MANAGED_SNIPPET_NAME)
        );
        assert_eq!(
            plan.include_line,
            format!("include {}", plan.snippet_path.display())
        );
    }

    #[test]
    fn write_managed_snippet_writes_expected_content() {
        let root = unique_temp_dir("snippet");
        let kitty_conf = root.join("kitty.conf");
        let plan = plan(Some(&kitty_conf)).expect("kitty setup plan should be created");

        write_managed_snippet(&plan).expect("managed snippet should be written");

        assert_eq!(
            fs::read_to_string(&plan.snippet_path).expect("snippet should be readable"),
            MANAGED_SNIPPET
        );
    }

    #[test]
    fn append_include_is_idempotent() {
        let root = unique_temp_dir("append");
        let kitty_conf = root.join("kitty.conf");
        let plan = plan(Some(&kitty_conf)).expect("kitty setup plan should be created");

        assert!(!include_present(&plan).expect("presence check should work"));
        assert_eq!(
            append_include(&plan).expect("first append should succeed"),
            KittyIncludeStatus::Added
        );
        assert!(include_present(&plan).expect("presence check should work"));
        assert_eq!(
            append_include(&plan).expect("second append should detect existing include"),
            KittyIncludeStatus::AlreadyPresent
        );
    }

    #[test]
    fn explanation_mentions_snippet_and_include_line() {
        let root = unique_temp_dir("explanation");
        let kitty_conf = root.join("kitty.conf");
        let plan = plan(Some(&kitty_conf)).expect("kitty setup plan should be created");
        let text = explanation(&plan);

        assert!(text.contains("allow_remote_control socket-only"));
        assert!(text.contains("listen_on unix:@kitty-{kitty_pid}"));
        assert!(text.contains(&plan.include_line));
        assert!(text.contains(&plan.kitty_conf_path.display().to_string()));
    }
}
