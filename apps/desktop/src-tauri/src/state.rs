//! Boot state machine for the desktop shell.

/// Observable boot phase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootPhase {
    /// Process has not spawned yet.
    Idle,
    /// Sidecar process is alive; port unknown.
    Spawned,
    /// Ready line reported a loopback port.
    Bound {
        /// OS-assigned listen port.
        port: u16,
    },
    /// GET `/` returned `__DSH_BOOT__`.
    LoaderReady {
        /// OS-assigned listen port.
        port: u16,
    },
    /// WebView is showing the sidecar URL.
    Visible {
        /// OS-assigned listen port.
        port: u16,
    },
    /// Terminal failure; the splash stays on the error page.
    Failed {
        /// User-visible failure text (never a token).
        reason: String,
    },
}

/// Input event for [`transition`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootEvent {
    /// Spawn syscall succeeded.
    SpawnOk,
    /// Ready line parsed.
    Bound {
        /// OS-assigned listen port.
        port: u16,
    },
    /// Index health check succeeded.
    LoaderReady,
    /// WebView navigation completed.
    Visible,
    /// Any step failed.
    Failed {
        /// User-visible failure text.
        reason: String,
    },
}

/// Apply one boot event. Illegal combinations become [`BootPhase::Failed`].
///
/// # Parameters
/// - `phase`: current phase.
/// - `event`: observed event.
///
/// # Returns
/// The next phase.
pub fn transition(phase: BootPhase, event: BootEvent) -> BootPhase {
    if let BootEvent::Failed { reason } = event {
        return BootPhase::Failed { reason };
    }
    match (phase, event) {
        (BootPhase::Idle, BootEvent::SpawnOk) => BootPhase::Spawned,
        (BootPhase::Spawned, BootEvent::Bound { port }) => BootPhase::Bound { port },
        (BootPhase::Bound { port }, BootEvent::LoaderReady) => BootPhase::LoaderReady { port },
        (BootPhase::LoaderReady { port }, BootEvent::Visible) => BootPhase::Visible { port },
        (other, _) => BootPhase::Failed {
            reason: format!("illegal boot transition from {other:?}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path_without_host_describe() {
        let phase = transition(BootPhase::Idle, BootEvent::SpawnOk);
        let phase = transition(phase, BootEvent::Bound { port: 9 });
        let phase = transition(phase, BootEvent::LoaderReady);
        let phase = transition(phase, BootEvent::Visible);
        assert_eq!(phase, BootPhase::Visible { port: 9 });
    }

    #[test]
    fn failure_is_terminal() {
        let phase = transition(
            BootPhase::Spawned,
            BootEvent::Failed {
                reason: "ready line timed out".into(),
            },
        );
        assert!(matches!(phase, BootPhase::Failed { .. }));
    }
}
