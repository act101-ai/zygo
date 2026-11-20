#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    Ok = 0,
    Usage = 2,
    ConfigInvalid = 11,
    ServerUnreachable = 12,
    AgentNotFound = 13,
}
