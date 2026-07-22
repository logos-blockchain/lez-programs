use amm_client::{
    maximum_guard_amount, minimum_guard_amount, ClientError, SlippageTolerance,
    SLIPPAGE_BPS_DENOMINATOR,
};

const ABOVE_TWO_POW_53: u128 = 9_007_199_254_740_993;

#[test]
fn tolerance_accepts_closed_basis_point_range() {
    assert_eq!(SlippageTolerance::new(0).expect("zero is valid").bps(), 0);
    assert_eq!(
        SlippageTolerance::new(SLIPPAGE_BPS_DENOMINATOR)
            .expect("one hundred percent is valid")
            .bps(),
        SLIPPAGE_BPS_DENOMINATOR
    );

    let error = SlippageTolerance::new(SLIPPAGE_BPS_DENOMINATOR + 1)
        .expect_err("more than one hundred percent must be rejected");
    assert_eq!(error.code(), "slippage_tolerance_out_of_range");
    assert!(matches!(
        error,
        ClientError::SlippageToleranceOutOfRange {
            bps,
            maximum_bps,
        } if bps == 10_001 && maximum_bps == 10_000
    ));
}

#[test]
fn minimum_guards_round_down_and_stay_executable() {
    let one_percent = SlippageTolerance::new(100).expect("valid tolerance");
    assert_eq!(minimum_guard_amount(100, one_percent), Ok(99));
    assert_eq!(minimum_guard_amount(101, one_percent), Ok(99));
    assert_eq!(minimum_guard_amount(1, one_percent), Ok(1));
    assert_eq!(
        minimum_guard_amount(1, SlippageTolerance::new(10_000).expect("valid tolerance")),
        Ok(1)
    );
    assert_eq!(minimum_guard_amount(0, one_percent), Ok(0));
}

#[test]
fn maximum_guards_round_up() {
    let one_percent = SlippageTolerance::new(100).expect("valid tolerance");
    assert_eq!(maximum_guard_amount(100, one_percent), Ok(101));
    assert_eq!(maximum_guard_amount(101, one_percent), Ok(103));
    assert_eq!(maximum_guard_amount(0, one_percent), Ok(0));
}

#[test]
fn maximum_guard_reports_u128_overflow() {
    let error = maximum_guard_amount(
        u128::MAX,
        SlippageTolerance::new(1).expect("valid tolerance"),
    )
    .expect_err("expanded maximum must not saturate");

    assert_eq!(error.code(), "slippage_bound_overflow");
    assert!(matches!(
        error,
        ClientError::SlippageBoundOverflow {
            quoted_amount: u128::MAX,
            slippage_bps: 1,
        }
    ));
}

#[test]
fn guards_preserve_amounts_above_javascript_integer_range() {
    let tolerance = SlippageTolerance::new(1).expect("valid tolerance");

    assert_eq!(
        minimum_guard_amount(ABOVE_TWO_POW_53, tolerance),
        amm_core::checked_mul_div_floor(ABOVE_TWO_POW_53, 9_999, 10_000).ok_or(
            ClientError::SlippageBoundOverflow {
                quoted_amount: ABOVE_TWO_POW_53,
                slippage_bps: 1,
            }
        )
    );
    assert_eq!(
        maximum_guard_amount(ABOVE_TWO_POW_53, tolerance),
        amm_core::checked_mul_div_ceil(ABOVE_TWO_POW_53, 10_001, 10_000).ok_or(
            ClientError::SlippageBoundOverflow {
                quoted_amount: ABOVE_TWO_POW_53,
                slippage_bps: 1,
            }
        )
    );
}
