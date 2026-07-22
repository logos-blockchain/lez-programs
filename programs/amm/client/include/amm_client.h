#ifndef AMM_CLIENT_H
#define AMM_CLIENT_H

#ifdef __cplusplus
extern "C" {
#endif

/*
 * Accepts a tagged UTF-8 JSON request and returns an owned UTF-8 JSON envelope.
 * Supported operation tags: initialize, update_config, create_price_observations,
 * create_oracle_price_account, create_pool, add_liquidity, remove_liquidity,
 * swap_exact_input, swap_exact_output, and sync_reserves.
 * Release the result with amm_client_free.
 */
char *amm_client_plan(const char *request_json);

/*
 * Accepts a tagged UTF-8 JSON request and returns an owned UTF-8 JSON envelope.
 * Supported operation tags: protocol_constants, pair_order, create_pool,
 * prepare_create_pool, preview_add_liquidity, prepare_add_liquidity, add_liquidity,
 * preview_remove_liquidity, prepare_remove_liquidity, remove_liquidity,
 * preview_swap_exact_input, prepare_swap_exact_input, swap_exact_input,
 * preview_swap_exact_output, prepare_swap_exact_output, swap_exact_output,
 * sync_reserves, and create_oracle_price_account.
 * Release the result with amm_client_free.
 */
char *amm_client_quote(const char *request_json);

/*
 * Raw u128 and u64 values are unsigned decimal JSON strings. Program IDs and
 * instruction words are JSON u32 arrays. Account IDs are base58 strings and
 * account data is hexadecimal. Responses use {"ok":true,"value":...} or
 * {"ok":false,"error":{"code":...,"message":...}}.
 */

/*
 * Releases a response returned by amm_client_plan or amm_client_quote.
 * Passing NULL is allowed. Every non-NULL response must be released exactly once.
 */
void amm_client_free(char *value);

#ifdef __cplusplus
}
#endif

#endif
