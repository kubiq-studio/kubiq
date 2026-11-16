//! The host's half of the shared cell layer (`kubiq_cells`): what runs a cell.
//!
//! [`CellExecutor`] is the one shape a notebook's Jupyter kernel ([`crate::notebook::kernel`]) and
//! a runbook's captured process share: start a run of some code, interrupt it, go away. What a run
//! produces comes back on the executor's own channel — a kernel's iopub messages, a process's
//! pipes — because the two report in shapes nothing would gain from flattening; the document
//! worker that owns the executor turns either into its own outputs.

/// Something that runs a cell's code.
pub trait CellExecutor: Send {
    /// Start running `code` for `cell`. Returns the run's handle as the executor names it (a
    /// kernel's `msg_id`), which every report of that run carries.
    fn execute(&mut self, cell: &str, code: &str) -> Result<String, String>;
    /// Interrupt whatever runs now.
    fn interrupt(&mut self) -> Result<(), String>;
    /// Stop for good, waiting at most the executor's own grace period.
    fn shutdown(self: Box<Self>);
}

impl CellExecutor for crate::notebook::kernel::Kernel {
    fn execute(&mut self, _cell: &str, code: &str) -> Result<String, String> {
        crate::notebook::kernel::Kernel::execute(self, code)
    }

    fn interrupt(&mut self) -> Result<(), String> {
        crate::notebook::kernel::Kernel::interrupt(self)
    }

    fn shutdown(self: Box<Self>) {
        crate::notebook::kernel::Kernel::shutdown(*self)
    }
}
