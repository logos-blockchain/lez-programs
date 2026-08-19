#include "TokenUiBackend.h"

#include "LogosWalletProvider.h"
#include "WalletController.h"
#include "logos_api.h"

TokenUiBackend::TokenUiBackend(LogosAPI* logosAPI, QObject* parent)
    : TokenUiBackendSimpleSource(parent),
      m_logosAPI(logosAPI ? logosAPI : new LogosAPI("token_ui", this)),
      m_wallet(std::make_unique<LogosWalletProvider>(m_logosAPI)),
      m_walletController(std::make_unique<WalletController>(
          *m_wallet, QStringLiteral("TokenUI")))
{
    connect(m_walletController.get(), &WalletController::stateChanged,
            this, &TokenUiBackend::syncWalletState);
    syncWalletState();
    m_walletController->start();
}

TokenUiBackend::~TokenUiBackend() = default;

WalletAccountModel* TokenUiBackend::accountModel() const
{
    return m_walletController->accountModel();
}

QString TokenUiBackend::createAccountPublic()
{
    return m_walletController->createAccount(true);
}

QString TokenUiBackend::createAccountPrivate()
{
    return m_walletController->createAccount(false);
}

QString TokenUiBackend::createNewDefault(QString password)
{
    const QString mnemonic = m_walletController->createDefaultWallet(password);
    syncWalletState();
    return mnemonic;
}

bool TokenUiBackend::openExisting()
{
    const bool opened = m_walletController->open();
    syncWalletState();
    return opened;
}

void TokenUiBackend::disconnectWallet()
{
    m_walletController->disconnect();
    syncWalletState();
}

void TokenUiBackend::syncWalletState()
{
    const WalletUiState& state = m_walletController->state();
    setIsWalletOpen(state.isWalletOpen);
    setWalletExists(state.walletExists);
    setWalletHome(state.walletHome);
    setSequencerAddr(state.sequencerAddress);
    setSequencerReachable(state.sequencerReachable);
}
