#include "AmmUiPlugin.h"
#include "AmmUiBackend.h"

#include <QDebug>

AmmUiPlugin::AmmUiPlugin(QObject* parent)
    : QObject(parent)
{
}

AmmUiPlugin::~AmmUiPlugin() = default;

void AmmUiPlugin::initLogos(LogosAPI* api)
{
    if (m_backend) return;
    m_backend = new AmmUiBackend(api, this);
    setBackend(m_backend);
    qDebug() << "AmmUiPlugin: backend initialized";
}
