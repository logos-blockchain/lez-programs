#include "TokenUiPlugin.h"
#include "TokenUiBackend.h"

#include <QDebug>

TokenUiPlugin::TokenUiPlugin(QObject* parent)
    : QObject(parent)
{
}

TokenUiPlugin::~TokenUiPlugin() = default;

void TokenUiPlugin::initLogos(LogosAPI* api)
{
    if (m_backend) return;
    m_backend = new TokenUiBackend(api, this);
    setBackend(m_backend);
    qDebug() << "TokenUiPlugin: backend initialized";
}
