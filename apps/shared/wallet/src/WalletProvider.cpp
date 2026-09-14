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
    case WalletFailure::CapabilityUnavailable:
        return QStringLiteral("capability_unavailable");
    }
    return QStringLiteral("wallet_unavailable");
}

bool balanceLess(QString left, QString right)
{
    while (left.size() > 1 && left.startsWith(QLatin1Char('0')))
        left.remove(0, 1);
    while (right.size() > 1 && right.startsWith(QLatin1Char('0')))
        right.remove(0, 1);
    return left.size() != right.size() ? left.size() < right.size() : left < right;
}

bool canPayFees(const WalletAccount& account)
{
    if (!account.isPublic || account.balance.isEmpty())
        return false;
    for (const QChar c : account.balance) {
        if (!c.isDigit())
            return false;
    }
    return !balanceLess(account.balance, QLatin1String(MIN_FEE_RESERVE));
}
