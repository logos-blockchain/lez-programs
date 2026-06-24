#ifndef AMM_UI_PLUGIN_INTERFACE_H
#define AMM_UI_PLUGIN_INTERFACE_H

#include <QtPlugin>          // for Q_DECLARE_INTERFACE
#include "interface.h"

// Marker interface used by Qt's plugin loader to identify the AMM UI plugin.
// The actual API surface (slots, properties, signals) lives in
// AmmUiBackend.rep — this header only carries the IID.
class AmmUiPluginInterface : public PluginInterface
{
public:
    virtual ~AmmUiPluginInterface() = default;
};

#define AmmUiPluginInterface_iid "org.logos.AmmUiPluginInterface"
Q_DECLARE_INTERFACE(AmmUiPluginInterface, AmmUiPluginInterface_iid)

#endif // AMM_UI_PLUGIN_INTERFACE_H
