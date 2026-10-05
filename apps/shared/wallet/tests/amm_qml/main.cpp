#include <QMetaProperty>
#include <QQmlEngine>
#include <QtQuickTest/quicktest.h>

#include <utility>

#include "rep_AmmUiBackend_replica.h"

// Keep the properties and change signals identical to the production replica.
// Only the wallet operation and delivery of remote property updates are faked.
class AmmWalletReplica : public AmmUiBackendReplica
{
    Q_OBJECT

public:
    explicit AmmWalletReplica(QObject* parent = nullptr)
    {
        setParent(parent);
        deliverProperty("walletExists", true);
        deliverProperty("syncStatus", QStringLiteral("closed"));
    }

    Q_INVOKABLE bool openExisting()
    {
        deliverProperty("syncStatus", QStringLiteral("opening"));
        return true;
    }

    Q_INVOKABLE void completeOpen(bool success, const QString& error = {})
    {
        deliverProperty("syncError", error);
        deliverProperty("isWalletOpen", success);
        deliverProperty("syncStatus", success ? QStringLiteral("ready")
                                               : QStringLiteral("error"));
    }

private:
    void deliverProperty(const char* name, const QVariant& value)
    {
        const QMetaObject& contract = AmmUiBackendReplica::staticMetaObject;
        const int propertyIndex = contract.indexOfProperty(name);
        if (propertyIndex < contract.propertyOffset())
            return;

        QVariantList values;
        for (int i = contract.propertyOffset(); i < contract.propertyCount(); ++i)
            values.append(i == propertyIndex ? value : propAsVariant(i - contract.propertyOffset()));
        setProperties(std::move(values));

        const QMetaProperty property = contract.property(propertyIndex);
        property.notifySignal().invoke(this, QGenericArgument(property.typeName(), value.constData()));
    }
};

class AmmWalletTestSetup : public QObject
{
    Q_OBJECT

public slots:
    void applicationAvailable()
    {
        qmlRegisterType<AmmWalletReplica>("WalletContractTest", 1, 0, "AmmWalletReplica");
    }
};

QUICK_TEST_MAIN_WITH_SETUP(amm_wallet_qml, AmmWalletTestSetup)

#include "main.moc"
