# Changelog

All notable changes to the LEZ programs in this repository are documented here.
This file is generated from Conventional Commit messages by [git-cliff](https://git-cliff.org).

## [2.0.1] - 2026-09-09

### Bug Fixes

- **modules:**
  - Bump module versions ([3d28909](https://github.com/logos-blockchain/lez-programs/commit/3d289096513304d2fbb1c50cd35fc7de8d99cf2d))

## [2.0.0] - 2026-09-09

### ⚠️ Breaking Changes

- **amm:** The AMM guest changes, so its ProgramId (ImageID) changes — re-derive every value that depends on it (deployed program IDs, PDA-derived addresses, client/config files) before submitting. Specifically: - `AmmConfig` gains a `protocol_fee_bps` field: its on-chain (Borsh) layout   changes and existing config accounts are incompatible. - `Instruction::Initialize` gains a required `protocol_fee_bps` argument. - `SwapExactInput` / `SwapExactOutput` require an additional account — the input   token's protocol-fee holding PDA, appended after the clock. Clients building   swap transactions must supply it (the FFI swap plans do this automatically). - New `Instruction::WithdrawProtocolFees` variant. ([eae125a](https://github.com/logos-blockchain/lez-programs/commit/eae125a439956c776eeb52af315efba897280182))
- **amm:** The AMM swap fee is now instance-wide, not per-pool.  * Account layouts change (Borsh): AmmConfig gains `swap_fee_bps`; PoolDefinition   drops `fees`. Existing on-chain config and pool accounts are incompatible and   must be recreated. * Instruction ABI: `Initialize` requires `swap_fee_bps`; `NewDefinition` no longer   accepts `fees`. Regenerate IDL-based clients (artifacts/amm-idl.json is updated). * The fee is any value in `[0, FEE_BPS_DENOMINATOR)` set at `Initialize` — it is no   longer restricted to the 1/5/30/100 bps tiers, and there is no per-pool fee. * amm_core API: `PoolDefinition.fees` removed, `AmmConfig.swap_fee_bps` added;   `initialize()` gains a `swap_fee_bps` parameter and `new_definition()` drops its   `fees` parameter; new `assert_valid_swap_fee_bps` (the tier helpers are now unused   on-chain). * AMM module / FFI: `createPool` takes no `feeBps` (errors `bad_fee_bps_amount` /   `invalid_fee_tier` removed); swap-quote and resolve-pool FFI requests require the   `config` account; `configAccount` returns `swapFeeBps`. `AMM_POOLS_CONFIG` and the   registry pool schema no longer include `feeBps`. ([f2fcbcf](https://github.com/logos-blockchain/lez-programs/commit/f2fcbcfc209ac17c35afadee71c09ad08ec96d5f))
- **amm:** Namespace all PDAs to host many AMM instances per deployment ([c74b13e](https://github.com/logos-blockchain/lez-programs/commit/c74b13edfe0efa412f1cb552c48d847a5910c774))
- **stablecoin:** Migrate Position to spec §4.4 shape ([2d33923](https://github.com/logos-blockchain/lez-programs/commit/2d3392393a4981c4c7bb00be77d94cc33da6e245))
- **amm:** The UpdateConfig instruction ABI changed — the token_program_id and twap_oracle_program_id fields are removed and new_authority is now required (was Option). Any client constructing UpdateConfig must be updated. The instruction enum change also alters the program ImageID: redeploy and update every ImageID-derived value (deployed program ids, client/config files, PDA-derived addresses, AMM/ATA program-id inputs) before submitting ([de9a3d5](https://github.com/logos-blockchain/lez-programs/commit/de9a3d532018733a7e8bceaf54d7a37a6f4141bd))
- **amm:** The AMM swap instruction interface changed and the guest ImageID/ProgramId changes as a result. ([cca063c](https://github.com/logos-blockchain/lez-programs/commit/cca063ce2d315229d7c802fd7a0fd6bad74557ae))

### Features

- **amm:**
  - Add configurable protocol fees on swaps **[breaking]** ([eae125a](https://github.com/logos-blockchain/lez-programs/commit/eae125a439956c776eeb52af315efba897280182))
  - Make the swap fee instance-wide in the AMM config, not per-pool **[breaking]** ([f2fcbcf](https://github.com/logos-blockchain/lez-programs/commit/f2fcbcfc209ac17c35afadee71c09ad08ec96d5f))
  - Point the module, UI app, and testnet setup at namespaced instances ([75aa9bc](https://github.com/logos-blockchain/lez-programs/commit/75aa9bca9a52f028862c25a9067ef18f772866d9))
  - Namespace all PDAs to host many AMM instances per deployment **[breaking]** ([c74b13e](https://github.com/logos-blockchain/lez-programs/commit/c74b13edfe0efa412f1cb552c48d847a5910c774))
  - Add display_name/author to amm module + app metadata ([fb76bec](https://github.com/logos-blockchain/lez-programs/commit/fb76bec03c661e3de2a0bab20fe1a719686a7199))
  - Let LPs choose the LP-token destination account ([62d133e](https://github.com/logos-blockchain/lez-programs/commit/62d133e909e8da8e668be6c4e551a11c5ffde44b))
  - Add oracle setup ops (createPriceObservations / createOraclePriceAccount) ([7e45e44](https://github.com/logos-blockchain/lez-programs/commit/7e45e44eac74a1e5358f738a0b2943446d6fe10b))
  - Add transferOwnership (UpdateConfig admin transfer) ([ca8adfc](https://github.com/logos-blockchain/lez-programs/commit/ca8adfc4af82950f181342d1d2847a5c6200e9e1))
  - Add configAccount read (decode the singleton config) ([56c80ce](https://github.com/logos-blockchain/lez-programs/commit/56c80ce29dcc63a0b06e3043fddecdd819515224))
  - Source liquidity tokens app-side + add custom tokens by id ([7a7ebfd](https://github.com/logos-blockchain/lez-programs/commit/7a7ebfdbafd0f596d9b6009ef29dc02ff5a93655))
  - Expose supported fee tiers via feeTiers() op ([cb1b457](https://github.com/logos-blockchain/lez-programs/commit/cb1b457ad39c22aa5627a90e173878a89ab318f2))
  - Drive the create-pool liquidity preview from liquidityQuote ([eb98aac](https://github.com/logos-blockchain/lez-programs/commit/eb98aac31f055500611c741e3865b1e27ae78de2))
  - Move all AMM logic into the amm_module core module; flip UI to consume it ([72b3301](https://github.com/logos-blockchain/lez-programs/commit/72b330122dfab9583d8b2bb4ac2e608148ea9d7f))
  - Swap via d70225ced program_id_hex API; pin wallet-module core to sequencer rev 415964d7 ([9b7a3dc](https://github.com/logos-blockchain/lez-programs/commit/9b7a3dcaaced295175c67d306ea0f0fabe915a6b))
  - Byte-encode swap instruction for QtRO + AMM_DEBUG tracing + token config ([c0568e3](https://github.com/logos-blockchain/lez-programs/commit/c0568e3a88da7030763db9858e80cac4bd4f65e3))
  - Config-driven token picker wired to on-chain swaps ([906aa65](https://github.com/logos-blockchain/lez-programs/commit/906aa65b4f4ce2e43ae6efdb9630ec98fa45acd2))
  - Wire Swap UI to on-chain resolvePool + swapExactInput ([a0c8983](https://github.com/logos-blockchain/lez-programs/commit/a0c8983302aa339be137989a67972f08f954c602))
  - ResolvePool + swapExactInput backend slots for on-chain swaps ([b51a71d](https://github.com/logos-blockchain/lez-programs/commit/b51a71ddf29de27c80d5dfbd4f087c96301af34f))
  - Decode_config in amm_client_ffi (reads token/twap program ids from AMM config) ([f3a14f0](https://github.com/logos-blockchain/lez-programs/commit/f3a14f051ab22402483a12e036050ddbdeda2ac2))
  - Metal-safe amm_client_ffi crate + root flake for on-chain swap calls ([3b9ca24](https://github.com/logos-blockchain/lez-programs/commit/3b9ca241c2301ccffd2d0b06116b7ef0c44d7d34))
- **apps/amm:**
  - Add an app settings modal for the registry ([6956b23](https://github.com/logos-blockchain/lez-programs/commit/6956b23f1bf64339a36e23472505de5479db484b))
  - Add a Registry settings field to the wallet menu ([90bd86c](https://github.com/logos-blockchain/lez-programs/commit/90bd86c701cd03236834a8701fc8a3c15951b31f))
  - Persist a configurable registry URL setting ([c5030f3](https://github.com/logos-blockchain/lez-programs/commit/c5030f3aaafc9dbf1a245f4ae27e3d3e40200eb5))
  - Select the registry network without a program bin ([8a3a372](https://github.com/logos-blockchain/lez-programs/commit/8a3a372a33e339edd034168f687ae7cb09a8bccb))
  - Multi-network single-file registry ([f306d84](https://github.com/logos-blockchain/lez-programs/commit/f306d843ea2a84febad8e6e9df94faccaef8f535))
  - Load known tokens/pools from a remote registry ([5e18135](https://github.com/logos-blockchain/lez-programs/commit/5e181353b401edb6d81c77acda61fd8c94384af6))
  - Let users pick which LP account to remove from ([3994ba3](https://github.com/logos-blockchain/lez-programs/commit/3994ba3055289e689ccf05baabd22b0676ae190d))
  - Remove liquidity from the pool detail view ([78edd23](https://github.com/logos-blockchain/lez-programs/commit/78edd23b5b663939162e108a587e106dd54bb789))
  - Introduce positions view ([99ea680](https://github.com/logos-blockchain/lez-programs/commit/99ea6805dfb6868fffd51733c47966225ba6d22b))
  - Add a pool detail view reached from the Pools list ([06d7119](https://github.com/logos-blockchain/lez-programs/commit/06d7119a97dcf2ca0201bbd4e912a87e819cf2a4))
  - Drive the Pools list from AMM_POOLS_CONFIG ([4cfc03a](https://github.com/logos-blockchain/lez-programs/commit/4cfc03a81574ea5d84cce1d7c40d05cdae09d3d7))
  - Drive add-liquidity quoting from addLiquidityQuote in the UI ([60e38f4](https://github.com/logos-blockchain/lez-programs/commit/60e38f4e5f2a0b7623684b37307b8e1363231e22))
  - Wire the add-liquidity submit end-to-end ([71fb18c](https://github.com/logos-blockchain/lez-programs/commit/71fb18c50f0370b56bca4684bd6cb66f8ef1ad45))
  - Pick the token account per side when creating a pool ([b1b4631](https://github.com/logos-blockchain/lez-programs/commit/b1b4631234e2802a1e12967589ab7cf049479514))
  - Pick the token account per swap side ([9680010](https://github.com/logos-blockchain/lez-programs/commit/968001066cf7fabdeea81127f5a8657328f263f1))
  - Create pools via the new createPool op; drop the pool-watch poll ([e1398ff](https://github.com/logos-blockchain/lez-programs/commit/e1398ffcad22bffe1ef85eeb2ac1c594e1a07d80))
  - Submit exact-output swaps and drop client-side swap math ([4363f13](https://github.com/logos-blockchain/lez-programs/commit/4363f13912e0ba8627e70575161862ab337c1169))
  - Drive the exact-output swap preview from swapExactOutQuote ([d9876d0](https://github.com/logos-blockchain/lez-programs/commit/d9876d08ca0df7ade9dabaa8c7d35f74b303e82f))
  - Drive the exact-input swap preview from swapExactInQuote ([37c2f29](https://github.com/logos-blockchain/lez-programs/commit/37c2f294ac65f1b866f52ef2ba5f74b653c80c43))
  - Add create-pool / new liquidity position flow ([01829a2](https://github.com/logos-blockchain/lez-programs/commit/01829a280ca9bf601dbd1b6293a0acf997cb6d53))
- **modules/amm:**
  - Add setAmmProgramId to select the program id at runtime ([b167052](https://github.com/logos-blockchain/lez-programs/commit/b167052e0a3078520b1a7598fb56e49deeb94a7f))
  - Add sync-reserves module op ([62dc451](https://github.com/logos-blockchain/lez-programs/commit/62dc45177da2ecfc66348fbff34f0fc59efa9b46))
  - Add remove-liquidity module ops ([44b70e4](https://github.com/logos-blockchain/lez-programs/commit/44b70e4333b14c6d4e7adab647da50c2160bfd8e))
  - Add_liquidity_quote takes slippage, returns minimumLpRaw ([37f28fe](https://github.com/logos-blockchain/lez-programs/commit/37f28fe66391d3227ca5692cb01f54aecec5cb13))
  - Add the add-liquidity API and quoting ([0eaf514](https://github.com/logos-blockchain/lez-programs/commit/0eaf51476bb7164e2c839674ce8e5d8a7cedd092))
  - Add tokenHoldings — list the wallet's token holdings ([2a3be12](https://github.com/logos-blockchain/lez-programs/commit/2a3be1278aea89d6167c44baf4114e11177fea97))
  - Add createPool quote + plan ops and module methods ([526d50b](https://github.com/logos-blockchain/lez-programs/commit/526d50bff1d6534c6f684655f79822c45e092cc6))
  - Add swap_exact_out_plan op and module swapExactOutput ([56b2d3a](https://github.com/logos-blockchain/lez-programs/commit/56b2d3a282d37c91644a516d5f9f76b529c283d9))
  - Add swap_exact_out_quote op and module swapExactOutQuote ([d038b3d](https://github.com/logos-blockchain/lez-programs/commit/d038b3d060b775c71b5c92af102091d845e9ded8))
  - Add swap_exact_in_quote op and module swapExactInQuote ([abc6d27](https://github.com/logos-blockchain/lez-programs/commit/abc6d27a9fc6e79a0cb453be128a7b4b7dd72b8b))
  - Add pool_id operation ([6a951f3](https://github.com/logos-blockchain/lez-programs/commit/6a951f3cadc15d402e8f42ee5bc4442e1abe1edc))
- **stablecoin:**
  - Add Logos API module ([a7faa92](https://github.com/logos-blockchain/lez-programs/commit/a7faa92b48e5d46cbb23bcf261393f3f51cbb9af))
  - Wire poke guest entries + e2e tests ([2ccf90e](https://github.com/logos-blockchain/lez-programs/commit/2ccf90ef3d9b8e573592b2c4be7adf3f8f5d3936))
  - Implement refresh_globals host function ([bc2544f](https://github.com/logos-blockchain/lez-programs/commit/bc2544f69dc2ecee404d96cf83c189f1592ce6d3))
  - Implement update_redemption_rate host function ([bc8d422](https://github.com/logos-blockchain/lez-programs/commit/bc8d4222ef08ac2692579fbf0ca6db798e3505c1))
  - Implement accrue_stability_fee host function ([bf3e393](https://github.com/logos-blockchain/lez-programs/commit/bf3e393d32f40287dd5f7e34970f5c2c362a8b9b))
  - Add poke instruction variants ([3b427cf](https://github.com/logos-blockchain/lez-programs/commit/3b427cf1191f653fa8693e2c17e685d525754ee7))
  - Add redemption rate controller + clamp constants ([bdf3e27](https://github.com/logos-blockchain/lez-programs/commit/bdf3e275ffc4cb26768014f830dd8913dc1b2f1e))
  - Add current value projection helpers ([91f0794](https://github.com/logos-blockchain/lez-programs/commit/91f07949af0c485a90314c87cd3a8d445bec68df))
  - Expose initialize_program guest entry + e2e test ([ebc9b4b](https://github.com/logos-blockchain/lez-programs/commit/ebc9b4b9fef50456e9aa3cb4735f388cdfa4b7b5))
  - Implement initialize_program host function ([0410d83](https://github.com/logos-blockchain/lez-programs/commit/0410d83ae40bb1c4fdda6884d5b226e0d21d585e))
  - Add Instruction::InitializeProgram variant ([5b67d8d](https://github.com/logos-blockchain/lez-programs/commit/5b67d8d886a339b607255f46de621a055bf5906d))
  - Add RedemptionPriceState account type ([4e087e6](https://github.com/logos-blockchain/lez-programs/commit/4e087e6ff60032f25901c8b7aa12e523471f88e2))
  - Add StabilityFeeAccumulator account type ([0a44380](https://github.com/logos-blockchain/lez-programs/commit/0a44380a701120f8849d588384c61a060b7c87da))
- **token:**
  - Add token definition app ([24b66f4](https://github.com/logos-blockchain/lez-programs/commit/24b66f4c7790febc0e17419be0e1b21a26a8a844))
  - Add Logos token API module ([741e72a](https://github.com/logos-blockchain/lez-programs/commit/741e72add94475658d2261aca4325526f0a2ca2d))
- **token-mint-authority:**
  - Add testnet faucet mint-authority program ([95a6dff](https://github.com/logos-blockchain/lez-programs/commit/95a6dff56f462e97929d025ce1547af94c73bde2))
- **token-ui:**
  - Integrate Basecamp token module ([470e06c](https://github.com/logos-blockchain/lez-programs/commit/470e06c6c365982dc3db226276e8ee1b2ee2a3bb))
  - Connect Basecamp UI to token module ([dc386de](https://github.com/logos-blockchain/lez-programs/commit/dc386dee177dff46dbd1e71b44aae4326c10421c))
- **wallet:**
  - Add reusable ProgramAccountSelector component ([e0ae320](https://github.com/logos-blockchain/lez-programs/commit/e0ae3208a188bfa9a7893c1d97cc562d31218399))
  - Add reusable wallet modules ([64ce091](https://github.com/logos-blockchain/lez-programs/commit/64ce0910453121102c200c78bc47417861bab69e))

### Bug Fixes

- **amm:**
  - Restore liquidity controls after refresh ([574d814](https://github.com/logos-blockchain/lez-programs/commit/574d814f48dec1d63edec4f5da8f40141402c1ac))
  - Restrict UpdateConfig to authority transfer only **[breaking]** ([de9a3d5](https://github.com/logos-blockchain/lez-programs/commit/de9a3d532018733a7e8bceaf54d7a37a6f4141bd))
  - Align execution zone dependencies ([54afb26](https://github.com/logos-blockchain/lez-programs/commit/54afb26087d1b36ea3778ed2502e1f3ca1b7dbd3))
  - LE-serialize instruction words; correct ELF→ProgramBinary docs; clarify sharedWalletIsOpen ([e03164b](https://github.com/logos-blockchain/lez-programs/commit/e03164baf33b6638d16580b1c6fcd99adb887b67))
  - Guard resolvePool against stale callbacks; clarify deadline-ms and program-binary docs ([cf92e5d](https://github.com/logos-blockchain/lez-programs/commit/cf92e5d111060d0a86b69a7b18d551fcc23bf6c9))
  - Address Copilot review — exact BigInt min_out, fail on oversized pool fee, sync flake run docs ([fe41baf](https://github.com/logos-blockchain/lez-programs/commit/fe41baf21098f54759c58a3720fc89e1711e4428))
  - Order swap holdings/reserves by pool token order; accept base58 ids ([caf53d0](https://github.com/logos-blockchain/lez-programs/commit/caf53d0409b84bbf9987389614a7bc03bd1b502f))
- **amm-ui:**
  - Cache network snapshot to avoid remote calls on the hot path ([8358cfa](https://github.com/logos-blockchain/lez-programs/commit/8358cfa2f1dff14b68585c2f6e5252976d7da910))
  - Require explicit liquidity inputs ([aafe5e9](https://github.com/logos-blockchain/lez-programs/commit/aafe5e900b8faab83d3146053fdaf0a67d7369ad))
- **apps/amm:**
  - Feed wallet restore-keys the fresh-home 3-input order ([9921571](https://github.com/logos-blockchain/lez-programs/commit/99215716290c1237d9ce9f2c7e2fd8d0cea1d9c4))
  - One token list and one token picker for both views ([4651f28](https://github.com/logos-blockchain/lez-programs/commit/4651f28a053515ff2e4f0dad97c19f15a8739788))
  - Make the AMM UI load in Basecamp ([627fcfa](https://github.com/logos-blockchain/lez-programs/commit/627fcfa4e26540654509f9a225420546ad0295d6))
  - Match swap holdings on the configured id encoding ([72a3e74](https://github.com/logos-blockchain/lez-programs/commit/72a3e741a04289f4f57b81ef5a0d318cd4287855))
  - Repair addCustomToken — restore token resolution and closing brace ([10b52ea](https://github.com/logos-blockchain/lez-programs/commit/10b52ea2f6da5f418c406766398d45fcd58b6906))
  - Disable the already-selected token in the swap token picker ([bf63070](https://github.com/logos-blockchain/lez-programs/commit/bf63070a9e55888c94add9aa5ef035b0a1b33148))
  - Ensure changing endpoint works ([266c20a](https://github.com/logos-blockchain/lez-programs/commit/266c20a90e8f4f402bc2eefbd1721ce4758c952e))
- **apps/wallet:**
  - Use @loader_path rpath for the QML plugin on macOS ([1dcfb78](https://github.com/logos-blockchain/lez-programs/commit/1dcfb784f8588f8fd86dd647a8b089e4b2d8858c))
- **modules/amm:**
  - Swap plan must use the pool's stored vault ids ([bf1f76b](https://github.com/logos-blockchain/lez-programs/commit/bf1f76b051ad4254b7a0810b22db80b79403bc27))
- **stablecoin:**
  - Annotate initialize_program guest accounts ([092aa4a](https://github.com/logos-blockchain/lez-programs/commit/092aa4ac9800e7ad6a1c28c342a6361f5ccef913))
- **wallet:**
  - Send tx instruction as a byte string, not QVariantList<u32> ([f9bd853](https://github.com/logos-blockchain/lez-programs/commit/f9bd85336f9c571a108d630afb9f9bde29fba856))

### Refactor

- **amm:**
  - Drop the `Raw` suffix from amount/value field names ([4cb7c7e](https://github.com/logos-blockchain/lez-programs/commit/4cb7c7e51c940ad25631f0fde1b32b9df0510543))
  - Move tokenList off the module; app reads TOKENS_CONFIG ([09f3d59](https://github.com/logos-blockchain/lez-programs/commit/09f3d594a7505907e4114cbd5e05f1415fdc08ba))
  - Enrich resolvePool into resolvePoolAccount ([92f5565](https://github.com/logos-blockchain/lez-programs/commit/92f55652a87f02ae877931620502e77f4502280a))
  - Remove the dead Network/context machinery ([cbb75c3](https://github.com/logos-blockchain/lez-programs/commit/cbb75c38fd4a21a9d2d8f3c60b1df7042e1b809b))
  - Rename the create-pool quote surface for symmetry ([c47f387](https://github.com/logos-blockchain/lez-programs/commit/c47f387ad4f105f203631ac39fbd21a583930678))
  - Remove the dead newPosition quote path ([64e7614](https://github.com/logos-blockchain/lez-programs/commit/64e7614e742483f8f99cf567fc3e7c50bcc50ad7))
  - Remove the dead submitNewPosition path ([b1b6ec8](https://github.com/logos-blockchain/lez-programs/commit/b1b6ec851746731b856a3e4b053469b6c49da59d))
  - Extract swap-input formula into amm_core ([cd8a843](https://github.com/logos-blockchain/lez-programs/commit/cd8a84374b2cb019e8edcbd5386e9f20d0f9d180))
  - Extract swap-output formula into amm_core ([c3dc9dd](https://github.com/logos-blockchain/lez-programs/commit/c3dc9dd94fe73e8a1a9314490cbedb270b2d3c82))
  - Drop the new-position schema version tag ([afeba56](https://github.com/logos-blockchain/lez-programs/commit/afeba568d8d0758b4ac9f547248197c8617a1613))
  - Consolidate the two client FFIs into one JSON crate ([737b2f6](https://github.com/logos-blockchain/lez-programs/commit/737b2f674a72bcfd8480c149bf1c9e81aec1ead3))
  - Select swap direction by input holding, sign only the input **[breaking]** ([cca063c](https://github.com/logos-blockchain/lez-programs/commit/cca063ce2d315229d7c802fd7a0fd6bad74557ae))
- **apps/amm:**
  - Drop the registry timestamp field ([ce8c157](https://github.com/logos-blockchain/lez-programs/commit/ce8c157ccf4a5b0ec105aa77801f07a1c0d70435))
  - Extract RegistryLoader + registry-refresh plumbing ([5229928](https://github.com/logos-blockchain/lez-programs/commit/522992897cf9f6c72ce8d2dd30a534f9e93d6a12))
  - Decouple token/pool loading from its byte source ([01b4a36](https://github.com/logos-blockchain/lez-programs/commit/01b4a365b0a78d0f8ffaaa85c258c24bc1aa3057))
  - Drop dead liquidity-form leftovers from the quote migration ([037e0a1](https://github.com/logos-blockchain/lez-programs/commit/037e0a192c55721c9a7a7da0c27cf4fd5922931e))
  - Move the amm_client crate into modules/amm/ffi as amm_ffi ([f5ff9b8](https://github.com/logos-blockchain/lez-programs/commit/f5ff9b829f2a5bbba0d9a90134eeaa30a7b6d35a))
- **modules/amm:**
  - ResolvePool accepts base58 ids and orients reserves ([c95322c](https://github.com/logos-blockchain/lez-programs/commit/c95322c033fe9342b6a35254d4c9cfdd807424bd))
  - Drop status/code from swap_exact_in_plan ([b2f0e4b](https://github.com/logos-blockchain/lez-programs/commit/b2f0e4b85144b41bfa78383d23ffbdda4e4eabc4))
  - Rename swap_plan to swap_exact_in_plan ([a389dc6](https://github.com/logos-blockchain/lez-programs/commit/a389dc6056de3502f4610582a915801c3aa9149b))
- **stablecoin:**
  - Address review on Position migration ([f62444f](https://github.com/logos-blockchain/lez-programs/commit/f62444ffa2bcdef16c6fc0c6c6b682be454ef449))
  - Migrate Position to spec §4.4 shape **[breaking]** ([2d33923](https://github.com/logos-blockchain/lez-programs/commit/2d3392393a4981c4c7bb00be77d94cc33da6e245))
  - Simplify RedemptionPriceState PDA domain ([ed6e20e](https://github.com/logos-blockchain/lez-programs/commit/ed6e20e11cb715dd4a0a4aff695445dc51db1938))
  - Simplify StabilityFeeAccumulator PDA domain ([d9b7366](https://github.com/logos-blockchain/lez-programs/commit/d9b7366990b3233d248f7149de6e490f4abbca26))
- Rename nssa/nssa_core → lee/lee_core ([154b41b](https://github.com/logos-blockchain/lez-programs/commit/154b41ba5c7df88a01da77c726cfe8a4d58bd0bc))

### Documentation

- **amm-ui:**
  - Clarify test wallet password (throwaway setup vs restore-keys) ([c5b9508](https://github.com/logos-blockchain/lez-programs/commit/c5b9508e4d9347b963213ab0ecce3392c2d06bb4))
  - Document isolated UI test flow in tests/README.md ([191942f](https://github.com/logos-blockchain/lez-programs/commit/191942f2a85be66d96fe3d9ee98334196ebb1ea5))
- **apps/amm:**
  - Remove stale BIN reference ([96dc69c](https://github.com/logos-blockchain/lez-programs/commit/96dc69c39f5e199176186c264bbd1a9ca70780a8))
- **stablecoin:**
  - Document the CLOCK_01 clock account ([0a1f30a](https://github.com/logos-blockchain/lez-programs/commit/0a1f30a7d536e516686afe07791da433f3f91cfd))
  - Reference issues instead of plan names ([3204a37](https://github.com/logos-blockchain/lez-programs/commit/3204a3758846e9f781f4f2fef33201910e8f36e7))
- **token:**
  - Add token program usage runbook" ([ad020b1](https://github.com/logos-blockchain/lez-programs/commit/ad020b1c7fe99c4a676d49c42a93f55a6b3ad64d))
- Note that Instruction variants need guest entries, run make clippy-guest ([a338de4](https://github.com/logos-blockchain/lez-programs/commit/a338de44c342e3c3a032ebb020f7de1179b8c246))

### Testing

- **amm-ui:**
  - Feed setup password via stdin so bootstrap never blocks ([200f429](https://github.com/logos-blockchain/lez-programs/commit/200f429ec61c83254222eb2b9499285cea5ad996))
  - Feed wallet setup password non-interactively ([0576b10](https://github.com/logos-blockchain/lez-programs/commit/0576b10dcba039c39590ebb61820c4e882a555a2))
  - Isolate test token config, fix repo-root resolution ([2ed1350](https://github.com/logos-blockchain/lez-programs/commit/2ed13506d1e5c69dace84665c43ac10134ad3340))
  - Bootstrap deterministic test wallet in testnet setup ([9a1f76f](https://github.com/logos-blockchain/lez-programs/commit/9a1f76ff6b1e456e13c9a6f270d32fa73fd331f3))
  - Add isolated AMM testnet setup script ([380bbff](https://github.com/logos-blockchain/lez-programs/commit/380bbff30876805e95723dc02366fb669a235259))
  - Add swap UI test ([fc7b071](https://github.com/logos-blockchain/lez-programs/commit/fc7b07174cf8f52fc0faef3c870b0c8c82dc4458))
- **apps/amm:**
  - Emit a local registry from the AMM testnet setup script ([29e4885](https://github.com/logos-blockchain/lez-programs/commit/29e48858b9dbe7837e9ae8c16c253b9275ec795b))
  - Add remove-liquidity e2e test ([68c0e9c](https://github.com/logos-blockchain/lez-programs/commit/68c0e9cd23227c260b0cf4b9ea3a75b7fb5194cf))
  - Select the funding account before submitting a swap ([86575a6](https://github.com/logos-blockchain/lez-programs/commit/86575a65aad881fb3b9c34607506b7f0cc2228e0))
  - Add create-pool UI e2e test + Token C setup ([02e7703](https://github.com/logos-blockchain/lez-programs/commit/02e77032b7fdcc487de59ccdc654af1200366604))
- **privacy:**
  - Add privacy-preserving coverage, ported to lez v0.2.4 ([5b54b88](https://github.com/logos-blockchain/lez-programs/commit/5b54b8868ea195659ccfeae6a3ea93ad62f02932))

### Build System

- **amm:**
  - Fork logos_execution_zone for QtRO byte-string tx args + align amm_client_ffi to lee_core v0.2.0 ([69cd58d](https://github.com/logos-blockchain/lez-programs/commit/69cd58d3775e1d7ea78794181531a3da45062794))
  - Load amm_client_ffi via absolute store-path id + DYLD fallback on macOS ([8f67c4c](https://github.com/logos-blockchain/lez-programs/commit/8f67c4c4b639c61c1bc2c141f4ce6b042ddabbc9))
  - Link amm_client_ffi into the AMM UI module ([cfb62e4](https://github.com/logos-blockchain/lez-programs/commit/cfb62e4d2fb7dc1928b21f017295cb2aabead971))
- **flake:**
  - Pin lez_core to the byte-string-fix branch ([9346106](https://github.com/logos-blockchain/lez-programs/commit/9346106f44ba1c25801c066dfb93fb6d7b735c8a))
  - Make the amm module and UI flakes self-contained ([dd0e550](https://github.com/logos-blockchain/lez-programs/commit/dd0e550b2b7d94355345303847966ce3bcb6e76b))
  - Expose portable LGX outputs for the core modules ([235d6a4](https://github.com/logos-blockchain/lez-programs/commit/235d6a450cbd2ddab2eaf2e1902958d310a49798))

### CI

- Trigger gh actions on any PR ([8cc4055](https://github.com/logos-blockchain/lez-programs/commit/8cc4055f64a24fff6979393c784fe1993ae9f9fc))

### Chores

- Reformat three comments for current nightly rustfmt ([6b53f3a](https://github.com/logos-blockchain/lez-programs/commit/6b53f3a13f1c0846984f7f4d8e6082b15fa970b8))
- Update to release set v0.2.1 ([1d88e6f](https://github.com/logos-blockchain/lez-programs/commit/1d88e6f3c40060eef31708125cdca1490c5e5539))
- Remove unnecessary macos prerequisites docs ([d7017a2](https://github.com/logos-blockchain/lez-programs/commit/d7017a25159e106f5843a56a4c2426a1f118ac16))

### Styling

- **amm:**
  - Cargo fmt ([f1fc061](https://github.com/logos-blockchain/lez-programs/commit/f1fc0616649f7e8c3d413542207f3ec2c78488fc))

## [1.0.0] - 2026-07-16

### ⚠️ Breaking Changes

- **ata:** `Instruction::Transfer`, `Instruction::Burn`, `Instruction::Create` now requires a `token_program_id` field. Any existing call site that omits it will fail to compile. ([5229855](https://github.com/logos-blockchain/lez-programs/commit/5229855d57cc05623ca2f776f2b99b912d9ac195))
- **amm:** AddLiquidity, RemoveLiquidity, SwapExactInput, SwapExactOutput, and NewDefinition instruction variants now require a `deadline` field. ([6b21c36](https://github.com/logos-blockchain/lez-programs/commit/6b21c3695a5dd1ac321e5965293d7010df7e9c6e))
- Update dependencies and implement new required features across multiple modules ([471abef](https://github.com/logos-blockchain/lez-programs/commit/471abef7192b4a77f341eab8bc49c7c4c79e48fd))
- **amm:** `PoolDefinition` Borsh serialization format has changed. Existing on-chain pool accounts encoded with the `active` field are incompatible with this version. ([4a9a441](https://github.com/logos-blockchain/lez-programs/commit/4a9a441ccd19d038e6fe304f459d4b92410645e7))
- **amm:** The Swap instruction variant and swap() function are renamed to SwapExactInput and swap_exact_input(). Callers must update instruction construction and any IDL-generated bindings. ([4419e1e](https://github.com/logos-blockchain/lez-programs/commit/4419e1e9a004524728f5a31ba898363759de7439))
- **amm:** NewDefinition instruction requires an additional LP-lock holding account derived via `compute_lp_lock_holding_pda(amm_program_id, pool_id)`. ([fddd6e1](https://github.com/logos-blockchain/lez-programs/commit/fddd6e15bdd6a4ccc10b6993c3f4f1ae15cb7ae1))

### Features

- **amm:**
  - Wire the AMM app to the LEZ wallet module ([751d4ac](https://github.com/logos-blockchain/lez-programs/commit/751d4ac530e24fec5c1ca1598907cb94648f0e9e))
  - Create TWAP oracle price account on behalf of the pool ([53e563f](https://github.com/logos-blockchain/lez-programs/commit/53e563f8e3ea8373c66e03b130d9dbd5255c004d))
  - Refresh TWAP current tick on every reserve-mutating instruction ([0e2c5f9](https://github.com/logos-blockchain/lez-programs/commit/0e2c5f93298c47fe6836c4ce3e8818f9486301a8))
  - Bootstrap pool TWAP current-tick account at pool creation ([b997ca6](https://github.com/logos-blockchain/lez-programs/commit/b997ca678e68e3ca9d663beaca887a2a4b1a3780))
  - Create TWAP price observations on behalf of the pool ([4e43389](https://github.com/logos-blockchain/lez-programs/commit/4e4338945df6a82abf9531dbbab39aa45bd9f667))
  - Add admin authority and UpdateConfig instruction ([1d9e3dc](https://github.com/logos-blockchain/lez-programs/commit/1d9e3dcb49cb73a7f358a82374cc79ff78812bca))
  - Add Initialize instruction with config-gated chained calls ([3624ea1](https://github.com/logos-blockchain/lez-programs/commit/3624ea1451ea59bddba2c3537b86233a79305157))
  - Add transaction deadlines to swap and liquidity instructions **[breaking]** ([6b21c36](https://github.com/logos-blockchain/lez-programs/commit/6b21c3695a5dd1ac321e5965293d7010df7e9c6e))
  - Apply trading fees to LP accounting ([6c86b5b](https://github.com/logos-blockchain/lez-programs/commit/6c86b5b9cec4ea2253fd36d3797005558972e6f4))
  - Add configurable fee tiers ([9824cd8](https://github.com/logos-blockchain/lez-programs/commit/9824cd8f90e57aaf87d2e3c237e6eaf667bc5441))
  - Add swap exact output instruction ([664fd84](https://github.com/logos-blockchain/lez-programs/commit/664fd849bd2bfc1cffd17ec99ffae9a739257828))
  - Add SyncReserves instruction ([e61cd59](https://github.com/logos-blockchain/lez-programs/commit/e61cd594b532312a8a43d2b4c185d45b9682600e))
  - Introduce minimum liquidity lock on pool initialization **[breaking]** ([fddd6e1](https://github.com/logos-blockchain/lez-programs/commit/fddd6e15bdd6a4ccc10b6993c3f4f1ae15cb7ae1))
- **amm-ui:**
  - Add navbar to switch between views ([0cec2e2](https://github.com/logos-blockchain/lez-programs/commit/0cec2e25735ff361165c3efd8985880f0c859526))
- **amm/ui:**
  - Show swap summary under the swap card ([3df3c3d](https://github.com/logos-blockchain/lez-programs/commit/3df3c3d7c444542f33b2ebbc1ad1a867f6949508))
  - Add mock confirmation flow ([a3bdc96](https://github.com/logos-blockchain/lez-programs/commit/a3bdc964c7f8a89f148b89f07756b4efe738e5c5))
  - Add slippage min received controls ([d92a61f](https://github.com/logos-blockchain/lez-programs/commit/d92a61fd9b6f6bafed30927cae2314edfdbb22be))
  - Add remove-liquidity preview ([bf9001c](https://github.com/logos-blockchain/lez-programs/commit/bf9001c363a22e54ec3d00acfbfe4987cfb0e73c))
  - Add liquidity deposit preview ([67b1e50](https://github.com/logos-blockchain/lez-programs/commit/67b1e501e84cdc14a6eb8663804ee73e8cd3cead))
  - Add pool position summary ([f5a0106](https://github.com/logos-blockchain/lez-programs/commit/f5a01063ed99e97e109882966cfe71eaf4cce10c))
  - Add initial AMM UI module ([29d949d](https://github.com/logos-blockchain/lez-programs/commit/29d949d75ac375395e22269ed5f1e73bfa8537b2))
- **stablecoin:**
  - Add ProtocolParameters account type ([5b82a52](https://github.com/logos-blockchain/lez-programs/commit/5b82a52c6b812a541b0b864a2fca8aa9692aaac1))
  - Add fixed-point math utilities ([3eab96a](https://github.com/logos-blockchain/lez-programs/commit/3eab96a2176f337375e048f61573d973a6e385e9))
  - Implement `repay_debt` ([cdb53a4](https://github.com/logos-blockchain/lez-programs/commit/cdb53a4d0ca808c1071e0527be2fa6018ca38a3f))
  - Implement `withdraw_collateral` ([eb7f44a](https://github.com/logos-blockchain/lez-programs/commit/eb7f44a98a7e7338b993e8494087339c8c30815c))
  - Implement `open_position` ([f4f7b45](https://github.com/logos-blockchain/lez-programs/commit/f4f7b45bd4270efa796b1a46077ed6bb9adc8abd))
  - Initial stablecoin scaffold ([4178406](https://github.com/logos-blockchain/lez-programs/commit/4178406fdacaa230e42fca5008474dcd179ddf47))
- **token:**
  - Add mint authority model to token program ([fe4c7a9](https://github.com/logos-blockchain/lez-programs/commit/fe4c7a96da393808946d0ffdb9ef44a5da9d8ef0))
  - Verify definition ownership via self_program_id in initialize and mint ([8005c74](https://github.com/logos-blockchain/lez-programs/commit/8005c74e2620f8911f9ddf76003054a90c2b4413))
- **twap-oracle:**
  - Implement PublishPrice with tick-to-price conversion and tail extrapolation ([c528d85](https://github.com/logos-blockchain/lez-programs/commit/c528d85a2b60ddfa76f1217ae1bd3e683c85939a))
  - Implement RecordTick instruction ([e8fe634](https://github.com/logos-blockchain/lez-programs/commit/e8fe634a2cb40099d6e90ffabaaddd39e3e60da0))
  - Implement CreateCurrentTickAccount and UpdateCurrentTick ([3285d57](https://github.com/logos-blockchain/lez-programs/commit/3285d5787e1c18bdd127ca1d7815c0dd62e780aa))
  - Implement CreateOraclePriceAccount instruction ([7461c95](https://github.com/logos-blockchain/lez-programs/commit/7461c9552b703d06723607bedb806bc83cbebba4))
  - Implement CreatePriceObservations instruction ([fe9d919](https://github.com/logos-blockchain/lez-programs/commit/fe9d919299c743553198f706ebfb9b34f5b18ea6))
- Make use of spel's `[#account_type]` directive ([f4a0aaf](https://github.com/logos-blockchain/lez-programs/commit/f4a0aaf8d0e5caf3240f887a37db35de59dc79a9))

### Bug Fixes

- **amm:**
  - Compute pool arithmetic in u256 to avoid u128 overflow ([2308681](https://github.com/logos-blockchain/lez-programs/commit/2308681dcf16523cb089e73717dc166c1ac2b9c9))
  - Require signer on user token holdings in swap and add-liquidity ([c8f061e](https://github.com/logos-blockchain/lez-programs/commit/c8f061e4a863f6a19d301e804ef54c9f20994c32))
  - Validate user deposit accounts are owned by vault's token program ([e69c910](https://github.com/logos-blockchain/lez-programs/commit/e69c9107f0980e89548d09a833f8335ec077c5dd))
  - Use checked mul/add/sub to avoid overflows/underflows ([36f78a2](https://github.com/logos-blockchain/lez-programs/commit/36f78a21aa43cce619d58128bab725e590041a90))
- **ata:**
  - Namespace accounts by token program **[breaking]** ([5229855](https://github.com/logos-blockchain/lez-programs/commit/5229855d57cc05623ca2f776f2b99b912d9ac195))
  - Lock down `ATA::Transfer` recipient contract ([f8cbcc6](https://github.com/logos-blockchain/lez-programs/commit/f8cbcc6956c87daa604a4a1ea856e15faf24560a))
- **idl:**
  - Align LEZ account metadata ([255f87f](https://github.com/logos-blockchain/lez-programs/commit/255f87f38f3b801f10d84d50dc578c01fdc4f484))
- **idl-gen:**
  - Sort types array for deterministic output ([b0ac300](https://github.com/logos-blockchain/lez-programs/commit/b0ac30039b054ef0711433aa8ab6ef2506b6ef26))
- **integration_tests:**
  - Remove no longer needed program ID ([29b4c01](https://github.com/logos-blockchain/lez-programs/commit/29b4c0173991b32548b5fcb3d7ec18742dc77070))
- **stablecoin:**
  - Leave position_post.program_owner default so the runtime sets it through Claim::Pda ([7da110a](https://github.com/logos-blockchain/lez-programs/commit/7da110a616ce044a82fb349047f80bd0c08ee4cf))
  - Address open position review feedback ([0b078b2](https://github.com/logos-blockchain/lez-programs/commit/0b078b2dde71209228bd4ba2eee29ffb1edffe49))
- **twap_oracle:**
  - Validate clock account ([3ce998c](https://github.com/logos-blockchain/lez-programs/commit/3ce998c37c4f34cdf55fac1f9e227e77ef0c9327))

### Refactor

- **amm:**
  - Derive pool active state from LP supply instead of explicit flag **[breaking]** ([4a9a441](https://github.com/logos-blockchain/lez-programs/commit/4a9a441ccd19d038e6fe304f459d4b92410645e7))
- **pda:**
  - Use descriptive string seeds for PDA derivation ([03345db](https://github.com/logos-blockchain/lez-programs/commit/03345db8036d02dd0a2fa38d83f47dc84f298638))
- **twap_oracle:**
  - Match instruction function order to Instruction enum ([c9fbb62](https://github.com/logos-blockchain/lez-programs/commit/c9fbb626eaeb00594a3c7efd2aecc52e6cdcf338))
- Migrate programs to LEZ lez-core-v0.2.0 ([c42d4b6](https://github.com/logos-blockchain/lez-programs/commit/c42d4b6c07716ad7ad39ee3c7be7b4581cecf24d))
- Move programs into `programs` and UIs into `apps` ([3622016](https://github.com/logos-blockchain/lez-programs/commit/3622016e6c7c6b8813c287d2f1971520e7ed37b4))
- Rename rust-toolchain file ([20a9471](https://github.com/logos-blockchain/lez-programs/commit/20a947137c54037bd4c60baadb90f90a1d3b602d))
- Update dependencies and implement new required features across multiple modules **[breaking]** ([471abef](https://github.com/logos-blockchain/lez-programs/commit/471abef7192b4a77f341eab8bc49c7c4c79e48fd))

### Documentation

- **stablecoin:**
  - Move design document into docs folder ([3774d51](https://github.com/logos-blockchain/lez-programs/commit/3774d5112cdfd4195ad0d638a9a1df1ae61c7bc4))
  - Remove minimum time between fee acrruals ([222c01e](https://github.com/logos-blockchain/lez-programs/commit/222c01e7d60e32888ec17d6ba08f598e4ae9a969))
  - Add refresh_globals and use milliseconds everywhere ([5298c43](https://github.com/logos-blockchain/lez-programs/commit/5298c43dadfcdc264cb2bb28442c92dbff5c7f76))
  - Add more examples and debt accrual terminology ([b6c53e0](https://github.com/logos-blockchain/lez-programs/commit/b6c53e096d04fa77928e277dee029dfc3cdf5123))
  - Fix rate adjustment math ([78f61f4](https://github.com/logos-blockchain/lez-programs/commit/78f61f43e2b7423a58740156483f47b6f27ca74a))
  - Improve after comments ([b3369d2](https://github.com/logos-blockchain/lez-programs/commit/b3369d2a3d30bc87c221909b8f2060027a864330))
  - Fix sections references ([6331115](https://github.com/logos-blockchain/lez-programs/commit/63311157c8e37c5ea27e9d78b79080707fa4d7b7))
  - Add stablecoin design docs ([7c62668](https://github.com/logos-blockchain/lez-programs/commit/7c62668731b7a21c20a19377d01bcbcd1e0f21a5))
  - Improve documentation for OpenPosition instruction ([d6082d0](https://github.com/logos-blockchain/lez-programs/commit/d6082d0c816ad5753470664bd09a8889e0067dd0))
  - List all five `OpenPosition` accounts and qualify `size_of_val` ([e63d09f](https://github.com/logos-blockchain/lez-programs/commit/e63d09f793fed47dc640a58518de6d652ae734e2))
- Add a testnet run book to show how to deploy and use the programs ([0a120bd](https://github.com/logos-blockchain/lez-programs/commit/0a120bd42cb10d3b82acccc081432d23cdfd144f))
- Update README and CLAUDE.md to reflect current state ([7b1696f](https://github.com/logos-blockchain/lez-programs/commit/7b1696f98e37fa8e6c8de145bb230048ea5647de))

### Testing

- **ata:**
  - Add integration for private accounts create public ATAs ([d0f3988](https://github.com/logos-blockchain/lez-programs/commit/d0f398814c00ef54491e77d5bfd70f7a38966002))
- **stablecoin:**
  - Move chained-transfer coverage to integration tests ([1ae2b32](https://github.com/logos-blockchain/lez-programs/commit/1ae2b325fffbb8e2467a883d9a263cfad549bef2))
  - Cover invalid withdraw transfer pre-states ([a0a1e08](https://github.com/logos-blockchain/lez-programs/commit/a0a1e08dfbddd91320f0ea433969d925ab6a3a50))
- **twap:**
  - Cover CreateOraclePriceAccount and PublishPrice end-to-end ([bd8064a](https://github.com/logos-blockchain/lez-programs/commit/bd8064a587549edd12d44cb5efbc0af9d5a566c4))
  - Cover RecordTick end-to-end and add zkVM cycle benchmark ([9d5eea2](https://github.com/logos-blockchain/lez-programs/commit/9d5eea2b416d66fd3a0d6c00f3d0dd79d60ca056))

### Build System

- **deps:**
  - Bump spel to v0.6.0 and logos-execution-zone to v0.2.0 ([ff89025](https://github.com/logos-blockchain/lez-programs/commit/ff89025ead6bff700d7a2c41e92c64b9bb209075))
- **guest:**
  - Strip release symbols ([497e13d](https://github.com/logos-blockchain/lez-programs/commit/497e13db85e875ec902afd3c6f746de4b8eb7e12))
- Pin enum-ordinalize to 4.3.2 in the AMM guest lockfile ([4a6192d](https://github.com/logos-blockchain/lez-programs/commit/4a6192d84f4c084588b8d67d7fb7e4957fd2be70))
- Add shared guest program build ([a26debd](https://github.com/logos-blockchain/lez-programs/commit/a26debd5922c724923ca6e84ca4b61368ee27dc0))

### CI

- Add IDL freshness check and consolidate artifacts ([94f14ae](https://github.com/logos-blockchain/lez-programs/commit/94f14ae305a00beee55e310e1fe234be273045ef))

### Chores

- **Makefile:**
  - Add `idl` command to `Makefile` ([f4f61be](https://github.com/logos-blockchain/lez-programs/commit/f4f61be32272524ea887a7882892e9b9dbe5f5fa))
- **amm:**
  - Add Logos Basecamp support ([25b8b86](https://github.com/logos-blockchain/lez-programs/commit/25b8b861031432e57d850de3dcd88bc04a671ab8))
  - Validate fee tier in `sync_reserves` ([c8a192e](https://github.com/logos-blockchain/lez-programs/commit/c8a192e3779386588d8ec79c433ff084cb454d7f))
  - Add defensive check for lp token solvency ([0d532a8](https://github.com/logos-blockchain/lez-programs/commit/0d532a8fd31874eba0ee07885d2b7db2f977b2e2))
  - New_definition allows only uninitialized pools ([1f8eea8](https://github.com/logos-blockchain/lez-programs/commit/1f8eea84422d18efab1f3e271e292ddab2ee8b95))
  - Rename Swap instruction to SwapExactInput **[breaking]** ([4419e1e](https://github.com/logos-blockchain/lez-programs/commit/4419e1e9a004524728f5a31ba898363759de7439))
- **amm-ui:**
  - Reorganize liquidity page components ([22b41bd](https://github.com/logos-blockchain/lez-programs/commit/22b41bdb3d84c2442633a188aab935a966f96daa))
  - Add layout and reorganize swap UI files ([9375129](https://github.com/logos-blockchain/lez-programs/commit/9375129c9e42c205e0a178bc5d77c62e5b1e7192))
  - Add SlippageToleranceControl ([6eed55d](https://github.com/logos-blockchain/lez-programs/commit/6eed55d7e4381ef7c22cbd601ce268051285149d))
  - Swap form activates exact input or output based on the fields updated ([476087a](https://github.com/logos-blockchain/lez-programs/commit/476087a36b1b746b0c73602a9b909e081e4ae03d))
  - Add swap confirmation modal ([5a61cf3](https://github.com/logos-blockchain/lez-programs/commit/5a61cf39f25090789dfb1d881af6343637833a8d))
  - Update styles to match the liquidity page ([37fc2ea](https://github.com/logos-blockchain/lez-programs/commit/37fc2ea0885f62786bb360ae2411c0e2145f16f3))
  - Add basic swap UI for Token Pair Selector & Swap Direction ([e18f0f3](https://github.com/logos-blockchain/lez-programs/commit/e18f0f3c32447885e9af7018ef4d4626c0c1e6a4))
- **ata:**
  - Remove redundant test directive ([9ebd9fc](https://github.com/logos-blockchain/lez-programs/commit/9ebd9fcc12c60c4789452d52d71aeb2fffbc9d07))
  - Add IDL for ata program ([6287bd9](https://github.com/logos-blockchain/lez-programs/commit/6287bd9df91feeb9cc1fcaa4f481a86b0ab6ac26))
- **lint:**
  - Add staged lint baseline ([49d7f91](https://github.com/logos-blockchain/lez-programs/commit/49d7f91ee5edb15e00bb2b80194f8a8a6e4a75f6))
- **stablecoin:**
  - Use alloy primitives ([e93db41](https://github.com/logos-blockchain/lez-programs/commit/e93db419c40c352fdf6f7c635bbd47d8286b7326))
- **twap_oracle:**
  - Scaffold twap_oracle program ([291149b](https://github.com/logos-blockchain/lez-programs/commit/291149b114b1c4950478e81af835d72ebb46b237))
- Add helper examples to calculate program PDAs ([0fa2b49](https://github.com/logos-blockchain/lez-programs/commit/0fa2b49880b378a7837f2d36f644b7204f356366))
- Update to LEZ v0.2.0-rc6 ([091ea5a](https://github.com/logos-blockchain/lez-programs/commit/091ea5a5d09b31d549e8f8fe0c9f6854932560f3))
- Add integration test task to make file ([065a4e4](https://github.com/logos-blockchain/lez-programs/commit/065a4e4937a03424ec1d5141f1496e9d1bb40765))
- Pin ruint dep ([e444761](https://github.com/logos-blockchain/lez-programs/commit/e4447617f69a4f30a2794bead1ef64faba37c5e8))
- Include stablecoin program in clippy task ([cfa4bb1](https://github.com/logos-blockchain/lez-programs/commit/cfa4bb1e3676111c3db1708e9bfadee150a228b4))
- Adjust linting rules and introduce Makefile ([6fd8776](https://github.com/logos-blockchain/lez-programs/commit/6fd87766c299c8ede66dceca96164b77162c2d89))
- Update spel to v0.3.0 ([035f593](https://github.com/logos-blockchain/lez-programs/commit/035f593f5eaa2e0d578ea62a2c3f6e55ca74bf13))
- Update spel ([ceb8a4b](https://github.com/logos-blockchain/lez-programs/commit/ceb8a4b597d2cb9ae89a3ddd5a56986accf689b7))
- Update LEZ to v0.2.0-rc3 ([e7a69f6](https://github.com/logos-blockchain/lez-programs/commit/e7a69f619f677a1d5595ce29e320b566f9b74b48))
- Fix RISC-V guest build on macOS 26 ([06a141e](https://github.com/logos-blockchain/lez-programs/commit/06a141ef6eec25460bcb46ca7f8186fb170c0b11))
- Update `spel` and `logos-execution-zone` dependencies ([f89a8f9](https://github.com/logos-blockchain/lez-programs/commit/f89a8f9865cb8365a7269c4e8be01ce30b5815ba))
- Update `spel-cli` references to use `spel` ([cb8426c](https://github.com/logos-blockchain/lez-programs/commit/cb8426cbf1e4f53ffcb0b7b0d2763a07ecbd26d8))

## [0.1.0] - 2026-03-30

### Chores

- Initial repository setup for programs ([45ed284](https://github.com/logos-blockchain/lez-programs/commit/45ed284825cb403ce3dd53328060cd7a3e5ee6ba))

