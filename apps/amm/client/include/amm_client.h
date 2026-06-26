#ifndef AMM_CLIENT_H
#define AMM_CLIENT_H

#ifdef __cplusplus
extern "C" {
#endif

char *amm_config_id(const char *request_json);
char *amm_token_ids(const char *request_json);
char *amm_pair_ids(const char *request_json);
char *amm_context(const char *request_json);
char *amm_quote(const char *request_json);
char *amm_plan(const char *request_json);
void amm_free(char *value);

#ifdef __cplusplus
}
#endif

#endif
