//! Estado puro para o ciclo de vida de uma superfície Android.
//!
//! Este módulo NÃO está ligado ao renderer, ao winit ou a callbacks Android e
//! não cria, configura, destrói ou libera nenhum recurso. Consultar uma ação
//! pendente não a executa; somente a integração futura pode executá-la e depois
//! confirmar seu resultado. Nenhuma garantia de suporte Android decorre daqui.
//!
//! O callback Android `suspended` deve parar o uso e efetivamente destruir a
//! superfície de forma síncrona antes de retornar. Se houver criação em curso,
//! ela deve ser concluída ou cancelada de forma comprovada e qualquer superfície
//! resultante destruída antes do retorno. Registrar `suspended` ou uma ação de
//! liberação pendente NÃO satisfaz essa obrigação. Falha ou operação pendente ao
//! retornar deve ser tratada pela integração como falha de ciclo de vida, não
//! como liberação bem-sucedida. A integração é responsável por serializar o
//! acesso e por garantir que não há renderização em curso durante a destruição.
//!
//! O modelo presume uma única superfície e uma única operação em voo. A criação
//! só pode ser confirmada depois de a superfície existir e estar configurada
//! com as dimensões solicitadas; falha de criação só pode ser informada depois
//! de comprovar que não ficou nenhum recurso, inclusive parcial. Falha de
//! liberação significa que a destruição NÃO foi confirmada: o recurso deve ser
//! tratado como possivelmente existente, sem renderizar ou criar outro. Repetir
//! a liberação depende da segurança/idempotência da API real do renderer e da
//! confirmação de que a operação anterior terminou (quiescência).
//! IDs impedem que uma resposta atrasada confirme uma operação diferente no
//! mesmo objeto. Antes de iniciar a operação real, o executor deve chamar
//! `start` sob a mesma serialização usada pelos callbacks; não pode executar
//! uma ação obtida anteriormente sem que `start` tenha sido aceito. Uma ação
//! iniciada não aparece mais em `pending_action`. Não recriar este objeto com
//! operações antigas ainda em curso: IDs de objetos distintos podem coincidir.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dimensions {
    pub width: u32,
    pub height: u32,
}

impl Dimensions {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    fn usable(self) -> bool {
        self.width != 0 && self.height != 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Create { id: u64, dimensions: Dimensions },
    Release { id: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Absent,
    Creating {
        id: u64,
        dimensions: Dimensions,
        invalidated: bool,
        in_flight: bool,
    },
    Ready {
        dimensions: Dimensions,
    },
    Releasing {
        id: u64,
        in_flight: bool,
    },
    /// A criação falhou e o executor confirmou que não sobraram recursos.
    CreateFailed,
    /// Destruição não confirmada; a superfície pode ainda existir.
    ReleaseFailed {
        id: u64,
        quiescent: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Resposta duplicada, atrasada ou de tipo incorreto; o estado não muda.
    UnexpectedCompletion,
    /// A tentativa explícita só é válida em uma fase de falha.
    NotFailed,
    /// A liberação anterior ainda pode estar em execução.
    NotQuiescent,
    /// A ação não está pendente ou já foi iniciada.
    UnexpectedStart,
    /// IDs esgotados: estado permanentemente bloqueado, sem permissão de renderizar.
    IdExhausted,
}

/// Planejador de estado, sem acesso a janelas, dispositivos ou superfícies reais.
#[derive(Debug)]
pub struct SurfaceLifecycle {
    resumed: bool,
    dimensions: Dimensions,
    phase: Phase,
    last_id: u64,
    blocked: bool,
}

impl Default for SurfaceLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

impl SurfaceLifecycle {
    pub const fn new() -> Self {
        Self {
            resumed: false,
            dimensions: Dimensions::new(0, 0),
            phase: Phase::Absent,
            last_id: 0,
            blocked: false,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn is_resumed(&self) -> bool {
        self.resumed
    }
    pub fn dimensions(&self) -> Dimensions {
        self.dimensions
    }
    pub fn is_blocked(&self) -> bool {
        self.blocked
    }

    /// Esgotamento bloqueia operações normais, mas não prova ausência de recurso.
    /// Se verdadeiro, a integração deve parar a renderização e providenciar
    /// limpeza emergencial fora deste planejador, após assegurar quiescência.
    /// A indicação não confirma limpeza nem autoriza operações concorrentes.
    pub fn needs_emergency_cleanup(&self) -> bool {
        self.blocked
            && match self.phase {
                Phase::Ready { .. } | Phase::Releasing { .. } | Phase::ReleaseFailed { .. } => true,
                Phase::Creating { in_flight, .. } => in_flight,
                _ => false,
            }
    }

    /// Permissão lógica, não prova de que o renderer ou a janela ainda são válidos.
    /// A integração deve verificar este resultado imediatamente antes de usar a
    /// superfície e coordenar renderização com callbacks e operações reais.
    pub fn can_render(&self) -> bool {
        !self.blocked
            && self.resumed
            && self.dimensions.usable()
            && matches!(self.phase, Phase::Ready { dimensions } if dimensions == self.dimensions)
    }

    /// Retorna apenas uma ação ainda não iniciada; consulta não a executa.
    pub fn pending_action(&self) -> Option<Action> {
        if self.blocked {
            return None;
        }
        match self.phase {
            Phase::Creating {
                id,
                dimensions,
                in_flight: false,
                ..
            } => Some(Action::Create { id, dimensions }),
            Phase::Releasing {
                id,
                in_flight: false,
            } => Some(Action::Release { id }),
            _ => None,
        }
    }

    /// Reserva a ação para execução exatamente uma vez. O executor deve
    /// serializar esta chamada com callbacks e com o início da operação real.
    pub fn start(&mut self, action: Action) -> Result<(), Error> {
        self.ensure_unblocked()?;
        match (&mut self.phase, action) {
            (
                Phase::Creating {
                    id,
                    dimensions,
                    in_flight,
                    invalidated: false,
                },
                Action::Create {
                    id: supplied,
                    dimensions: d,
                },
            ) if *id == supplied && *dimensions == d && !*in_flight => {
                *in_flight = true;
                Ok(())
            }
            (Phase::Releasing { id, in_flight }, Action::Release { id: supplied })
                if *id == supplied && !*in_flight =>
            {
                *in_flight = true;
                Ok(())
            }
            _ => Err(Error::UnexpectedStart),
        }
    }

    /// Eventos repetidos com as mesmas dimensões são idempotentes.
    pub fn resumed(&mut self, dimensions: Dimensions) -> Result<(), Error> {
        self.ensure_unblocked()?;
        self.resumed = true;
        self.set_dimensions(dimensions);
        self.reconcile()
    }

    /// Não equivale à destruição. Veja a obrigação síncrona no início do módulo.
    pub fn suspended(&mut self) -> Result<(), Error> {
        self.ensure_unblocked()?;
        self.resumed = false;
        self.invalidate_creation();
        self.reconcile()
    }

    /// Dimensões zero impedem criação e renderização. Mudanças exigem recriação;
    /// nunca se presume que o renderer reconfigurou a superfície por conta própria.
    pub fn resized(&mut self, dimensions: Dimensions) -> Result<(), Error> {
        self.ensure_unblocked()?;
        self.set_dimensions(dimensions);
        self.reconcile()
    }

    fn set_dimensions(&mut self, dimensions: Dimensions) {
        if self.dimensions != dimensions {
            self.invalidate_creation();
            self.dimensions = dimensions;
        }
    }

    fn invalidate_creation(&mut self) {
        match self.phase {
            Phase::Creating {
                in_flight: false, ..
            } => self.phase = Phase::Absent,
            Phase::Creating {
                ref mut invalidated,
                in_flight: true,
                ..
            } => *invalidated = true,
            _ => {}
        }
    }

}

include!("android_surface_lifecycle/operations.rs");

#[cfg(test)]
mod tests;
