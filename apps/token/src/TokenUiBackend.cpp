#include "TokenUiBackend.h"

// TokenUiBackend is header-only: its constructor and slot forwarders are inline,
// and all wallet behaviour lives in the shared WalletBackendLogic CRTP base
// (apps/common/wallet-ui). This translation unit exists so AUTOMOC compiles the
// generated moc for TokenUiBackend and its vtable is emitted here.
