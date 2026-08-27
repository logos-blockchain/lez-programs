#ifndef AMM_UI_REGISTRY_LOADER_H
#define AMM_UI_REGISTRY_LOADER_H

#include <QObject>
#include <QVariantList>

// Loads the AMM app's "known tokens" and "known pools" and serves them as an
// in-memory snapshot that the backend's QtRO slots read synchronously.
//
// The source is resolved on refresh(): the local JSON files at TOKENS_CONFIG /
// AMM_POOLS_CONFIG (dev / local-sequencer testing). refresh() re-reads the
// source, bumps revision(), and emits changed() when the snapshot updates, so
// the backend can re-publish its registryRevision PROP and the UI can re-fetch.
//
// A later phase adds a remote GitHub registry (AMM_REGISTRY_URL) that refreshes
// asynchronously; this snapshot/consumer contract stays the same — only the
// body of refresh() changes.
class RegistryLoader : public QObject {
    Q_OBJECT

public:
    explicit RegistryLoader(QObject* parent = nullptr);

    // Current snapshot. Safe to call from the backend's QtRO slots.
    QVariantList tokens() const { return m_tokens; }
    QVariantList pools() const { return m_pools; }
    // Increments on every snapshot update — the value published as
    // AmmUiBackend::registryRevision so QML replicas re-fetch the lists.
    int revision() const { return m_revision; }

public slots:
    // (Re)load the configured source into the snapshot. Synchronous for the
    // local-file source; emits changed() when the snapshot has been refreshed.
    void refresh();

signals:
    void changed();

private:
    QVariantList m_tokens;
    QVariantList m_pools;
    int m_revision = 0;
};

#endif // AMM_UI_REGISTRY_LOADER_H
