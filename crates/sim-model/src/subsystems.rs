use serde::{Deserialize, Serialize};

/// M0 domain subsystems mapped to stable numeric IDs for PRNG logical addressing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u16)]
pub enum Subsystem {
    /// Day 0 agent trait and world stochastic generation (ID 0).
    Initialization = 0,
    /// Phase 4 action selection Softmax draw (ID 1).
    Decision = 1,
    /// Phase 4 StealFood victim candidate sampling draw (ID 2).
    TheftTarget = 2,
    /// Phase 4 GiveFood recipient candidate sampling draw (ID 3).
    MutualAidTarget = 3,
    /// Phase 6B StealFood success probability draw (ID 4).
    TheftSuccess = 4,
    /// Phase 6B ResolutionKey evaluation priority mixer (ID 5).
    ResolutionPriority = 5,
}

impl Subsystem {
    /// Returns the stable numeric identifier for this subsystem.
    #[inline]
    pub const fn id(self) -> u16 {
        self as u16
    }
}

impl From<Subsystem> for u16 {
    #[inline]
    fn from(subsystem: Subsystem) -> Self {
        subsystem as u16
    }
}

/// Error returned when attempting to parse an invalid numeric subsystem ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidSubsystemIdError(pub u16);

impl std::fmt::Display for InvalidSubsystemIdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid subsystem id: {}", self.0)
    }
}

impl std::error::Error for InvalidSubsystemIdError {}

impl TryFrom<u16> for Subsystem {
    type Error = InvalidSubsystemIdError;

    fn try_from(id: u16) -> Result<Self, Self::Error> {
        match id {
            0 => Ok(Self::Initialization),
            1 => Ok(Self::Decision),
            2 => Ok(Self::TheftTarget),
            3 => Ok(Self::MutualAidTarget),
            4 => Ok(Self::TheftSuccess),
            5 => Ok(Self::ResolutionPriority),
            other => Err(InvalidSubsystemIdError(other)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subsystem_numeric_mappings() {
        assert_eq!(Subsystem::Initialization.id(), 0);
        assert_eq!(Subsystem::Decision.id(), 1);
        assert_eq!(Subsystem::TheftTarget.id(), 2);
        assert_eq!(Subsystem::MutualAidTarget.id(), 3);
        assert_eq!(Subsystem::TheftSuccess.id(), 4);
        assert_eq!(Subsystem::ResolutionPriority.id(), 5);

        for id in 0..=5 {
            let sub = Subsystem::try_from(id).expect("should map");
            assert_eq!(sub.id(), id);
        }

        assert!(Subsystem::try_from(6).is_err());
        assert!(Subsystem::try_from(99).is_err());
    }
}
