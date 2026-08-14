#ifndef TOKEN_UI_BACKEND_H
#define TOKEN_UI_BACKEND_H

#include <QObject>

#include "rep_TokenUiBackend_source.h"

#include "AccountModel.h"
#include "WalletBackendLogic.h"

class LogosAPI;

// Per-app wallet backend. All wallet behaviour lives in the shared
// WalletBackendLogic CRTP base (apps/common/wallet-ui), parameterised on the
// QtRO source generated from TokenUiBackend.rep. This class only adds the
// module identity and the accountModel property exposed to QML.
class TokenUiBackend : public WalletBackendLogic<TokenUiBackendSimpleSource> {
    Q_OBJECT
    Q_PROPERTY(AccountModel* accountModel READ accountModel CONSTANT)

public:
    explicit TokenUiBackend(LogosAPI* logosAPI = nullptr, QObject* parent = nullptr)
        : WalletBackendLogic(logosAPI, parent, "token_ui", "TokenUI") {}
};

#endif // TOKEN_UI_BACKEND_H
