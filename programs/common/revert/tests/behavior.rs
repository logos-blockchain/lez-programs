use std::{cell::Cell, num::NonZeroU8};

use program_revert::{require, require_eq, require_ne, UnwrapOrRevert as _};

const CODE: NonZeroU8 = NonZeroU8::MIN;

#[test]
fn successful_checks_borrow_operands_once_and_do_not_format_errors() {
    let calls = Cell::new(0);
    let value = String::from("value");
    require_eq!(
        CODE,
        {
            calls.set(calls.get() + 1);
            &value
        },
        &value
    );
    require_ne!(
        CODE,
        {
            calls.set(calls.get() + 1);
            &value
        },
        "other"
    );
    require!(CODE, !value.is_empty(), "{}", {
        calls.set(99);
        "error"
    });
    assert_eq!(calls.get(), 2);
    assert_eq!(value, "value");
}

#[test]
fn successful_unwraps_return_owned_values() {
    assert_eq!(
        Some(String::from("option")).unwrap_or_revert(CODE, "failed"),
        "option"
    );
    assert_eq!(
        Ok::<_, ()>(String::from("result")).unwrap_or_revert(CODE, "failed"),
        "result"
    );
}

#[test]
#[should_panic(expected = "Program reverted [1]: missing balance")]
fn native_option_failure_keeps_code_and_message() {
    None::<u128>.unwrap_or_revert(CODE, "missing balance");
}

#[test]
#[should_panic(expected = "invalid account: \"decode failed\"")]
fn native_result_failure_keeps_cause() {
    Err::<(), _>("decode failed").unwrap_or_revert(CODE, "invalid account");
}

#[test]
#[should_panic(expected = "must match: left=1, right=2")]
fn equality_failure_keeps_values() {
    require_eq!(CODE, 1, 2, "must match");
}

#[test]
#[should_panic(expected = "must differ: left=1, right=1")]
fn inequality_failure_keeps_values() {
    require_ne!(CODE, 1, 1, "must differ");
}
