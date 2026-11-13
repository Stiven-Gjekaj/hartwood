//! Exceptions and interrupts.

/// A synchronous exception. Each variant that has a value carries the value
/// for `mtval` or `stval`: the address that failed, or the bits of the
/// instruction that is illegal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exception {
    InstructionAddressMisaligned(u64),
    InstructionAccessFault(u64),
    IllegalInstruction(u64),
    Breakpoint(u64),
    LoadAddressMisaligned(u64),
    LoadAccessFault(u64),
    StoreAddressMisaligned(u64),
    StoreAccessFault(u64),
    EnvironmentCallFromU,
    EnvironmentCallFromS,
    EnvironmentCallFromM,
    InstructionPageFault(u64),
    LoadPageFault(u64),
    StorePageFault(u64),
}

impl Exception {
    /// The exception code for `mcause` and `scause`.
    pub fn code(self) -> u64 {
        match self {
            Exception::InstructionAddressMisaligned(_) => 0,
            Exception::InstructionAccessFault(_) => 1,
            Exception::IllegalInstruction(_) => 2,
            Exception::Breakpoint(_) => 3,
            Exception::LoadAddressMisaligned(_) => 4,
            Exception::LoadAccessFault(_) => 5,
            Exception::StoreAddressMisaligned(_) => 6,
            Exception::StoreAccessFault(_) => 7,
            Exception::EnvironmentCallFromU => 8,
            Exception::EnvironmentCallFromS => 9,
            Exception::EnvironmentCallFromM => 11,
            Exception::InstructionPageFault(_) => 12,
            Exception::LoadPageFault(_) => 13,
            Exception::StorePageFault(_) => 15,
        }
    }

    /// The value for `mtval` or `stval`.
    pub fn tval(self) -> u64 {
        match self {
            Exception::InstructionAddressMisaligned(v)
            | Exception::InstructionAccessFault(v)
            | Exception::IllegalInstruction(v)
            | Exception::Breakpoint(v)
            | Exception::LoadAddressMisaligned(v)
            | Exception::LoadAccessFault(v)
            | Exception::StoreAddressMisaligned(v)
            | Exception::StoreAccessFault(v)
            | Exception::InstructionPageFault(v)
            | Exception::LoadPageFault(v)
            | Exception::StorePageFault(v) => v,
            Exception::EnvironmentCallFromU
            | Exception::EnvironmentCallFromS
            | Exception::EnvironmentCallFromM => 0,
        }
    }
}

/// An interrupt. The value is the bit in `mip` and `mie`, and the code in
/// `mcause`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interrupt {
    SupervisorSoftware = 1,
    MachineSoftware = 3,
    SupervisorTimer = 5,
    MachineTimer = 7,
    SupervisorExternal = 9,
    MachineExternal = 11,
}

impl Interrupt {
    /// The order in which the privileged specification takes pending
    /// interrupts: external, then software, then timer, and machine before
    /// supervisor.
    pub const PRIORITY: [Interrupt; 6] = [
        Interrupt::MachineExternal,
        Interrupt::MachineSoftware,
        Interrupt::MachineTimer,
        Interrupt::SupervisorExternal,
        Interrupt::SupervisorSoftware,
        Interrupt::SupervisorTimer,
    ];

    /// The bit of this interrupt in `mip` and `mie`.
    pub fn bit(self) -> u64 {
        1 << (self as u64)
    }
}

/// A trap: an exception or an interrupt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trap {
    Exception(Exception),
    Interrupt(Interrupt),
}

impl From<Exception> for Trap {
    fn from(e: Exception) -> Self {
        Trap::Exception(e)
    }
}
