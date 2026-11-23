use super::*;

fn fixture(passes: u8) -> Transport {
    // Independent values, including signed rails. No WAVE decoder/producer is
    // used as this transport oracle; original-file qualification is separate.
    let mut pcm = vec![0.0; 10000 * 2];
    for (i, n) in [-8388608, -1, 0, 1, 8388607].into_iter().enumerate() {
        pcm[4800 * 2 + i] = n as f32 / 8388608.0;
    }
    Transport::new(pcm.into(), "Texto español\n".into(), [7; 32], passes, 0).unwrap()
}
fn started(passes: u8) -> Transport {
    let mut t = fixture(passes);
    t.enqueue("start").unwrap();
    assert_eq!(t.poll(0, Notice::None).open, Some(1));
    t.poll(1, Notice::Opened { generation: 1 });
    assert_eq!(t.snapshot().state, State::Playing);
    t
}
fn command(t: &mut Transport, line: &str, at: u64) -> Effects {
    t.enqueue(line).unwrap();
    t.poll(at, Notice::None)
}

#[test]
fn exact_pcm24_conversion_and_finite_passes() {
    for passes in 1..=3 {
        let mut t = started(passes);
        let mut expected = Vec::new();
        for pass in 0..passes {
            if pass > 0 {
                expected.extend(vec![0.0; INTERVAL_FRAMES as usize * 2])
            }
            expected.extend(t.samples.iter().copied());
        }
        let mut actual = Vec::new();
        // Deliberately not a divisor of either boundary: the interval must
        // cross callbacks without extra padding or skipped source samples.
        let mut out = vec![77.0; 1022];
        let mut now = 2;
        while t.snapshot().state != State::Completed {
            t.fill(1, now, &mut out).unwrap();
            actual.extend_from_slice(&out);
            now += 10;
        }
        assert_eq!(&actual[..expected.len()], expected);
        assert!(actual[expected.len()..].iter().all(|n| *n == 0.0));
        assert_eq!(t.snapshot().source_frame, 10000);
        assert_eq!(t.snapshot().pass, passes);
        assert_eq!(t.snapshot().interval_frame, 0);
        assert_eq!(t.transcript(), "Texto español\n");
        let before = t.snapshot();
        t.fill(1, now, &mut out).unwrap();
        assert!(out.iter().all(|n| *n == 0.0));
        assert_eq!(t.snapshot(), before);
    }
}
#[test]
fn pause_preserves_source_and_interval_cursors() {
    for interval in [false, true] {
        let mut t = started(2);
        let mut out = [0.0; 2000];
        let count = if interval { 11 } else { 3 };
        for n in 0..count {
            t.fill(1, 2 + n * 10, &mut out).unwrap();
        }
        assert_eq!(
            t.snapshot().state,
            if interval {
                State::LoopWait
            } else {
                State::Playing
            }
        );
        command(&mut t, "pause", 120);
        let paused = t.snapshot();
        t.fill(1, 5000, &mut out).unwrap();
        t.fill(1, 6000, &mut out).unwrap();
        assert_eq!(t.snapshot(), paused);
        assert!(out.iter().all(|v| *v == 0.0));
        command(&mut t, "resume", 6001);
        t.fill(1, 6002, &mut out).unwrap();
        if interval {
            assert_eq!(t.snapshot().interval_frame, paused.interval_frame + 1000)
        } else {
            assert_eq!(t.snapshot().source_frame, paused.source_frame + 1000)
        }
    }
}
#[test]
fn command_order_and_old_generation_refusal() {
    let mut a = started(2);
    let mut b = started(2);
    for t in [&mut a, &mut b] {
        let mut out = [0.0; 2000];
        for n in 0..9 {
            t.fill(1, 2 + n * 10, &mut out).unwrap();
        }
        command(t, "stop", 100);
        t.fill(1, 101, &mut out).unwrap();
        assert!(out.iter().all(|v| *v == 0.0));
        assert_eq!(t.snapshot().pass, 1);
        assert_eq!(t.snapshot().source_frame, 9000);
        assert_eq!(command(t, "restart", 102).recheck, Some(2));
        let snapshot = t.snapshot();
        assert_eq!(
            t.fill(1, 103, &mut out),
            Err(TransportError::StaleGeneration)
        );
        assert_eq!(t.snapshot(), snapshot);
        assert!(out.iter().all(|n| *n == 0.0));
    }
    assert_eq!(a.snapshot(), b.snapshot());
    assert_eq!(
        serde_json::to_vec(a.trace()).unwrap(),
        serde_json::to_vec(b.trace()).unwrap()
    );
    a.enqueue("restart").unwrap();
    a.enqueue("quit").unwrap();
    let effects = a.poll(103, Notice::BackendFault { generation: 2 });
    assert_eq!(a.snapshot().state, State::Fault);
    assert!(effects.quit && effects.close);
}
#[test]
fn restart_requires_fresh_checks_and_rejects_late_cancelled_results() {
    let mut t = started(1);
    command(&mut t, "restart", 2);
    t.poll(3, Notice::Opened { generation: 2 });
    assert_eq!(t.snapshot().fault, Some(TransportError::UnverifiedRestart));
    let mut t = started(1);
    command(&mut t, "restart", 2);
    assert_eq!(
        t.accept_restart(
            RestartPermit {
                session: t.session,
                generation: 2,
                seal: [8; 32]
            },
            3
        ),
        Err(TransportError::UnverifiedRestart)
    );
    let mut t = started(1);
    command(&mut t, "restart", 2);
    command(&mut t, "stop", 3);
    assert_eq!(
        t.accept_restart(
            RestartPermit {
                session: t.session,
                generation: 2,
                seal: [7; 32]
            },
            4
        ),
        Err(TransportError::StaleGeneration)
    );
    assert_eq!(t.snapshot().state, State::Stopped);
    let mut t = started(1);
    command(&mut t, "restart", 2);
    assert_eq!(
        t.accept_restart(
            RestartPermit {
                session: t.session,
                generation: 2,
                seal: [7; 32]
            },
            30000
        )
        .unwrap()
        .open,
        Some(2)
    );
    t.poll(30001, Notice::Opened { generation: 2 });
    assert_eq!(t.snapshot().state, State::Playing);
    assert_eq!(t.snapshot().source_frame, 0);
    let mut t = started(1);
    command(&mut t, "restart", 2);
    assert_eq!(
        t.accept_restart(
            RestartPermit {
                session: t.session,
                generation: 2,
                seal: [7; 32]
            },
            35002
        ),
        Err(TransportError::StaleGeneration)
    );
    assert_eq!(t.snapshot().fault, Some(TransportError::RestartExpired));
}
#[test]
fn limits_fault_cleanup_and_no_auto_resume() {
    let mut t = started(1);
    t.enqueue("pause").unwrap();
    t.poll(251, Notice::None);
    assert_eq!(t.snapshot().fault, Some(TransportError::CallbackStalled));
    assert!(t.enqueue("resume").is_err());
    t.poll(252, Notice::Opened { generation: 1 });
    assert_eq!(t.snapshot().state, State::Fault);
    let mut t = fixture(1);
    command(&mut t, "start", 0);
    t.poll(2000, Notice::Opened { generation: 1 });
    assert_eq!(t.snapshot().fault, Some(TransportError::StartupExpired));
    let mut t = fixture(1);
    assert!(t.poll(MAX_SESSION_MS, Notice::None).quit);
    assert!(t.snapshot().session_closed);
    assert!(t.enqueue("start").is_err());
    let mut t = started(1);
    let mut invalid = [1.0; 3];
    assert_eq!(
        t.fill(1, 2, &mut invalid),
        Err(TransportError::InvalidBuffer)
    );
    assert_eq!(invalid, [0.0; 3]);
    assert!(t.poll(3, Notice::None).close);
    for size in [0, MAX_CALLBACK_FRAMES * 2 + 2] {
        let mut t = started(1);
        let mut out = vec![1.0; size];
        assert_eq!(t.fill(1, 2, &mut out), Err(TransportError::InvalidBuffer));
        assert!(out.iter().all(|n| *n == 0.0));
        assert!(t.poll(3, Notice::None).close);
    }
    let mut t = started(1);
    t.poll(0, Notice::None);
    assert_eq!(t.snapshot().fault, Some(TransportError::ClockReversed));
    let mut t = started(1);
    t.poll(2, Notice::BackendFault { generation: 0 });
    assert_eq!(t.snapshot().state, State::Playing);
    t.poll(3, Notice::BackendFault { generation: 1 });
    assert_eq!(t.snapshot().fault, Some(TransportError::BackendFault));
}

#[test]
fn identical_source_sessions_and_duplicate_permits_cannot_cross() {
    let mut a = started(1);
    let mut b = started(1);
    command(&mut a, "restart", 2);
    command(&mut b, "restart", 2);
    let ta = a.restart_ticket().unwrap();
    let foreign = RestartPermit {
        session: ta.session,
        generation: ta.generation,
        seal: ta.seal,
    };
    let before = b.snapshot();
    assert_eq!(
        b.accept_restart(foreign, 3),
        Err(TransportError::StaleGeneration)
    );
    assert_eq!(b.snapshot(), before);
    let first = b.restart_ticket().unwrap();
    let second = b.restart_ticket().unwrap();
    let permit = |t: RestartTicket| RestartPermit {
        session: t.session,
        generation: t.generation,
        seal: t.seal,
    };
    assert_eq!(b.accept_restart(permit(first), 4).unwrap().open, Some(2));
    assert_eq!(
        b.accept_restart(permit(second), 5),
        Err(TransportError::StaleGeneration)
    );
    assert!(b.restart_ticket().is_err());
    b.poll(6, Notice::Opened { generation: 2 });
    assert_eq!(b.snapshot().state, State::Playing);
}
#[test]
fn bounded_queues_and_command_refusals() {
    let mut t = fixture(1);
    for _ in 0..8 {
        t.enqueue("status").unwrap();
    }
    assert_eq!(t.enqueue("start"), Err(TransportError::QueueFull));
    assert_eq!(t.snapshot().fault, Some(TransportError::QueueFull));
    t.poll(0, Notice::None);
    assert!(t.trace().len() <= 256);
    let mut t = fixture(1);
    for _ in 0..256 {
        command(&mut t, "status", 0);
    }
    command(&mut t, "status", 0);
    assert_eq!(t.trace().len(), 256);
    assert_eq!(t.snapshot().fault, Some(TransportError::TraceFull));
    assert!(command(&mut t, "quit", 1).quit);
    for line in [
        "",
        "advance",
        "iniciar",
        "\u{1b}[start",
        "start\nstop",
        "pause\t",
    ] {
        assert_eq!(Command::parse(line), Err(TransportError::InvalidCommand));
    }
    assert_eq!(Command::parse("  PaUsE\r\n"), Ok(Command::Pause));
    let mut t = fixture(1);
    let before = t.snapshot();
    command(&mut t, "resume", 0);
    assert_eq!(t.snapshot().state, before.state);
    assert_eq!(t.snapshot().source_frame, before.source_frame);
    assert_eq!(t.trace()[0].refusal, Some(TransportError::InvalidState));
}
#[test]
fn startup_stop_wins_and_lifetime_cannot_be_restarted() {
    let mut t = fixture(1);
    command(&mut t, "start", 0);
    t.enqueue("stop").unwrap();
    let e = t.poll(1, Notice::Opened { generation: 1 });
    assert_eq!(t.snapshot().state, State::Stopped);
    assert!(e.close && e.open.is_none());
    let mut t = started(1);
    command(&mut t, "pause", 2);
    command(&mut t, "resume", MAX_SESSION_MS);
    assert_eq!(t.snapshot().state, State::Stopped);
    assert!(t.snapshot().session_closed);
    assert!(t.enqueue("restart").is_err());
    assert_eq!(fixture(1).snapshot().state, State::Ready);
}
