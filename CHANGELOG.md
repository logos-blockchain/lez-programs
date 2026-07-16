# Changelog

All notable changes to the LEZ programs in this repository are documented here.
This file is generated from Conventional Commit messages by [git-cliff](https://git-cliff.org).

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

