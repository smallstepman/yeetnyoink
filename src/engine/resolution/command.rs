use crate::engine::contracts::TearResult;

/// Assembles a full terminal+mux attach command by prepending the terminal's
/// launch prefix to mux-provided attach arguments.
pub fn spawn_attach_command(
    aliases: &[&str],
    terminal_launch_prefix: &[&str],
    target: String,
) -> Option<Vec<String>> {
    let mux_args = crate::adapters::terminal_multiplexers::active_mux_provider(aliases)
        .mux_attach_args(target)?;
    let mut command: Vec<String> = terminal_launch_prefix
        .iter()
        .map(|segment| segment.to_string())
        .collect();
    command.extend(mux_args);
    Some(command)
}

/// Prepends the terminal host's launch prefix to `tear.spawn_command` in-place,
/// wrapping the mux attach command with the terminal binary invocation.
pub fn prepend_terminal_launch_prefix(
    terminal_launch_prefix: &[&str],
    mut tear: TearResult,
) -> TearResult {
    if let Some(mux_args) = tear.spawn_command.take() {
        let mut command: Vec<String> = terminal_launch_prefix
            .iter()
            .map(|segment| segment.to_string())
            .collect();
        command.extend(mux_args);
        tear.spawn_command = Some(command);
    }
    tear
}
