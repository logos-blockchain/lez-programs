#pragma once

#include <QString>
#include <QVariantMap>

#include <functional>

namespace SwapConfirmation {

struct Request {
    QString mode;
    QString input;
    QString output;
    QString inputHolding;
    QString outputHolding;
    QString amount;
    QString bound;
    QString deadline;
};

using Submit = std::function<QString(const Request&)>;

// Check the backend's current context immediately before dispatch. Request values
// remain strings so raw token amounts never pass through floating-point numbers.
QVariantMap submit(const QVariantMap& request, int currentRevision, bool walletOpen,
                   const Submit& dispatch);

} // namespace SwapConfirmation
