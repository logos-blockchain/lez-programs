#ifndef AMM_UI_PLUGIN_H
#define AMM_UI_PLUGIN_H

#include <QObject>
#include <QString>
#include <QtPlugin>          // for Q_PLUGIN_METADATA, Q_INTERFACES
#include "AmmUiPluginInterface.h"
#include "LogosViewPluginBase.h"

class LogosAPI;
class AmmUiBackend;

// Thin plugin entry point. Holds an AmmUiBackend and lets the generated
// view-plugin base expose it to ui-host.
class AmmUiPlugin : public QObject,
                    public AmmUiPluginInterface,
                    public AmmUiBackendViewPluginBase
{
    Q_OBJECT
    Q_PLUGIN_METADATA(IID AmmUiPluginInterface_iid FILE "../metadata.json")
    Q_INTERFACES(AmmUiPluginInterface)

public:
    explicit AmmUiPlugin(QObject* parent = nullptr);
    ~AmmUiPlugin() override;

    QString name()    const override { return "amm_ui"; }
    QString version() const override { return "0.1.0"; }

    // Called by ui-host after plugin load. Creates the backend and wires it
    // up with the provided LogosAPI.
    Q_INVOKABLE void initLogos(LogosAPI* api);

private:
    AmmUiBackend* m_backend = nullptr;
};

#endif // AMM_UI_PLUGIN_H
