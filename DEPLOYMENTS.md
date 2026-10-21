# Deployments

Live on-chain accounts per network.

## Testnet (v0.2.4)

| Network | Value |
|---|---|
| Sequencer | `https://testnet.lez.logos.co/` |
| Explorer | `https://explorer.testnet.lez.logos.co/` |

### Programs

| Program | ImageID (hex) | ImageID (base58) |
|---|---|---|
| token | `14b3bc6cc129d0359f9595ba0f60a13e4a8df08ffe8eb1a83251c47abecc67a8` | `2Pp7aXgGY9Lsox326tcY58KAD6VmzYB8wquZw83nhUyV` |
| amm | `cf93244a56a4bb974b72a5f5fa2fe68906709dd17ce422367baef195b57c1709` | `EyHXXf4r1CfqSfLUUyDeL7Tqod89pKRydMhovhAedGkY` |
| twap_oracle | `5681e27c27b3ece4a7c161bbf8e12e6ece6ca69925a87b77f9e193fa4a249b31` | `6pgw6c78ZkWjz3Hjmnro4yGLZTSFsw6kNpJEwKgz2SAC` |
| token_mint_authority | `b0163189f35d29b3c4a4e6b64f3d797015e52fdbb5dae30433f810b8b007033a` | `CrNPCwYQX9CZV1CBkzfzu4Nhb3DssaDtV3Zp5CsZGrTT` |

### Faucet mint authority

| Item | Account |
|---|---|
| Mint-authority PDA (base58) | `GaEqKnuxYfe2ET7Lp6tgaF9jqxaRWoTZ2f978Z2ySKCP` |
| Mint-authority PDA (hex) | `e7631c9fbf2cbe344b483c13375546539c98cfb4914009708b7439ef027135a4` |

Set as `mint_authority` on every faucet token definition (Token A and Token B below).

### Token accounts 

| Label | Role | Account |
|---|---|---|
| token-a-def | Token A definition | `9zHnyUgGP7o2wJyt9fk56J55mzv5gsBeB9GrEJLqX4P9` |
| token-b-def | Token B definition | `6jZvWmfhVT5HZ54wYs1mnnU2raE8e5m9ryndraHyKu4P` |

### AMM instance

| Item | Value |
|---|---|
| config PDA | `FQeKyLsnVTD5XxZC96FLQS2dWoHQYewv372BdmocMHr6` |
| swap_fee_bps | `30` (0.30%) |
| protocol_fee_bps | `0` |

### Pool — TKA / TKB

Derived from the amm ProgramId + config + `(token-a-def, token-b-def)`.

| PDA | Account |
|---|---|
| pool | `8rJJ14smQ1edE11UQ95dSLkMVBSdcY5aUBA88dCRiEWZ` |
| vault_a | `4sUPDB9MWVadzFpdcUg6UEQyudQz6tExBwYLsngwZQQC` |
| vault_b | `25G2G9JtLSaDoRECt5yze2T9sNAitRPo336nW3NxDyut` |
| pool_definition_lp | `5V8zHawcU3yb96L8iVjUYGHqA7ShhG3gGjc3Xzek4qMz` |
| lp_lock_holding | `FLxi26KNg3SSmZqkzP2ycEr54UhJ4ZewafuVHjUyppT8` |
| current_tick_account | `DqFMvcUaEupqU9mt27ztdWsPC6sZTuruPoLhxfkMR71m` |
| protocol_fee_a | `CPSyoZbgSFrYvDQEhf4HkakpwtYiuL7geszd8AWmypbs` |
| protocol_fee_b | `HuXPJeXXn65XP5Se9ooHpUUE95hvpZueRDEsWN9qYQFi` |

### TWAP oracle — pool as price source (24h window)

Derived from the twap ProgramId + `pool` + `window_duration = 86400000`.

| PDA | Account |
|---|---|
| current_tick_account | `DqFMvcUaEupqU9mt27ztdWsPC6sZTuruPoLhxfkMR71m` |
| price_observations | `R5CTpqjyazqRTFYB432SdM7KoBFEgFzwmvzecvJ5DDW` |
| oracle_price_account | `6jppmhj32NF8HeuJc12duLxARRDq1rk32WJxUFbGGhVw` |
