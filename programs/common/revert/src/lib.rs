//! Explicit failures for expected program errors.
//!
//! Guests halt with a nonzero code so the executor retains the session and its
//! metered cycles. Native callers retain panic diagnostics, including existing
//! `should_panic` tests. Ordinary panics are never intercepted: broken internal
//! invariants still fail through the zkVM panic path.
//!
//! Codes belong to each program's core crate; zero is excluded by the type.

use std::{fmt, num::NonZeroU8};

mod decode;

pub use decode::decode_instruction;

/// Abort an expected error without committing program output.
#[cold]
#[track_caller]
pub fn revert(code: NonZeroU8, message: fmt::Arguments<'_>) -> ! {
    #[cfg(target_os = "zkvm")]
    {
        risc0_zkvm::guest::env::log(&message.to_string());
        risc0_zkvm::guest::env::exit(code.get());
    }
    #[cfg(not(target_os = "zkvm"))]
    panic!("Program reverted [{}]: {message}", code.get());
}

/// Unwrap an explicitly classified, expected error.
pub trait UnwrapOrRevert<T> {
    /// Return the value or abort with the supplied program-local code.
    fn unwrap_or_revert(self, code: NonZeroU8, message: &str) -> T;
}

impl<T> UnwrapOrRevert<T> for Option<T> {
    #[track_caller]
    fn unwrap_or_revert(self, code: NonZeroU8, message: &str) -> T {
        match self {
            Some(value) => value,
            None => revert(code, format_args!("{message}")),
        }
    }
}

impl<T, E: fmt::Debug> UnwrapOrRevert<T> for Result<T, E> {
    #[track_caller]
    fn unwrap_or_revert(self, code: NonZeroU8, message: &str) -> T {
        match self {
            Ok(value) => value,
            Err(error) => revert(code, format_args!("{message}: {error:?}")),
        }
    }
}

/// Abort with a formatted diagnostic and a program-local code.
#[macro_export]
macro_rules! revert {
    ($code:expr, $($message:tt)+) => {
        $crate::revert($code, format_args!($($message)+))
    };
}

/// Require a condition on an expected rejection path.
#[macro_export]
macro_rules! require {
    ($code:expr, $condition:expr $(,)?) => {
        $crate::require!($code, $condition, "Requirement failed: {}", stringify!($condition))
    };
    ($code:expr, $condition:expr, $($message:tt)+) => {
        if !$condition {
            $crate::revert!($code, $($message)+);
        }
    };
}

/// Require equality, evaluating and borrowing each operand once.
#[macro_export]
macro_rules! require_eq {
    ($code:expr, $left:expr, $right:expr $(,)?) => {
        $crate::require_eq!($code, $left, $right, "Values must match")
    };
    ($code:expr, $left:expr, $right:expr, $($message:tt)+) => {
        match (&$left, &$right) {
            (left, right) => {
                if *left != *right {
                    $crate::revert!($code, "{}: left={left:?}, right={right:?}", format_args!($($message)+));
                }
            }
        }
    };
}

/// Require inequality, evaluating and borrowing each operand once.
#[macro_export]
macro_rules! require_ne {
    ($code:expr, $left:expr, $right:expr $(,)?) => {
        $crate::require_ne!($code, $left, $right, "Values must differ")
    };
    ($code:expr, $left:expr, $right:expr, $($message:tt)+) => {
        match (&$left, &$right) {
            (left, right) => {
                if *left == *right {
                    $crate::revert!($code, "{}: left={left:?}, right={right:?}", format_args!($($message)+));
                }
            }
        }
    };
}
