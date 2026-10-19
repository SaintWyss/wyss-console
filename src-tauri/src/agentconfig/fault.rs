//! Failure injection for the writer and the stores (F2a-12). In production the injector is
//! empty and a check costs one lock; a test arms a step and says how it fails.
//!
//! Two failures differ on purpose. `Fail` is the disk refusing: the code under test must
//! clean up after itself. `Crash` is the process dying: the code must stop at once and
//! leave exactly what a death leaves, because the next start has to repair it.
use std::sync::{Arc, Mutex};

/// Every step of an apply at which something can go wrong (14 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Step {
    RecordSealed,
    RecordWritten,
    IntentJournaled,
    TemporaryCreated,
    TemporaryWritten,
    TemporaryFlushed,
    Renamed,
    /// Right after the swap: the displaced file is at the temporary name and not yet judged.
    Exchanged,
    /// Before the swap is undone because the displaced file was not the one that was read.
    SwappedBack,
    DirectoryFlushed,
    ReadBack,
    OutcomeJournaled,
    RecordMarked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    Fail,
    Crash,
}

/// What a step reports when it is armed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Injected {
    Failed,
    Crashed,
}

#[derive(Clone, Default)]
pub struct Faults {
    armed: Arc<Mutex<Vec<(Step, Fault)>>>,
}

impl Faults {
    pub fn arm(&self, step: Step, fault: Fault) {
        self.armed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((step, fault));
    }
    /// `Err` when `step` is armed. A step armed once fires once.
    pub fn check(&self, step: Step) -> Result<(), Injected> {
        let mut armed = self
            .armed
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match armed.iter().position(|(armed_step, _)| *armed_step == step) {
            None => Ok(()),
            Some(index) => Err(match armed.remove(index).1 {
                Fault::Fail => Injected::Failed,
                Fault::Crash => Injected::Crashed,
            }),
        }
    }
}
