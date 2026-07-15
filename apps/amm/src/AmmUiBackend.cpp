#include "AmmUiBackend.h"

#include "LogosWalletProvider.h"
#include "WalletController.h"
#include "logos_api.h"

AmmUiBackend::AmmUiBackend(LogosAPI* logosAPI, QObject* parent)
    : AmmUiBackendSimpleSource(parent),
      m_logosAPI(logosAPI ? logosAPI : new LogosAPI("amm_ui", this)),
      m_wallet(std::make_unique<LogosWalletProvider>(m_logosAPI)),
      m_walletController(std::make_unique<WalletController>(
          *m_wallet, QStringLiteral("AmmUI")))
{
    connect(m_walletController.get(), &WalletController::stateChanged,
            this, &AmmUiBackend::syncWalletState);
    syncWalletState();
    m_walletController->start();
}

AmmUiBackend::~AmmUiBackend() = default;

WalletAccountModel* AmmUiBackend::accountModel() const
{
    return m_walletController->accountModel();
}

QString AmmUiBackend::createAccountPublic()
{
    return m_walletController->createAccount(true);
}

QString AmmUiBackend::createAccountPrivate()
{
    return m_walletController->createAccount(false);
}

void AmmUiBackend::refreshAccounts()
{
    m_walletController->refresh();
}

void AmmUiBackend::refreshBalances()
{
    m_walletController->refresh();
}

QString AmmUiBackend::getBalance(QString accountIdHex, bool isPublic)
{
    return m_walletController->balance(accountIdHex, isPublic);
}

QString AmmUiBackend::createNewDefault(QString password)
{
    return m_walletController->createDefaultWallet(password);
}

QString AmmUiBackend::createNew(QString configPath,
                                QString storagePath,
                                QString password)
{
    return m_walletController->createWallet(configPath, storagePath, password);
}

bool AmmUiBackend::openExisting()
{
    return m_walletController->open();
}

void AmmUiBackend::disconnectWallet()
{
    m_walletController->disconnect();
}

void AmmUiBackend::syncWalletState()
{
    const WalletUiState& state = m_walletController->state();
    setIsWalletOpen(state.isWalletOpen);
    setWalletExists(state.walletExists);
    setConfigPath(state.configPath);
    setStoragePath(state.storagePath);
    setWalletHome(state.walletHome);
    setLastSyncedBlock(state.lastSyncedBlock);
    setCurrentBlockHeight(state.currentBlockHeight);
    setSequencerAddr(state.sequencerAddress);
    setSequencerReachable(state.sequencerReachable);
}
