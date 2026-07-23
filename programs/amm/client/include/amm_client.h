#ifndef AMM_CLIENT_H
#define AMM_CLIENT_H

#ifdef __cplusplus
extern "C" {
#endif

/*
 * Accepts a tagged UTF-8 JSON request and returns an owned UTF-8 JSON envelope.
 * Supported operation tags: initialize, update_config, create_price_observations,
 * create_oracle_price_account, create_pool, add_liquidity, remove_liquidity,
 * swap_exact_input, swap_exact_output, sync_reserves, and the five
 * prepare_*_transaction task operations documented in docs/wire-api.md.
 * Release the result with amm_client_free.
 */
char *amm_client_plan(const char *request_json);

/*
 * Accepts a tagged UTF-8 JSON request and returns an owned UTF-8 JSON envelope.
 * Supported operation tags include protocol constants; config and pair discovery;
 * pair inspection; caller-order opening preparation; economic quote/preparation
 * operations; reserve synchronization; oracle-price initialization; raw sequencer
 * account normalization; and human-price Q64.64 conversion.
 * Snapshot-bound prepare_*_transaction operations belong to amm_client_plan.
 * See docs/wire-api.md for fields.
 * Release the result with amm_client_free.
 */
char *amm_client_quote(const char *request_json);

/*
 * Raw u128, u64, and signed tick values are decimal JSON strings. Program IDs
 * are JSON arrays of eight u32 words. Hexadecimal and byte layouts are host-adapter-only.
 * Instruction words are JSON u32 arrays. Account IDs use canonical base58 and account data is
 * hexadecimal.
 * Requests may carry schema "amm-client.v1";
 * schema-less legacy requests remain accepted. Responses use
 * {"schema":"amm-client.v1","ok":true,"value":...} or the same envelope
 * with ok=false and error={"code":...,"message":...}. Plan values contain
 * typed instructionArgs as well as exact RISC Zero instructionWords.
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
