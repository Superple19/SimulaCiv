use serde::{Deserialize, Serialize};

/// Stable permanent logical identifier for an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(transparent)]
pub struct AgentId(pub u32);

impl From<u32> for AgentId {
    #[inline]
    fn from(id: u32) -> Self {
        Self(id)
    }
}

impl From<AgentId> for u32 {
    #[inline]
    fn from(id: AgentId) -> Self {
        id.0
    }
}

impl AgentId {
    #[inline]
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    #[inline]
    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

impl std::fmt::Display for AgentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Volatile runtime storage index into active contiguous arrays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(transparent)]
pub struct DenseSlot(pub u32);

impl From<u32> for DenseSlot {
    #[inline]
    fn from(slot: u32) -> Self {
        Self(slot)
    }
}

impl From<DenseSlot> for u32 {
    #[inline]
    fn from(slot: DenseSlot) -> Self {
        slot.0
    }
}

impl DenseSlot {
    #[inline]
    pub const fn new(slot: u32) -> Self {
        Self(slot)
    }

    #[inline]
    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

impl std::fmt::Display for DenseSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Daily simulation time index (Day 0, 1, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(transparent)]
pub struct SimulationDay(pub u32);

impl From<u32> for SimulationDay {
    #[inline]
    fn from(day: u32) -> Self {
        Self(day)
    }
}

impl From<SimulationDay> for u32 {
    #[inline]
    fn from(day: SimulationDay) -> Self {
        day.0
    }
}

impl SimulationDay {
    #[inline]
    pub const fn new(day: u32) -> Self {
        Self(day)
    }

    #[inline]
    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

impl std::fmt::Display for SimulationDay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Deterministic integer index identifying an experiment replicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(transparent)]
pub struct ReplicateId(pub u32);

impl From<u32> for ReplicateId {
    #[inline]
    fn from(id: u32) -> Self {
        Self(id)
    }
}

impl From<ReplicateId> for u32 {
    #[inline]
    fn from(id: ReplicateId) -> Self {
        id.0
    }
}

impl ReplicateId {
    #[inline]
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    #[inline]
    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

impl std::fmt::Display for ReplicateId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Geographic / settlement bucket identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(transparent)]
pub struct GroupId(pub u16);

impl From<u16> for GroupId {
    #[inline]
    fn from(id: u16) -> Self {
        Self(id)
    }
}

impl From<GroupId> for u16 {
    #[inline]
    fn from(id: GroupId) -> Self {
        id.0
    }
}

impl GroupId {
    #[inline]
    pub const fn new(id: u16) -> Self {
        Self(id)
    }

    #[inline]
    pub const fn as_u16(self) -> u16 {
        self.0
    }
}

impl std::fmt::Display for GroupId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Fixed-point currency representation (1 Currency Unit = 1,000 Subunits).
pub type Money = i64;
