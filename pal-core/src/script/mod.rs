//! Deterministic PAL script execution and typed runtime yields.

mod action;
mod condition;
mod event;
pub(crate) mod executor;
mod inspection;
mod opcode;
mod trigger;
mod visual;

pub use action::ScriptAction;
pub use condition::ScriptCondition;
pub use event::{DialogPosition, ScriptEvent};
pub use inspection::{
    inspect_script_object_targets, inspect_script_record, inspect_script_records,
    inspect_script_targets, ScriptControlFlow, ScriptInstructionReferenceCategory,
    ScriptInstructionReferenceKind, ScriptInstructionTarget, ScriptObjectOperand,
    ScriptObjectReference, ScriptObjectTarget, ScriptRecordInspection, ScriptReferenceCatalog,
    ScriptReferenceSource,
};
pub use opcode::ScriptOpcode;
pub(crate) use trigger::ScriptInstructionStep;
pub use trigger::{
    ScriptDebugSnapshot, ScriptInstructionDebug, ScriptRuntime, ScriptTraceEvent,
    ScriptTraceOutcome, ScriptTraceRecord,
};
pub use visual::ScriptVisual;

#[cfg(test)]
mod tests;
