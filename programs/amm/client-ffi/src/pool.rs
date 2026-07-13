//! Decoding for the AMM's on-chain `PoolDefinition` account, so client apps
//! can read reserves / fees / vault ids without depending on `amm_core`
//! directly.

use amm_core::{AmmConfig, PoolDefinition};

pub struct PoolView {
    pub def_a: [u8; 32],
    pub def_b: [u8; 32],
    pub vault_a: [u8; 32],
    pub vault_b: [u8; 32],
    pub reserve_a: u128,
    pub reserve_b: u128,
    pub liquidity_supply: u128,
    /// Fee tier in basis points. Source is `u128` but supported tiers are all
    /// <= 100, so downcasting to `u32` is safe.
    pub fees: u32,
}

pub fn decode_pool(bytes: &[u8]) -> Result<PoolView, String> {
    let p: PoolDefinition = borsh::from_slice(bytes).map_err(|e| format!("{e:?}"))?;
    Ok(PoolView {
        def_a: p.definition_token_a_id.into_value(),
        def_b: p.definition_token_b_id.into_value(),
        vault_a: p.vault_a_id.into_value(),
        vault_b: p.vault_b_id.into_value(),
        reserve_a: p.reserve_a,
        reserve_b: p.reserve_b,
        liquidity_supply: p.liquidity_pool_supply,
        fees: u32::try_from(p.fees).unwrap_or(u32::MAX),
    })
}

pub struct ConfigView {
    pub token_program_id: [u32; 8],
    pub twap_oracle_program_id: [u32; 8],
    pub authority: [u8; 32],
}

pub fn decode_config(bytes: &[u8]) -> Result<ConfigView, String> {
    let c: AmmConfig = borsh::from_slice(bytes).map_err(|e| format!("{e:?}"))?;
    Ok(ConfigView {
        token_program_id: c.token_program_id,
        twap_oracle_program_id: c.twap_oracle_program_id,
        authority: c.authority.into_value(),
    })
}

#[cfg(test)]
mod tests {
    use amm_core::PoolDefinition;
    use nssa_core::account::AccountId;

    use super::*;

    #[test]
    fn decode_roundtrip() {
        let p = PoolDefinition::default();
        let bytes = borsh::to_vec(&p).unwrap();
        let v = decode_pool(&bytes).unwrap();
        assert_eq!(v.reserve_a, 0);
    }

    #[test]
    fn decode_config_roundtrip() {
        let c = AmmConfig {
            token_program_id: [7u32; 8],
            twap_oracle_program_id: [9u32; 8],
            authority: AccountId::new([0u8; 32]),
        };
        let bytes = borsh::to_vec(&c).unwrap();
        let v = decode_config(&bytes).unwrap();
        assert_eq!(v.token_program_id, [7u32; 8]);
        assert_eq!(v.twap_oracle_program_id, [9u32; 8]);
        assert_eq!(v.authority, [0u8; 32]);
    }
}
