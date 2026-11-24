#[test]
fn falha_desconhecida_nao_permite_retry_antes_de_quiescencia() {
    let mut s = ready();
    s.suspended().unwrap();
    let r = release(&mut s);
    s.release_failed(r).unwrap();
    s.resumed(B).unwrap();
    assert_eq!(
        s.phase(),
        Phase::ReleaseFailed {
            id: r,
            quiescent: false
        }
    );
    assert!(!s.can_render());
    assert_eq!(s.pending_action(), None);
    assert_eq!(s.retry(), Err(Error::NotQuiescent));
    assert_eq!(
        s.confirm_release_quiescent(r + 1),
        Err(Error::UnexpectedCompletion)
    );
    assert_eq!(s.retry(), Err(Error::NotQuiescent));
    s.confirm_release_quiescent(r).unwrap();
    assert_eq!(
        s.confirm_release_quiescent(r),
        Err(Error::UnexpectedCompletion)
    );
    s.retry().unwrap();
    let r2 = release(&mut s);
    assert_ne!(r, r2);
    assert_eq!(s.released(r), Err(Error::UnexpectedCompletion));
    assert_eq!(s.release_failed(r), Err(Error::UnexpectedCompletion));
    s.released(r2).unwrap();
    assert!(matches!(
        s.pending_action(),
        Some(Action::Create { dimensions: B, .. })
    ));
}

#[test]
fn falha_desconhecida_pode_ser_resolvida_por_confirmacao_real() {
    let mut s = ready();
    s.suspended().unwrap();
    let r = release(&mut s);
    s.release_failed(r).unwrap();
    s.released(r).unwrap();
    assert_eq!(s.phase(), Phase::Absent);
    assert_eq!(
        s.confirm_release_quiescent(r),
        Err(Error::UnexpectedCompletion)
    );
    assert_eq!(s.retry(), Err(Error::NotFailed));
}

#[test]
fn retry_de_liberacao_pode_ocorrer_suspenso_apos_quiescencia() {
    let mut s = ready();
    s.suspended().unwrap();
    let r = release(&mut s);
    s.release_failed(r).unwrap();
    s.confirm_release_quiescent(r).unwrap();
    s.retry().unwrap();
    let r2 = release(&mut s);
    s.released(r2).unwrap();
    assert_eq!(s.phase(), Phase::Absent);
}

#[test]
fn esgotamento_com_recurso_pronto_indica_limpeza() {
    let mut s = ready();
    s.last_id = u64::MAX;
    assert_eq!(s.suspended(), Err(Error::IdExhausted));
    assert!(s.is_blocked());
    assert!(s.needs_emergency_cleanup());
    assert!(!s.can_render());
    assert_eq!(s.pending_action(), None);
    assert_eq!(s.resumed(A), Err(Error::IdExhausted));
    assert!(s.needs_emergency_cleanup());
}

#[test]
fn esgotamento_apos_criacao_invalidada_preserva_recurso() {
    let mut s = SurfaceLifecycle::new();
    s.last_id = u64::MAX - 1;
    s.resumed(A).unwrap();
    let c = create(&mut s);
    s.suspended().unwrap();
    assert_eq!(s.created(c), Err(Error::IdExhausted));
    assert!(s.needs_emergency_cleanup());
    assert!(matches!(s.phase(), Phase::Ready { .. }));
    assert!(!s.can_render());
}

#[test]
fn esgotamento_na_retentativa_preserva_falha_e_limpeza() {
    let mut s = ready();
    s.suspended().unwrap();
    let r = release(&mut s);
    s.release_failed(r).unwrap();
    s.confirm_release_quiescent(r).unwrap();
    s.last_id = u64::MAX;
    assert_eq!(s.retry(), Err(Error::IdExhausted));
    assert!(s.needs_emergency_cleanup());
    assert_eq!(
        s.phase(),
        Phase::ReleaseFailed {
            id: r,
            quiescent: true
        }
    );
}

#[test]
fn esgotamento_sem_recurso_nao_inventa_limpeza() {
    let mut s = SurfaceLifecycle::new();
    s.last_id = u64::MAX;
    assert_eq!(s.resumed(A), Err(Error::IdExhausted));
    assert!(s.is_blocked());
    assert!(!s.needs_emergency_cleanup());
    assert_eq!(s.phase(), Phase::Absent);
}

#[test]
fn esgotamento_apos_liberacao_real_nao_inventa_recurso() {
    let mut s = ready();
    s.suspended().unwrap();
    let r = release(&mut s);
    s.resumed(A).unwrap();
    s.last_id = u64::MAX;
    assert_eq!(s.released(r), Err(Error::IdExhausted));
    assert_eq!(s.phase(), Phase::Absent);
    assert!(!s.needs_emergency_cleanup());
    assert!(!s.can_render());
}

#[test]
fn falhas_consecutivas_exigem_quiescencia_de_cada_id() {
    let mut s = ready();
    s.suspended().unwrap();
    let r = release(&mut s);
    s.release_failed(r).unwrap();
    s.confirm_release_quiescent(r).unwrap();
    s.retry().unwrap();
    let r2 = release(&mut s);
    s.release_failed(r2).unwrap();
    assert_eq!(
        s.confirm_release_quiescent(r),
        Err(Error::UnexpectedCompletion)
    );
    assert_eq!(s.retry(), Err(Error::NotQuiescent));
    s.confirm_release_quiescent(r2).unwrap();
    s.retry().unwrap();
    let r3 = release(&mut s);
    s.released(r3).unwrap();
    assert_eq!(s.phase(), Phase::Absent);
}

#[test]
fn sequencias_exploradas_preservam_invariantes() {
    fn explore(s: SurfaceLifecycle, depth: usize) {
        if s.can_render() {
            assert!(!s.blocked && s.resumed && s.dimensions.usable());
            assert_eq!(
                s.phase,
                Phase::Ready {
                    dimensions: s.dimensions
                }
            );
            assert!(s.pending_action().is_none());
        }
        match s.phase {
            Phase::Creating {
                id,
                dimensions,
                in_flight: false,
                invalidated: false,
            } if !s.blocked => {
                assert!(s.resumed && s.dimensions.usable());
                assert_eq!(dimensions, s.dimensions);
                assert_eq!(s.pending_action(), Some(Action::Create { id, dimensions }));
            }
            Phase::Releasing {
                id,
                in_flight: false,
            } if !s.blocked => assert_eq!(s.pending_action(), Some(Action::Release { id })),
            _ => assert!(s.pending_action().is_none()),
        }
        if depth == 0 {
            return;
        }
        for step in 0..14 {
            let mut n = SurfaceLifecycle {
                resumed: s.resumed,
                dimensions: s.dimensions,
                phase: s.phase,
                last_id: s.last_id,
                blocked: s.blocked,
            };
            let current = match n.phase {
                Phase::Creating { id, .. }
                | Phase::Releasing { id, .. }
                | Phase::ReleaseFailed { id, .. } => id,
                _ => 0,
            };
            let _ = match step {
                0 => n.resumed(A),
                1 => n.resumed(B),
                2 => n.resumed(ZERO),
                3 => n.suspended(),
                4 => n.resized(A),
                5 => n.resized(ZERO),
                6 => n.created(current),
                7 => n.creation_failed(current),
                8 => n.released(current),
                9 => n.release_failed(current),
                10 => n.retry(),
                11 => n.created(0),
                12 => match n.pending_action() {
                    Some(a) => n.start(a),
                    None => Err(Error::UnexpectedStart),
                },
                _ => n.confirm_release_quiescent(current),
            };
            explore(n, depth - 1);
        }
    }
    explore(SurfaceLifecycle::new(), 6);
}
