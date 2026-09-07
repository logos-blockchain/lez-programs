//! Nonzero exit codes for the token program.
//!
//! Codes are local to this program. Keep existing values stable when adding errors.
//! Diagnostic logs identify the failed check within each category.

use std::num::NonZeroU8;

/// Invalid instruction, account, authorization, configuration, or operation state.
pub const INVALID_INPUT: NonZeroU8 = NonZeroU8::MIN;
/// Requested amount exceeds the available balance, debt, or collateral.
pub const INSUFFICIENT_BALANCE: NonZeroU8 = NonZeroU8::new(2).expect("nonzero error code");
/// Requested operation exceeds the representable arithmetic range.
pub const ARITHMETIC: NonZeroU8 = NonZeroU8::new(3).expect("nonzero error code");
