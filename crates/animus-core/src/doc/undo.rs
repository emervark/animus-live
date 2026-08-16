//! Undo and redo over [`DocCommand`]s.
//!
//! The stack owns applied commands and hands the document back to earlier
//! states by reverting them in order. It is capped two ways — by entry count
//! and by roughly-held bytes — because a session that retriangulates a 10k
//! vertex puppet forty times should not quietly hold half a gigabyte.
//!
//! ## Why merging is bounded by a call, not a clock
//!
//! Dragging a joint emits a command per mouse-move. Those must collapse into
//! one undo step, and the usual trick is a time window — merge if the last
//! command arrived under 500 ms ago. `animus-core` has no clock, and the
//! window is the wrong idea regardless: a gesture ends when the mouse comes
//! up, not when a timer expires. Slow, deliberate dragging would break into
//! several undo steps under a time window; here it stays one.
//!
//! So the editor calls [`UndoStack::break_merge`] on pointer-up, and until
//! it does, like commands fold together.

use super::Project;
use super::command::{CommandError, DocCommand, PendingChanges};

/// Entry cap. Spec §10.5.
pub const MAX_ENTRIES: usize = 100;
/// Memory cap in bytes. Spec §10.5.
pub const MAX_BYTES: usize = 500 * 1024 * 1024;

pub struct UndoStack {
    done: Vec<Box<dyn DocCommand>>,
    undone: Vec<Box<dyn DocCommand>>,
    /// Set by [`break_merge`](UndoStack::break_merge); cleared on the next push.
    sealed: bool,
    bytes: usize,
    max_entries: usize,
    max_bytes: usize,
}

impl Default for UndoStack {
    fn default() -> Self {
        Self::new()
    }
}

impl UndoStack {
    pub fn new() -> Self {
        Self::with_caps(MAX_ENTRIES, MAX_BYTES)
    }

    pub fn with_caps(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            done: Vec::new(),
            undone: Vec::new(),
            sealed: false,
            bytes: 0,
            max_entries,
            max_bytes,
        }
    }

    /// End the current gesture: the next command starts a new undo entry
    /// even if it would otherwise merge.
    pub fn break_merge(&mut self) {
        self.sealed = true;
    }

    /// Record a command that has **already been applied**.
    ///
    /// Returns `true` if it merged into the previous entry rather than
    /// creating a new one.
    pub fn push_applied(&mut self, cmd: Box<dyn DocCommand>) -> bool {
        self.undone.clear();

        if !self.sealed
            && let Some(last) = self.done.last_mut()
        {
            let before = last.memory_bytes();
            if last.merge(cmd.as_ref()) {
                self.bytes = self.bytes + last.memory_bytes() - before;
                return true;
            }
        }
        self.sealed = false;
        self.bytes += cmd.memory_bytes();
        self.done.push(cmd);
        self.trim();
        false
    }

    /// Drop oldest entries until both caps hold. Never drops the newest
    /// entry: a single command larger than the byte cap is still undoable
    /// once, which is better than silently refusing to undo it at all.
    fn trim(&mut self) {
        while self.done.len() > self.max_entries
            || (self.bytes > self.max_bytes && self.done.len() > 1)
        {
            let dropped = self.done.remove(0);
            self.bytes = self.bytes.saturating_sub(dropped.memory_bytes());
        }
    }

    pub fn undo(&mut self, p: &mut Project) -> Option<Result<PendingChanges, CommandError>> {
        let mut cmd = self.done.pop()?;
        self.bytes = self.bytes.saturating_sub(cmd.memory_bytes());
        let out = cmd.revert(p);
        if out.is_ok() {
            self.undone.push(cmd);
            self.sealed = true;
        } else {
            // A failed revert leaves the document where it was; put the
            // command back so the stack still describes reality.
            self.bytes += cmd.memory_bytes();
            self.done.push(cmd);
        }
        Some(out)
    }

    pub fn redo(&mut self, p: &mut Project) -> Option<Result<PendingChanges, CommandError>> {
        let mut cmd = self.undone.pop()?;
        let out = cmd.apply(p);
        if out.is_ok() {
            self.bytes += cmd.memory_bytes();
            self.done.push(cmd);
            self.sealed = true;
        } else {
            self.undone.push(cmd);
        }
        Some(out)
    }

    pub fn can_undo(&self) -> bool {
        !self.done.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }

    pub fn len(&self) -> usize {
        self.done.len()
    }

    pub fn is_empty(&self) -> bool {
        self.done.is_empty()
    }

    pub fn memory_bytes(&self) -> usize {
        self.bytes
    }

    /// Newest first, for the history panel.
    pub fn labels(&self) -> impl Iterator<Item = &str> {
        self.done.iter().rev().map(|c| c.label())
    }
}

/// Apply `cmd` and record it, in one step. The path the editor uses.
pub fn apply_command(
    p: &mut Project,
    stack: &mut UndoStack,
    mut cmd: Box<dyn DocCommand>,
) -> Result<PendingChanges, CommandError> {
    let changes = cmd.apply(p)?;
    stack.push_applied(cmd);
    Ok(changes)
}
