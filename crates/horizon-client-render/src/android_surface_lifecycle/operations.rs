impl SurfaceLifecycle {
    /// Confirma criação terminada, real e configurada; a operação não pode
    /// continuar acessando o recurso. Uma criação invalidada é liberada, mesmo
    /// se um novo `resumed` ou tamanho anterior tiver chegado entretanto.
    pub fn created(&mut self, id: u64) -> Result<(), Error> {
        self.ensure_unblocked()?;
        let (dimensions, invalidated) = match self.phase {
            Phase::Creating {
                id: expected,
                dimensions,
                invalidated,
                in_flight: true,
            } if id == expected => (dimensions, invalidated),
            _ => return Err(Error::UnexpectedCompletion),
        };
        self.phase = Phase::Ready { dimensions };
        if invalidated {
            let release_id = self.next_id()?;
            self.phase = Phase::Releasing {
                id: release_id,
                in_flight: false,
            };
            Ok(())
        } else {
            self.reconcile()
        }
    }

    /// Só chamar após comprovar que a operação terminou, não pode mais acessar
    /// recursos e não deixou nenhum recurso parcial. Também pode registrar
    /// cancelamento comprovado de uma criação iniciada.
    pub fn creation_failed(&mut self, id: u64) -> Result<(), Error> {
        self.ensure_unblocked()?;
        match self.phase {
            Phase::Creating {
                id: expected,
                in_flight: true,
                ..
            } if id == expected => {
                self.phase = Phase::CreateFailed;
                Ok(())
            }
            _ => Err(Error::UnexpectedCompletion),
        }
    }

    /// Confirma a destruição real e que a operação terminou e não pode mais
    /// acessar o recurso, não apenas a intenção de destruir. Pode confirmar o
    /// resultado tardio de uma liberação anteriormente desconhecida.
    pub fn released(&mut self, id: u64) -> Result<(), Error> {
        self.ensure_unblocked()?;
        match self.phase {
            Phase::Releasing {
                id: expected,
                in_flight: true,
            }
            | Phase::ReleaseFailed { id: expected, .. }
                if id == expected =>
            {
                self.phase = Phase::Absent;
                self.reconcile()
            }
            _ => Err(Error::UnexpectedCompletion),
        }
    }

    /// Falha ou resultado desconhecido: assume possível recurso ainda existente
    /// e operação possivelmente em curso. Isso não confirma quiescência.
    pub fn release_failed(&mut self, id: u64) -> Result<(), Error> {
        self.ensure_unblocked()?;
        match self.phase {
            Phase::Releasing {
                id: expected,
                in_flight: true,
            } if id == expected => {
                self.phase = Phase::ReleaseFailed {
                    id,
                    quiescent: false,
                };
                Ok(())
            }
            _ => Err(Error::UnexpectedCompletion),
        }
    }

    /// Confirmar somente após provar que a operação antiga terminou e não
    /// pode mais tocar o recurso; resultado desconhecido não é quiescência.
    pub fn confirm_release_quiescent(&mut self, id: u64) -> Result<(), Error> {
        self.ensure_unblocked()?;
        match self.phase {
            Phase::ReleaseFailed {
                id: expected,
                quiescent: false,
            } if id == expected => {
                self.phase = Phase::ReleaseFailed {
                    id,
                    quiescent: true,
                };
                Ok(())
            }
            _ => Err(Error::UnexpectedCompletion),
        }
    }

    /// Retentativa explícita, nunca automática após erro. Para falha de criação
    /// pode não haver ação até que esteja ativo e as dimensões sejam válidas.
    /// Para falha de liberação, exige quiescência confirmada e que o backend
    /// permita nova tentativa segura mesmo com resultado anterior desconhecido.
    pub fn retry(&mut self) -> Result<(), Error> {
        self.ensure_unblocked()?;
        match self.phase {
            Phase::CreateFailed => {
                self.phase = Phase::Absent;
                self.reconcile()
            }
            Phase::ReleaseFailed {
                quiescent: false, ..
            } => Err(Error::NotQuiescent),
            Phase::ReleaseFailed {
                quiescent: true, ..
            } => {
                let id = self.next_id()?;
                self.phase = Phase::Releasing {
                    id,
                    in_flight: false,
                };
                Ok(())
            }
            _ => Err(Error::NotFailed),
        }
    }

    fn ensure_unblocked(&self) -> Result<(), Error> {
        if self.blocked {
            Err(Error::IdExhausted)
        } else {
            Ok(())
        }
    }

    fn next_id(&mut self) -> Result<u64, Error> {
        match self.last_id.checked_add(1) {
            Some(id) => {
                self.last_id = id;
                Ok(id)
            }
            None => {
                self.blocked = true;
                Err(Error::IdExhausted)
            }
        }
    }

    fn reconcile(&mut self) -> Result<(), Error> {
        self.ensure_unblocked()?;
        match self.phase {
            Phase::Absent if self.resumed && self.dimensions.usable() => {
                let id = self.next_id()?;
                self.phase = Phase::Creating {
                    id,
                    dimensions: self.dimensions,
                    invalidated: false,
                    in_flight: false,
                };
            }
            Phase::Ready { dimensions }
                if !self.resumed || !self.dimensions.usable() || dimensions != self.dimensions =>
            {
                let id = self.next_id()?;
                self.phase = Phase::Releasing {
                    id,
                    in_flight: false,
                };
            }
            _ => {}
        }
        Ok(())
    }
}

