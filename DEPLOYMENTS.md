# Deployments

Live on-chain accounts per network.

## Testnet legacy (v0.2.4)

| Network | Value |
|---|---|
| Sequencer | `https://testnet-legacy.lez.logos.co/` |
| Explorer | `https://explorer.testnet-legacy.lez.logos.co/` |

Programs built from tag `v2.0.1` with `cargo risczero build` (release profile).

### Programs

| Program | ImageID (hex) | ImageID (base58) |
|---|---|---|
| token | `14b3bc6cc129d0359f9595ba0f60a13e4a8df08ffe8eb1a83251c47abecc67a8` | `2Pp7aXgGY9Lsox326tcY58KAD6VmzYB8wquZw83nhUyV` |
| amm | `cf93244a56a4bb974b72a5f5fa2fe68906709dd17ce422367baef195b57c1709` | `EyHXXf4r1CfqSfLUUyDeL7Tqod89pKRydMhovhAedGkY` |
| twap_oracle | `5681e27c27b3ece4a7c161bbf8e12e6ece6ca69925a87b77f9e193fa4a249b31` | `6pgw6c78ZkWjz3Hjmnro4yGLZTSFsw6kNpJEwKgz2SAC` |
| token_mint_authority | `1ea6d932d240e54e53682547e6d9f08a03a42bd9bbaa8e6b4a9974fc86d1e70e` | `34eny5SUVoZX5FkQ9YgGsthnccH6CtDDvFc2ymuo4gyB` |

### Faucet mint authority

| Item | Account |
|---|---|
| Mint-authority PDA (base58) | `GmAwyCjGaT7ocZ2ePDgGFmNw9Yv51ohG421dHMukZRQC` |
| Mint-authority PDA (hex) | `ea301b814c93699a5f788c3394082f3c28490ed95e4f3559aa3bbcf0b6ae50d1` |

Set as `mint_authority` on every faucet token definition (Token A and Token B below).

### Token accounts

| Label | Role | Account |
|---|---|---|
| token-a-def | Token A definition | `642odmr8h1SYNkkeFeESuJ3wRWNUmJYY1Yi6KSdFs51s` |
| token-b-def | Token B definition | `5AyxKGHttD31qazmaRDqBM1eF2Yd8NzobZKw3dvdkguj` |

### AMM instance

| Item | Value |
|---|---|
| config PDA | `3oDQXpmuyfELVo37HRdGY5QQCtpwrWxTPbQVkB9m3W8t` |
| owner (namespace) | `58z1Msc9GPvGpdeKnTiz9dnKAKhXSc6cnpBuSNJPQh4L` |
| nonce | `0` (default instance) |
| authority | `Ex8DT7BhKcNzLGm5a6VjNa7HtxCCcfYMdPFUzTEKHmkb` |
| swap_fee_bps | `30` (0.30%) |
| protocol_fee_bps | `0` |

### Pool — TKA / TKB

Derived from the amm ProgramId + config + `(token-a-def, token-b-def)`.

| PDA | Account |
|---|---|
| pool | `5HpRvWvLuGAT3jkYeDVv8fqpgThS7zgB8uHbgTX4Q92k` |
| vault_a | `7ASWbHzgK1m1ADvELrRQZz7egYzJY2t2ppRVAMmzkSWH` |
| vault_b | `FgvDQE2M7Jey9myidy19Yzfi62hk74ia1wZYuh3N9Krq` |
| pool_definition_lp | `Nk35PPioaesBWP74r6PZAz5C26SNcdEac6Xp5rTm6BR` |
| lp_lock_holding | `EtYUS1Ee2cMvfEYChf9fv45RWYrgLSmo8yRFu2ajooda` |
| current_tick_account | `8JXTD35tGndyHe4cgUvSQJ8rHvSk1eE2eJMfSwHeURnk` |
| protocol_fee_a | `GZsW3MUo2nPJK3VnWMpebaFkVNKMv2EMBVY6JWJk5azR` |
| protocol_fee_b | `GdDZpfiU571smwJ37qYk8swyMqz1VvgmQVBMCwHRNs4Y` |

### TWAP oracle — pool as price source (24h window)

Derived from the twap ProgramId + `pool` + `window_duration = 86400000`.

| PDA | Account |
|---|---|
| current_tick_account | `8JXTD35tGndyHe4cgUvSQJ8rHvSk1eE2eJMfSwHeURnk` |
| price_observations | `BQwDNhqKEusjix2tMaghJDzdRMeoNoCFb2vbQF2ndXDz` |
| oracle_price_account | `BhYDumn3h28h2LPJBcEVCJLemPCaDzVKWZjTNTP5RuFb` |
