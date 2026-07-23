//! C-ABI client helpers for calling the AMM program from host apps.
//! All AMM instruction/PDA logic is delegated to `amm_core`; encoding is
//! delegated to `lee` — this crate only bridges to a C ABI.
#![allow(
    unsafe_code,
    reason = "this crate exists solely to expose a C ABI; every unsafe fn \
              is a documented pointer dereference at the FFI boundary"
)]

mod pda;
mod pool;
mod swap;

use nssa_core::account::AccountId;

/// C-ABI representation of `nssa_core::program::ProgramId` (an Image ID),
/// which is itself defined as `[u32; 8]`. Declared as a concrete array here
/// (rather than an alias through `nssa_core::program::ProgramId`) so
/// cbindgen — which only inspects this crate's source, not its
/// dependencies' — can emit a real typedef instead of an opaque,
/// self-referential `ProgramId` in the generated header. Since both are
/// plain type aliases to `[u32; 8]`, they remain interchangeable to rustc.
pub type ProgramId = [u32; 8];

/// Heap-allocated buffer of RISC0 instruction words, returned across the C
/// boundary. Free with `amm_client_free_words`.
#[repr(C)]
pub struct AmmWords {
    pub ptr: *mut u32,
    pub len: usize,
    pub ok: bool,
}

/// C-ABI mirror of `pool::PoolView`. u128 fields are little-endian bytes.
#[repr(C)]
pub struct FfiPoolView {
    pub def_a: [u8; 32],
    pub def_b: [u8; 32],
    pub vault_a: [u8; 32],
    pub vault_b: [u8; 32],
    pub reserve_a: [u8; 16],
    pub reserve_b: [u8; 16],
    pub liquidity_supply: [u8; 16],
    pub fees: u32,
    pub ok: bool,
}

/// C-ABI mirror of `pool::ConfigView`.
#[repr(C)]
pub struct FfiConfigView {
    pub token_program_id: [u32; 8],
    pub twap_oracle_program_id: [u32; 8],
    pub authority: [u8; 32],
    pub ok: bool,
}

/// # Safety
/// `p` must be a valid, non-null pointer to a readable `[u8; 32]`.
unsafe fn acc(p: *const [u8; 32]) -> AccountId {
    AccountId::new(*p)
}

/// Builds the RISC0 instruction words for a `SwapExactInput`. `amount_in` and
/// `min_out` are little-endian encoded `u128` values (`[u8; 16]`). The
/// returned buffer must be freed with `amm_client_free_words`.
///
/// # Safety
/// `amount_in` and `min_out` must be valid, non-null pointers to readable
/// memory of the indicated sizes.
#[no_mangle]
pub unsafe extern "C" fn amm_client_swap_words(
    amount_in: *const [u8; 16],
    min_out: *const [u8; 16],
    deadline: u64,
) -> AmmWords {
    let (a, m) = (
        u128::from_le_bytes(*amount_in),
        u128::from_le_bytes(*min_out),
    );
    match swap::swap_exact_input_words(a, m, deadline) {
        Ok(w) => {
            let boxed = w.into_boxed_slice();
            let len = boxed.len();
            let ptr = Box::into_raw(boxed).cast::<u32>();
            AmmWords { ptr, len, ok: true }
        }
        Err(_) => AmmWords {
            ptr: core::ptr::null_mut(),
            len: 0,
            ok: false,
        },
    }
}

/// Frees a buffer previously returned by `amm_client_swap_words`.
///
/// # Safety
/// `w` must be a value previously returned by `amm_client_swap_words` that
/// has not already been freed.
#[no_mangle]
pub unsafe extern "C" fn amm_client_free_words(w: AmmWords) {
    if !w.ptr.is_null() {
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
            w.ptr, w.len,
        )));
    }
}

/// Computes the `ProgramId` (Image ID) of a deployed program binary — the RISC
/// Zero `ProgramBinary` (`.bin`) format, NOT a raw ELF (the `elf` bytes are
/// decoded via `ProgramBinary::decode`). Returns `false` on invalid input,
/// leaving `out` unwritten.
///
/// # Safety
/// `elf` must be valid for reads of `elf_len` bytes, and `out` must be a
/// valid, non-null pointer to writable memory for a `ProgramId`.
#[no_mangle]
pub unsafe extern "C" fn amm_client_program_id_from_elf(
    elf: *const u8,
    elf_len: usize,
    out: *mut ProgramId,
) -> bool {
    let bytes = core::slice::from_raw_parts(elf, elf_len);
    match pda::program_id_from_elf(bytes) {
        Ok(id) => {
            *out = id;
            true
        }
        Err(_) => false,
    }
}

/// Fills `out` with the AMM config PDA.
///
/// # Safety
/// `amm` must be a valid, non-null pointer to a readable `ProgramId`, and
/// `out` must be a valid, non-null pointer to writable memory.
#[no_mangle]
pub unsafe extern "C" fn amm_client_config_pda(amm: *const ProgramId, out: *mut [u8; 32]) {
    *out = pda::config_pda(*amm).into_value();
}

/// Fills `out` with the pool PDA for the two definition ids.
///
/// # Safety
/// All pointer arguments must be valid, non-null, and point to readable (or,
/// for `out`, writable) memory of the indicated sizes.
#[no_mangle]
pub unsafe extern "C" fn amm_client_pool_pda(
    amm: *const ProgramId,
    def_a: *const [u8; 32],
    def_b: *const [u8; 32],
    out: *mut [u8; 32],
) {
    *out = pda::pool_pda(*amm, acc(def_a), acc(def_b)).into_value();
}

/// Fills `out` with the vault PDA for a pool + token definition.
///
/// # Safety
/// All pointer arguments must be valid, non-null, and point to readable (or,
/// for `out`, writable) memory of the indicated sizes.
#[no_mangle]
pub unsafe extern "C" fn amm_client_vault_pda(
    amm: *const ProgramId,
    pool: *const [u8; 32],
    def: *const [u8; 32],
    out: *mut [u8; 32],
) {
    *out = pda::vault_pda(*amm, acc(pool), acc(def)).into_value();
}

/// Fills `out` with the TWAP oracle current-tick PDA for a pool.
///
/// # Safety
/// All pointer arguments must be valid, non-null, and point to readable (or,
/// for `out`, writable) memory of the indicated sizes.
#[no_mangle]
pub unsafe extern "C" fn amm_client_current_tick_pda(
    twap: *const ProgramId,
    pool: *const [u8; 32],
    out: *mut [u8; 32],
) {
    *out = pda::current_tick_pda(*twap, acc(pool)).into_value();
}

/// Decodes a `PoolDefinition` account's raw bytes into `out`. Returns `false`
/// on decode failure, leaving `out` unwritten.
///
/// # Safety
/// `bytes` must be valid for reads of `len` bytes, and `out` must be a
/// valid, non-null pointer to writable memory for a `FfiPoolView`.
#[no_mangle]
pub unsafe extern "C" fn amm_client_decode_pool(
    bytes: *const u8,
    len: usize,
    out: *mut FfiPoolView,
) -> bool {
    let b = core::slice::from_raw_parts(bytes, len);
    match pool::decode_pool(b) {
        Ok(v) => {
            *out = FfiPoolView {
                def_a: v.def_a,
                def_b: v.def_b,
                vault_a: v.vault_a,
                vault_b: v.vault_b,
                reserve_a: v.reserve_a.to_le_bytes(),
                reserve_b: v.reserve_b.to_le_bytes(),
                liquidity_supply: v.liquidity_supply.to_le_bytes(),
                fees: v.fees,
                ok: true,
            };
            true
        }
        Err(_) => false,
    }
}

/// Decodes an `AmmConfig` account's raw bytes into `out`. Returns `false` on
/// decode failure, leaving `out` unwritten.
///
/// # Safety
/// `bytes` must be valid for reads of `len` bytes, and `out` must be a
/// valid, non-null pointer to writable memory for a `FfiConfigView`.
#[no_mangle]
pub unsafe extern "C" fn amm_client_decode_config(
    bytes: *const u8,
    len: usize,
    out: *mut FfiConfigView,
) -> bool {
    let b = core::slice::from_raw_parts(bytes, len);
    match pool::decode_config(b) {
        Ok(v) => {
            *out = FfiConfigView {
                token_program_id: v.token_program_id,
                twap_oracle_program_id: v.twap_oracle_program_id,
                authority: v.authority,
                ok: true,
            };
            true
        }
        Err(_) => false,
    }
}
