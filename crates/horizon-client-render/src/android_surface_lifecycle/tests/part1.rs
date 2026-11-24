use super::*;
const A: Dimensions = Dimensions::new(800, 600);
const B: Dimensions = Dimensions::new(1024, 768);
const ZERO: Dimensions = Dimensions::new(0, 600);

fn begin(s: &mut SurfaceLifecycle) -> Action {
    let action = s.pending_action().expect("ação pendente");
    s.start(action).unwrap();
    assert_eq!(s.pending_action(), None);
    action
}
fn create(s: &mut SurfaceLifecycle) -> u64 {
    match begin(s) {
        Action::Create { id, .. } => id,
        _ => panic!("esperada criação"),
    }
}
fn release(s: &mut SurfaceLifecycle) -> u64 {
    match begin(s) {
        Action::Release { id } => id,
        _ => panic!("esperada liberação"),
    }
}
fn ready() -> SurfaceLifecycle {
    let mut s = SurfaceLifecycle::new();
    s.resumed(A).unwrap();
    let c = create(&mut s);
    s.created(c).unwrap();
    assert!(s.can_render());
    s
}

#[test]
fn inicial_e_dimensoes_zero() {
    let mut s = SurfaceLifecycle::new();
    assert_eq!(s.phase(), Phase::Absent);
    assert!(!s.can_render());
    s.resumed(ZERO).unwrap();
    assert_eq!(s.pending_action(), None);
    s.resized(Dimensions::new(800, 0)).unwrap();
    assert_eq!(s.phase(), Phase::Absent);
    s.resized(A).unwrap();
    assert!(matches!(
        s.pending_action(),
        Some(Action::Create { dimensions: A, .. })
    ));
    assert!(!s.can_render());
}

#[test]
fn suspensao_cancela_criacao_nao_iniciada_e_rejeita_acao_antiga() {
    let mut s = SurfaceLifecycle::new();
    s.resumed(A).unwrap();
    let old = s.pending_action().unwrap();
    let id = match old {
        Action::Create { id, .. } => id,
        _ => unreachable!(),
    };
    s.suspended().unwrap();
    assert_eq!(s.phase(), Phase::Absent);
    assert_eq!(s.pending_action(), None);
    assert_eq!(s.start(old), Err(Error::UnexpectedStart));
    assert_eq!(s.created(id), Err(Error::UnexpectedCompletion));
    assert_eq!(s.creation_failed(id), Err(Error::UnexpectedCompletion));
    s.resumed(A).unwrap();
    assert_ne!(s.pending_action(), Some(old));
    assert_eq!(s.start(old), Err(Error::UnexpectedStart));
}

#[test]
fn resize_cancela_criacao_nao_iniciada_inclusive_zero_e_retorno() {
    let mut s = SurfaceLifecycle::new();
    s.resumed(A).unwrap();
    let old = s.pending_action().unwrap();
    s.resized(ZERO).unwrap();
    assert_eq!(s.pending_action(), None);
    assert_eq!(s.start(old), Err(Error::UnexpectedStart));
    s.resized(B).unwrap();
    let b = s.pending_action().unwrap();
    assert!(matches!(b, Action::Create { dimensions: B, .. }));
    s.resized(A).unwrap();
    assert_ne!(s.pending_action(), Some(old));
    assert_eq!(s.start(b), Err(Error::UnexpectedStart));
}

#[test]
fn criacao_iniciada_invalidada_por_suspensao_ou_resize() {
    for suspended in [true, false] {
        let mut s = SurfaceLifecycle::new();
        s.resumed(A).unwrap();
        let c = create(&mut s);
        if suspended {
            s.suspended().unwrap();
            s.resumed(A).unwrap();
        } else {
            s.resized(B).unwrap();
            s.resized(A).unwrap();
        }
        assert_eq!(s.pending_action(), None);
        s.created(c).unwrap();
        assert!(!s.can_render());
        let r = release(&mut s);
        s.released(r).unwrap();
        let c2 = create(&mut s);
        assert_ne!(c, c2);
        s.created(c2).unwrap();
        assert!(s.can_render());
    }
}

#[test]
fn suspender_exige_liberacao_real_e_retoma() {
    let mut s = ready();
    s.suspended().unwrap();
    let action = s.pending_action().unwrap();
    s.suspended().unwrap();
    s.resumed(A).unwrap();
    assert_eq!(s.pending_action(), Some(action));
    let r = release(&mut s);
    assert!(!s.can_render());
    s.released(r).unwrap();
    let c = create(&mut s);
    assert_ne!(r, c);
    s.created(c).unwrap();
    assert!(s.can_render());
}

#[test]
fn suspender_sem_retomar_deixa_ausente() {
    let mut s = ready();
    s.suspended().unwrap();
    let r = release(&mut s);
    s.released(r).unwrap();
    assert_eq!(s.phase(), Phase::Absent);
    assert_eq!(s.pending_action(), None);
    assert!(!s.can_render());
}

#[test]
fn resize_recria_e_zero_bloqueia() {
    let mut s = ready();
    s.resized(B).unwrap();
    let r = release(&mut s);
    s.resized(ZERO).unwrap();
    s.released(r).unwrap();
    assert_eq!(s.phase(), Phase::Absent);
    s.resized(B).unwrap();
    let c = create(&mut s);
    s.created(c).unwrap();
    assert!(s.can_render());
}

#[test]
fn inicio_exige_acao_exata_e_nao_aceita_duplicata() {
    let mut s = SurfaceLifecycle::new();
    s.resumed(A).unwrap();
    let action = s.pending_action().unwrap();
    let c = match action {
        Action::Create { id, .. } => id,
        _ => unreachable!(),
    };
    assert_eq!(
        s.start(Action::Create {
            id: c,
            dimensions: B
        }),
        Err(Error::UnexpectedStart)
    );
    assert_eq!(
        s.start(Action::Release { id: c }),
        Err(Error::UnexpectedStart)
    );
    assert_eq!(s.created(c), Err(Error::UnexpectedCompletion));
    assert_eq!(s.creation_failed(c), Err(Error::UnexpectedCompletion));
    s.resumed(A).unwrap();
    s.resized(A).unwrap();
    assert_eq!(s.pending_action(), Some(action));
    s.start(action).unwrap();
    assert_eq!(s.start(action), Err(Error::UnexpectedStart));
    s.created(c).unwrap();
    assert_eq!(s.created(c), Err(Error::UnexpectedCompletion));
    s.suspended().unwrap();
    let action = s.pending_action().unwrap();
    let r = match action {
        Action::Release { id } => id,
        _ => unreachable!(),
    };
    assert_eq!(s.released(r), Err(Error::UnexpectedCompletion));
    assert_eq!(s.release_failed(r), Err(Error::UnexpectedCompletion));
    s.start(action).unwrap();
    assert_eq!(s.start(action), Err(Error::UnexpectedStart));
    assert_eq!(s.created(r), Err(Error::UnexpectedCompletion));
    s.released(r).unwrap();
    assert_eq!(s.released(r), Err(Error::UnexpectedCompletion));
}

#[test]
fn falha_criacao_exige_retry_e_aguarda_condicoes_validas() {
    let mut s = SurfaceLifecycle::new();
    s.resumed(A).unwrap();
    let c = create(&mut s);
    s.suspended().unwrap();
    s.creation_failed(c).unwrap();
    s.resumed(ZERO).unwrap();
    assert_eq!(s.phase(), Phase::CreateFailed);
    s.retry().unwrap();
    assert_eq!(s.phase(), Phase::Absent);
    s.resized(B).unwrap();
    let c2 = create(&mut s);
    assert_ne!(c, c2);
    s.created(c2).unwrap();
    assert!(s.can_render());
}

