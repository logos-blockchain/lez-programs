#ifndef TOKEN_UI_PLUGIN_H
#define TOKEN_UI_PLUGIN_H

#include <QObject>
#include <QString>
#include <QtPlugin>          // for Q_PLUGIN_METADATA, Q_INTERFACES
#include "TokenUiPluginInterface.h"
#include "LogosViewPluginBase.h"

class LogosAPI;
class TokenUiBackend;

// Thin plugin entry point. Holds a TokenUiBackend and lets the generated
// view-plugin base expose it to ui-host.
class TokenUiPlugin : public QObject,
                      public TokenUiPluginInterface,
                      public TokenUiBackendViewPluginBase
{
    Q_OBJECT
    Q_PLUGIN_METADATA(IID TokenUiPluginInterface_iid FILE "../metadata.json")
    Q_INTERFACES(TokenUiPluginInterface)

public:
    explicit TokenUiPlugin(QObject* parent = nullptr);
    ~TokenUiPlugin() override;

    QString name()    const override { return "token_ui"; }
    QString version() const override { return "0.1.0"; }

    // Called by ui-host after plugin load. Creates the backend and wires it
    // up with the provided LogosAPI.
    Q_INVOKABLE void initLogos(LogosAPI* api);

private:
    TokenUiBackend* m_backend = nullptr;
};

#endif // TOKEN_UI_PLUGIN_H
