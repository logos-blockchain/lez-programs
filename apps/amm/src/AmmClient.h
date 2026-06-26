#pragma once

#include <QJsonObject>

struct AmmClientResult {
    bool ok = false;
    QJsonObject value;
};

class AmmClient {
public:
    virtual ~AmmClient() = default;

    virtual AmmClientResult configId(const QJsonObject& request) const = 0;
    virtual AmmClientResult tokenIds(const QJsonObject& request) const = 0;
    virtual AmmClientResult pairIds(const QJsonObject& request) const = 0;
    virtual AmmClientResult context(const QJsonObject& request) const = 0;
    virtual AmmClientResult quote(const QJsonObject& request) const = 0;
    virtual AmmClientResult plan(const QJsonObject& request) const = 0;
};

class BundledAmmClient final : public AmmClient {
public:
    AmmClientResult configId(const QJsonObject& request) const override;
    AmmClientResult tokenIds(const QJsonObject& request) const override;
    AmmClientResult pairIds(const QJsonObject& request) const override;
    AmmClientResult context(const QJsonObject& request) const override;
    AmmClientResult quote(const QJsonObject& request) const override;
    AmmClientResult plan(const QJsonObject& request) const override;
};
