//! Return type for [`crate::ExecutableItem::execute`].

use crate::error::ItemError;

/// What the executor does after an item runs successfully: keep
/// dispatching, or abort the enclosing chain/graph.
///
/// This is the *success* payload only. The fallible outcome of an
/// `execute` call — either this flow directive or an [`ItemError`] — is
/// wrapped by [`ExecuteResult`].
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
#[non_exhaustive]
pub enum ItemFlow {
    /// Continue scheduling normally.
    Continue,
    /// Abort the enclosing chain or graph (no further items dispatched).
    StopChain,
}

/// The fallible result of running an item: `Ok` carries an [`ItemFlow`]
/// telling the executor what to do next, `Err` carries an [`ItemError`].
pub type ExecuteResult = Result<ItemFlow, ItemError>;
