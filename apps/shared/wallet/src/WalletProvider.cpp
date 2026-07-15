#include "WalletProvider.h"

QString walletFailureCode(WalletFailure failure)
{
    switch (failure) {
    case WalletFailure::None:
        return {};
    case WalletFailure::WalletMissing:
        return QStringLiteral("wallet_missing");
    case WalletFailure::WalletUnavailable:
        return QStringLiteral("wallet_unavailable");
    case WalletFailure::OpenFailed:
        return QStringLiteral("open_failed");
    case WalletFailure::CreateFailed:
        return QStringLiteral("create_failed");
    case WalletFailure::SaveFailed:
        return QStringLiteral("save_failed");
    case WalletFailure::ReadFailed:
        return QStringLiteral("read_failed");
    case WalletFailure::InvalidRequest:
        return QStringLiteral("invalid_request");
    case WalletFailure::SubmissionFailed:
        return QStringLiteral("submission_failed");
    }
    return QStringLiteral("wallet_unavailable");
}
