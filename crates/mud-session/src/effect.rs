//! Effects the FSM emits for the driver to perform, and the results fed back.

/// I/O the driver must perform on the FSM's behalf.
#[derive(Debug)]
#[non_exhaustive]
pub enum Effect {}

/// The outcome of an [`Effect`], fed back via `SessionFsm::on_effect`.
#[derive(Debug)]
#[non_exhaustive]
pub enum EffectResult {}
