#ifndef TOKEN_UI_PLUGIN_INTERFACE_H
#define TOKEN_UI_PLUGIN_INTERFACE_H

#include <QtPlugin>          // for Q_DECLARE_INTERFACE
#include "interface.h"

// Marker interface used by Qt's plugin loader to identify the Token UI plugin.
// The actual API surface (slots, properties, signals) lives in
// TokenUiBackend.rep — this header only carries the IID.
class TokenUiPluginInterface : public PluginInterface
{
public:
    virtual ~TokenUiPluginInterface() = default;
};

#define TokenUiPluginInterface_iid "org.logos.TokenUiPluginInterface"
Q_DECLARE_INTERFACE(TokenUiPluginInterface, TokenUiPluginInterface_iid)

#endif // TOKEN_UI_PLUGIN_INTERFACE_H
