#ifndef WALLET_IDL_DECODER_H
#define WALLET_IDL_DECODER_H

#ifdef __cplusplus
extern "C" {
#endif

char *wallet_idl_decode_accounts(const char *request_json);
void wallet_idl_decoder_free(char *value);

#ifdef __cplusplus
}
#endif

#endif
