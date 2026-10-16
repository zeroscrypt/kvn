//! The connection state machine.
//!
//! Pure logic: it consumes [`Event`]s, updates its [`ConnectionState`] and
//! returns the [`Action`]s the executor must perform (spawn/kill the
//! engine, run a health check, schedule a retry). No I/O here, so every
//! transition — including failure and retry paths — is unit-tested.
//!
//! Invariants:
//! - `Connected` is only entered after an explicit `HealthCheckPassed` —
//!   a spawned process is never presented as a working tunnel.
//! - Automatic reconnection is bounded: at most `RetryPolicy::max_attempts`
//!   consecutive spawn attempts with exponential backoff.
//! - Non-retryable failures (bad config, missing engine, auth) go straight
//!   to `Error` and never loop.

/// What the executor should do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    SpawnEngine,
    KillEngine,
    RunHealthCheck,
    ScheduleRetry { delay_ms: u64, attempt: u32 },
    CancelRetry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// No usable Xray binary was found or installed.
    EngineMissing,
    /// The engine process failed to start or died unexpectedly.
    EngineFailed,
    /// The generated config was rejected by the engine.
    ConfigInvalid,
    /// The engine is up but traffic through it does not work.
    HealthCheckFailed,
    /// The server/network is unreachable.
    NetworkUnreachable,
    /// The server rejected our credentials.
    AuthFailed,
    /// Anything unexpected; treated as retryable once.
    Internal,
}

impl ErrorKind {
    pub fn is_retryable(self) -> bool {
        matches!(
            self,
            ErrorKind::EngineFailed
                | ErrorKind::HealthCheckFailed
                | ErrorKind::NetworkUnreachable
                | ErrorKind::Internal
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    ConnectRequested {
        profile_id: String,
    },
    /// Process is up and the local proxy port accepts connections.
    EngineReady,
    EngineSpawnFailed {
        kind: ErrorKind,
        message: String,
    },
    HealthCheckPassed {
        latency_ms: u32,
    },
    HealthCheckFailed {
        kind: ErrorKind,
        message: String,
    },
    /// The engine process exited (crash or external kill).
    EngineExited {
        code: Option<i32>,
    },
    StopRequested,
    /// The engine process has fully stopped (after `KillEngine`).
    EngineStopped,
    RetryTimerFired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting {
        profile_id: String,
        attempt: u32,
        engine_ready: bool,
    },
    Connected {
        profile_id: String,
        latency_ms: Option<u32>,
    },
    Reconnecting {
        profile_id: String,
        attempt: u32,
    },
    Stopping {
        pending_profile: Option<String>,
    },
    Error {
        kind: ErrorKind,
        message: String,
        retryable: bool,
    },
}

impl ConnectionState {
    pub fn label(&self) -> &'static str {
        match self {
            ConnectionState::Disconnected => "disconnected",
            ConnectionState::Connecting { .. } => "connecting",
            ConnectionState::Connected { .. } => "connected",
            ConnectionState::Reconnecting { .. } => "reconnecting",
            ConnectionState::Stopping { .. } => "stopping",
            ConnectionState::Error { .. } => "error",
        }
    }

    /// True when an engine process is expected to exist (or be starting).
    pub fn engine_involved(&self) -> bool {
        matches!(
            self,
            ConnectionState::Connecting { .. }
                | ConnectionState::Connected { .. }
                | ConnectionState::Reconnecting { .. }
                | ConnectionState::Stopping { .. }
        )
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub base_delay_ms: u64,
    pub factor: f64,
    pub cap_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        RetryPolicy {
            max_attempts: 5,
            base_delay_ms: 1_000,
            factor: 2.0,
            cap_ms: 30_000,
        }
    }
}

impl RetryPolicy {
    /// Backoff before the attempt following `attempts_done` failures:
    /// base * factor^(n-1), capped.
    pub fn delay_ms(&self, attempts_done: u32) -> u64 {
        let exp = attempts_done.saturating_sub(1) as i32;
        let raw = self.base_delay_ms as f64 * self.factor.powi(exp);
        (raw as u64).min(self.cap_ms)
    }
}

#[derive(Debug)]
pub struct StateMachine {
    state: ConnectionState,
    attempts: u32,
    policy: RetryPolicy,
}

impl Default for StateMachine {
    fn default() -> Self {
        Self::new(RetryPolicy::default())
    }
}

impl StateMachine {
    pub fn new(policy: RetryPolicy) -> Self {
        StateMachine {
            state: ConnectionState::Disconnected,
            attempts: 0,
            policy,
        }
    }

    pub fn state(&self) -> &ConnectionState {
        &self.state
    }

    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    pub fn handle(&mut self, event: Event) -> Vec<Action> {
        use ConnectionState as S;
        use Event as E;

        match (self.state.clone(), event) {
            (S::Disconnected, E::ConnectRequested { profile_id }) => self.begin(profile_id),
            (S::Error { .. }, E::ConnectRequested { profile_id }) => self.begin(profile_id),
            (S::Connected { .. }, E::ConnectRequested { profile_id })
            | (S::Connecting { .. }, E::ConnectRequested { profile_id })
            | (S::Reconnecting { .. }, E::ConnectRequested { profile_id }) => {
                // Switching servers while busy: stop first, then the
                // Stopping→EngineStopped transition starts the new one.
                self.state = S::Stopping {
                    pending_profile: Some(profile_id),
                };
                self.attempts = 0;
                vec![Action::KillEngine, Action::CancelRetry]
            }

            (
                S::Connecting {
                    profile_id,
                    attempt,
                    engine_ready: false,
                },
                E::EngineReady,
            ) => {
                self.state = S::Connecting {
                    profile_id,
                    attempt,
                    engine_ready: true,
                };
                vec![Action::RunHealthCheck]
            }

            (
                S::Connecting {
                    profile_id,
                    attempt,
                    ..
                },
                E::HealthCheckPassed { latency_ms },
            ) => {
                self.state = S::Connected {
                    profile_id,
                    latency_ms: Some(latency_ms),
                };
                self.attempts = 0;
                let _ = attempt;
                vec![Action::CancelRetry]
            }

            (S::Connecting { profile_id, .. }, E::HealthCheckFailed { kind, message })
            | (S::Connecting { profile_id, .. }, E::EngineSpawnFailed { kind, message }) => {
                self.fail(profile_id, kind, message)
            }

            (S::Connected { profile_id, .. }, E::EngineExited { code })
            | (S::Connecting { profile_id, .. }, E::EngineExited { code }) => {
                let message = match code {
                    Some(c) => format!("engine exited unexpectedly (code {c})"),
                    None => "engine exited unexpectedly".to_owned(),
                };
                self.fail(profile_id, ErrorKind::EngineFailed, message)
            }

            (S::Reconnecting { .. }, E::EngineStopped)
            | (S::Stopping { .. }, E::EngineExited { .. }) => {
                // Expected teardown noise; nothing to do.
                vec![]
            }

            (
                S::Reconnecting {
                    profile_id,
                    attempt,
                },
                E::RetryTimerFired,
            ) => {
                self.attempts = attempt;
                self.state = S::Connecting {
                    profile_id,
                    attempt,
                    engine_ready: false,
                };
                vec![Action::SpawnEngine]
            }

            (state, E::StopRequested) => match state {
                S::Disconnected | S::Error { .. } => {
                    self.attempts = 0;
                    self.state = S::Disconnected;
                    vec![Action::CancelRetry]
                }
                S::Stopping { .. } => vec![],
                _ => {
                    self.state = S::Stopping {
                        pending_profile: None,
                    };
                    vec![Action::KillEngine, Action::CancelRetry]
                }
            },

            (S::Stopping { pending_profile }, E::EngineStopped) => match pending_profile {
                Some(profile_id) => self.begin(profile_id),
                None => {
                    self.attempts = 0;
                    self.state = S::Disconnected;
                    vec![]
                }
            },

            // Anything else is stale (e.g. a health check result arriving
            // after the user pressed Stop). Ignoring is correct; the
            // executor logs it for diagnostics.
            _ => vec![],
        }
    }

    fn begin(&mut self, profile_id: String) -> Vec<Action> {
        self.attempts = 1;
        self.state = ConnectionState::Connecting {
            profile_id,
            attempt: 1,
            engine_ready: false,
        };
        vec![Action::CancelRetry, Action::SpawnEngine]
    }

    fn fail(&mut self, profile_id: String, kind: ErrorKind, message: String) -> Vec<Action> {
        let retryable = kind.is_retryable();
        if retryable && self.attempts < self.policy.max_attempts {
            let next_attempt = self.attempts + 1;
            let delay = self.policy.delay_ms(self.attempts);
            let mut actions = vec![Action::KillEngine];
            actions.push(Action::ScheduleRetry {
                delay_ms: delay,
                attempt: next_attempt,
            });
            self.state = ConnectionState::Reconnecting {
                profile_id,
                attempt: next_attempt,
            };
            actions
        } else {
            self.attempts = 0;
            self.state = ConnectionState::Error {
                kind,
                message,
                retryable,
            };
            vec![Action::KillEngine, Action::CancelRetry]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sm() -> StateMachine {
        StateMachine::default()
    }

    fn connect(sm: &mut StateMachine, id: &str) -> Vec<Action> {
        sm.handle(Event::ConnectRequested {
            profile_id: id.into(),
        })
    }

    fn assert_connected(sm: &StateMachine) {
        assert!(
            matches!(sm.state(), ConnectionState::Connected { .. }),
            "expected connected, got {:?}",
            sm.state()
        );
    }

    #[test]
    fn happy_path() {
        let mut m = sm();
        let actions = connect(&mut m, "p1");
        assert_eq!(actions, vec![Action::CancelRetry, Action::SpawnEngine]);
        assert_eq!(
            m.state(),
            &ConnectionState::Connecting {
                profile_id: "p1".into(),
                attempt: 1,
                engine_ready: false
            }
        );

        let actions = m.handle(Event::EngineReady);
        assert_eq!(actions, vec![Action::RunHealthCheck]);

        let actions = m.handle(Event::HealthCheckPassed { latency_ms: 42 });
        assert_eq!(actions, vec![Action::CancelRetry]);
        assert_connected(&m);
        assert_eq!(m.attempts(), 0);
    }

    #[test]
    fn connected_is_not_claimed_before_health_check() {
        let mut m = sm();
        connect(&mut m, "p1");
        m.handle(Event::EngineReady);
        // Still only Connecting until the check passes.
        assert_eq!(m.state().label(), "connecting");
    }

    #[test]
    fn retry_with_backoff_and_max_attempts() {
        let mut m = StateMachine::new(RetryPolicy {
            max_attempts: 3,
            base_delay_ms: 100,
            factor: 2.0,
            cap_ms: 1000,
        });
        connect(&mut m, "p1");
        m.handle(Event::EngineReady);

        // Failure 1 → retry in 100ms, attempt 2.
        let actions = m.handle(Event::HealthCheckFailed {
            kind: ErrorKind::HealthCheckFailed,
            message: "no traffic".into(),
        });
        assert_eq!(
            actions,
            vec![
                Action::KillEngine,
                Action::ScheduleRetry {
                    delay_ms: 100,
                    attempt: 2
                }
            ]
        );

        m.handle(Event::RetryTimerFired);
        assert_eq!(
            m.state(),
            &ConnectionState::Connecting {
                profile_id: "p1".into(),
                attempt: 2,
                engine_ready: false
            }
        );

        // Failure 2 → 200ms, attempt 3.
        let actions = m.handle(Event::HealthCheckFailed {
            kind: ErrorKind::NetworkUnreachable,
            message: "timeout".into(),
        });
        assert_eq!(
            actions,
            vec![
                Action::KillEngine,
                Action::ScheduleRetry {
                    delay_ms: 200,
                    attempt: 3
                }
            ]
        );

        // Failure 3 = max reached → Error, no further retry.
        m.handle(Event::RetryTimerFired);
        let actions = m.handle(Event::HealthCheckFailed {
            kind: ErrorKind::NetworkUnreachable,
            message: "timeout".into(),
        });
        assert_eq!(actions, vec![Action::KillEngine, Action::CancelRetry]);
        match m.state() {
            ConnectionState::Error {
                kind, retryable, ..
            } => {
                assert_eq!(*kind, ErrorKind::NetworkUnreachable);
                assert!(*retryable);
            }
            other => panic!("expected Error, got {other:?}"),
        }
        // Pressing Connect from Error starts a fresh cycle.
        let actions = connect(&mut m, "p1");
        assert_eq!(actions, vec![Action::CancelRetry, Action::SpawnEngine]);
        assert_eq!(m.attempts(), 1);
    }

    #[test]
    fn non_retryable_failure_goes_straight_to_error() {
        let mut m = sm();
        connect(&mut m, "p1");
        let actions = m.handle(Event::EngineSpawnFailed {
            kind: ErrorKind::ConfigInvalid,
            message: "unknown field".into(),
        });
        assert_eq!(actions, vec![Action::KillEngine, Action::CancelRetry]);
        match m.state() {
            ConnectionState::Error {
                kind, retryable, ..
            } => {
                assert_eq!(*kind, ErrorKind::ConfigInvalid);
                assert!(!*retryable);
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn engine_crash_while_connected_triggers_reconnect() {
        let mut m = sm();
        connect(&mut m, "p1");
        m.handle(Event::EngineReady);
        m.handle(Event::HealthCheckPassed { latency_ms: 10 });
        assert_connected(&m);

        let actions = m.handle(Event::EngineExited { code: Some(1) });
        assert!(matches!(actions.last(), Some(Action::ScheduleRetry { .. })));
        assert_eq!(m.state().label(), "reconnecting");

        m.handle(Event::RetryTimerFired);
        assert_eq!(m.state().label(), "connecting");
    }

    #[test]
    fn stop_from_each_state() {
        // From disconnected.
        let mut m = sm();
        assert_eq!(m.handle(Event::StopRequested), vec![Action::CancelRetry]);
        assert_eq!(m.state(), &ConnectionState::Disconnected);

        // From connecting: kill, then Stopped → Disconnected.
        let mut m = sm();
        connect(&mut m, "p1");
        let actions = m.handle(Event::StopRequested);
        assert_eq!(actions, vec![Action::KillEngine, Action::CancelRetry]);
        assert_eq!(
            m.state(),
            &ConnectionState::Stopping {
                pending_profile: None
            }
        );
        let actions = m.handle(Event::EngineStopped);
        assert!(actions.is_empty());
        assert_eq!(m.state(), &ConnectionState::Disconnected);

        // From connected.
        let mut m = sm();
        connect(&mut m, "p1");
        m.handle(Event::EngineReady);
        m.handle(Event::HealthCheckPassed { latency_ms: 1 });
        m.handle(Event::StopRequested);
        m.handle(Event::EngineStopped);
        assert_eq!(m.state(), &ConnectionState::Disconnected);
    }

    #[test]
    fn stale_health_result_after_stop_is_ignored() {
        let mut m = sm();
        connect(&mut m, "p1");
        m.handle(Event::EngineReady);
        m.handle(Event::StopRequested);
        // Health check task finishes after the user stopped: must not flip
        // us back to Connected.
        let actions = m.handle(Event::HealthCheckPassed { latency_ms: 5 });
        assert!(actions.is_empty());
        assert_ne!(m.state().label(), "connected");
    }

    #[test]
    fn switching_profile_while_connected_stops_then_starts() {
        let mut m = sm();
        connect(&mut m, "p1");
        m.handle(Event::EngineReady);
        m.handle(Event::HealthCheckPassed { latency_ms: 1 });

        let actions = connect(&mut m, "p2");
        assert_eq!(actions, vec![Action::KillEngine, Action::CancelRetry]);
        assert_eq!(
            m.state(),
            &ConnectionState::Stopping {
                pending_profile: Some("p2".into())
            }
        );

        let actions = m.handle(Event::EngineStopped);
        assert_eq!(actions, vec![Action::CancelRetry, Action::SpawnEngine]);
        assert_eq!(
            m.state(),
            &ConnectionState::Connecting {
                profile_id: "p2".into(),
                attempt: 1,
                engine_ready: false
            }
        );
    }

    #[test]
    fn backoff_delays_are_bounded() {
        let policy = RetryPolicy {
            max_attempts: 10,
            base_delay_ms: 1000,
            factor: 2.0,
            cap_ms: 5000,
        };
        assert_eq!(policy.delay_ms(1), 1000);
        assert_eq!(policy.delay_ms(2), 2000);
        assert_eq!(policy.delay_ms(3), 4000);
        assert_eq!(policy.delay_ms(4), 5000); // capped
        assert_eq!(policy.delay_ms(9), 5000);
    }
}
