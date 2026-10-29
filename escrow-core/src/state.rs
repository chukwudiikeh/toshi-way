//! Escrow states and the allowed transitions between them.

use std::{fmt, str::FromStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EscrowState {
    Created,
    InvoiceIssued,
    Locked,
    Settled,
    Refunded,
    ExpiredUnpaid,
    Disputed,
    Failed,
}

impl EscrowState {
    pub const ALL: [EscrowState; 8] = [
        Self::Created,
        Self::InvoiceIssued,
        Self::Locked,
        Self::Settled,
        Self::Refunded,
        Self::ExpiredUnpaid,
        Self::Disputed,
        Self::Failed,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "CREATED",
            Self::InvoiceIssued => "INVOICE_ISSUED",
            Self::Locked => "LOCKED",
            Self::Settled => "SETTLED",
            Self::Refunded => "REFUNDED",
            Self::ExpiredUnpaid => "EXPIRED_UNPAID",
            Self::Disputed => "DISPUTED",
            Self::Failed => "FAILED",
        }
    }

    /// No further transitions out of a terminal state. `Failed` is terminal for the
    /// state machine; an operator resolves it outside the normal flow.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Settled | Self::Refunded | Self::ExpiredUnpaid | Self::Failed)
    }

    pub fn can_transition_to(self, to: EscrowState) -> bool {
        use EscrowState::*;
        if to == Failed {
            return !self.is_terminal();
        }
        matches!(
            (self, to),
            (Created, InvoiceIssued)
                | (InvoiceIssued, Locked)
                | (InvoiceIssued, ExpiredUnpaid)
                | (Locked, Settled)
                | (Locked, Refunded)
                | (Locked, Disputed)
                | (Disputed, Refunded)
                | (Disputed, Settled)
        )
    }
}

impl fmt::Display for EscrowState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for EscrowState {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|st| st.as_str() == s)
            .ok_or_else(|| format!("unknown escrow state '{s}'"))
    }
}

#[cfg(test)]
mod tests {
    use super::EscrowState::{self, *};

    #[test]
    fn allowed_transitions() {
        for (from, to) in [
            (Created, InvoiceIssued),
            (InvoiceIssued, Locked),
            (InvoiceIssued, ExpiredUnpaid),
            (Locked, Settled),
            (Locked, Refunded),
            (Locked, Disputed),
            (Disputed, Refunded),
            (Disputed, Settled),
            (Created, Failed),
            (Locked, Failed),
            (Disputed, Failed),
        ] {
            assert!(from.can_transition_to(to), "{from} -> {to} should be allowed");
        }
    }

    #[test]
    fn terminal_states_have_no_exits() {
        for from in [Settled, Refunded, ExpiredUnpaid, Failed] {
            for to in EscrowState::ALL {
                assert!(!from.can_transition_to(to), "{from} -> {to} should be rejected");
            }
        }
    }

    #[test]
    fn skipping_states_is_rejected() {
        assert!(!Created.can_transition_to(Locked));
        assert!(!Created.can_transition_to(Settled));
        assert!(!InvoiceIssued.can_transition_to(Settled));
        assert!(!InvoiceIssued.can_transition_to(Refunded));
        assert!(!Locked.can_transition_to(ExpiredUnpaid));
        assert!(!Locked.can_transition_to(Created));
    }

    #[test]
    fn string_round_trip() {
        for st in EscrowState::ALL {
            assert_eq!(st.as_str().parse::<EscrowState>().unwrap(), st);
        }
        assert!("NOPE".parse::<EscrowState>().is_err());
    }
}
