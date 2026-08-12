//! Supported fee tiers. The single source of truth is `amm_core::SUPPORTED_FEE_TIERS` — the guest
//! enforces the same set via `is_supported_fee_tier`, so exposing the list here keeps the UI from
//! drifting. Raw basis points only; the app formats labels and decides selectability.

use amm_core::SUPPORTED_FEE_TIERS;
use serde_json::{json, Value};

use super::FeeTiersRequest;

/// The AMM's supported fee tiers as raw basis points, ascending. Pure — no inputs. Wrapped in
/// `{ feeTiers: [...] }` so the op returns an object (the module unwraps it to a bare list).
pub(super) fn fee_tiers(_request: FeeTiersRequest) -> Result<Value, String> {
    let tiers: Vec<u64> = SUPPORTED_FEE_TIERS
        .iter()
        .map(|&bps| u64::try_from(bps).map_err(|_| format!("fee tier {bps} overflows u64")))
        .collect::<Result<_, _>>()?;
    Ok(json!({ "feeTiers": tiers }))
}

#[cfg(test)]
mod tests {
    use amm_core::is_supported_fee_tier;

    use super::*;

    #[test]
    fn fee_tiers_are_amm_core_supported_and_ascending() {
        let value = fee_tiers(FeeTiersRequest {}).unwrap();
        let tiers: Vec<u64> = value["feeTiers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tier| tier.as_u64().unwrap())
            .collect();
        assert_eq!(tiers, vec![1, 5, 30, 100]);
        for tier in &tiers {
            assert!(
                is_supported_fee_tier(u128::from(*tier)),
                "{tier} bps unsupported"
            );
        }
    }
}
