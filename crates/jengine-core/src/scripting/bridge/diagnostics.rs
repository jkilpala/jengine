//! Common diagnostic types produced by every script adapter.
//!
//! All adapters must surface errors via [`ScriptError`] so editor tooling and runtime
//! logs can present them uniformly across Luau and JavaScript backends.

use std::fmt;

/// Identifies which adapter produced an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackendKind {
    Luau,
    JavaScript,
    /// Test-only no-op adapter.
    Noop,
}

impl fmt::Display for BackendKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BackendKind::Luau => write!(f, "luau"),
            BackendKind::JavaScript => write!(f, "javascript"),
            BackendKind::Noop => write!(f, "noop"),
        }
    }
}

/// Source location of a script error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLocation {
    /// 1-based line number.
    pub line: u32,
    /// 1-based column number, when available.
    pub column: Option<u32>,
}

/// Classification of a script error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptErrorKind {
    /// Source failed to parse / compile.
    Syntax,
    /// Script raised an error at runtime.
    Runtime,
    /// Bridge call rejected (bad arguments, missing entity, etc).
    BridgeCall,
    /// Script invoked a feature the host does not support.
    Capability,
}

/// A unified script error surfaced by every adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptError {
    pub backend: BackendKind,
    pub script_path: String,
    pub location: Option<SourceLocation>,
    pub kind: ScriptErrorKind,
    pub message: String,
    /// Which bridge call triggered the error, when applicable.
    pub bridge_call: Option<String>,
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.backend, self.script_path)?;
        if let Some(loc) = &self.location {
            write!(f, ":{}", loc.line)?;
            if let Some(col) = loc.column {
                write!(f, ":{}", col)?;
            }
        }
        write!(f, " ({:?}): {}", self.kind, self.message)?;
        if let Some(call) = &self.bridge_call {
            write!(f, " (in {})", call)?;
        }
        Ok(())
    }
}

impl std::error::Error for ScriptError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_with_location_and_call() {
        let e = ScriptError {
            backend: BackendKind::Luau,
            script_path: "player.luau".into(),
            location: Some(SourceLocation { line: 12, column: Some(5) }),
            kind: ScriptErrorKind::Runtime,
            message: "nil index".into(),
            bridge_call: Some("entity.set_position".into()),
        };
        let s = format!("{e}");
        assert!(s.contains("[luau]"));
        assert!(s.contains("player.luau:12:5"));
        assert!(s.contains("Runtime"));
        assert!(s.contains("nil index"));
        assert!(s.contains("entity.set_position"));
    }

    #[test]
    fn formats_minimal() {
        let e = ScriptError {
            backend: BackendKind::Noop,
            script_path: "x".into(),
            location: None,
            kind: ScriptErrorKind::Syntax,
            message: "bad".into(),
            bridge_call: None,
        };
        let s = format!("{e}");
        assert!(s.contains("[noop] x (Syntax): bad"));
    }
}
