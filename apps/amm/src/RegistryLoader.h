#ifndef AMM_UI_REGISTRY_LOADER_H
#define AMM_UI_REGISTRY_LOADER_H

#include <QObject>
#include <QString>
#include <QUrl>
#include <QVariantList>

class QNetworkAccessManager;
class QJsonArray;
class QJsonObject;

// Loads the AMM app's "known tokens" and "known pools" and serves them as an
// in-memory snapshot the backend's QtRO slots read synchronously.
//
// Source, resolved per refresh() (local-replaces-remote):
//   * If TOKENS_CONFIG / AMM_POOLS_CONFIG are set, the local JSON files (bare
//     `[...]` arrays, dev / local-sequencer testing) — parsed synchronously.
//   * Else if AMM_REGISTRY_URL is set, a single remote registry document
//     (Uniswap-token-list style, multi-network): `{ networks:[{id, programIds}],
//     tokens:[{network, ...}], pools:[{network, ...}] }`. Fetched asynchronously
//     (QNetworkAccessManager) with an on-disk cache served meanwhile
//     (stale-while-revalidate). Entries are filtered to the active network.
//
// Active network = AMM_NETWORK if set, else the registry network whose programIds
// match the deployment the app is connected to (see setConnectedProgramIds), else
// the lone network when the registry declares exactly one. A registry that names a
// network whose programIds contradict the app's deployment is rejected.
//
// refresh() bumps revision() and emits changed() whenever the snapshot updates,
// so the backend re-publishes registryRevision and the UI re-fetches.
class RegistryLoader : public QObject {
    Q_OBJECT

public:
    explicit RegistryLoader(QObject* parent = nullptr);

    QVariantList tokens() const { return m_tokens; }
    QVariantList pools() const { return m_pools; }
    int revision() const { return m_revision; }
    // Where the current snapshot came from: "local" | "remote" | "cache" | "none".
    QString source() const { return m_source; }
    // The network id the snapshot was filtered to (empty for local / none).
    QString activeNetwork() const { return m_activeNetwork; }

    // The deployment the app is connected to (base58 program ids, from
    // configAccount()): used to pick the matching network in a multi-network
    // registry and to reject a registry whose active network contradicts it.
    // Empty ⇒ selection falls back to AMM_NETWORK / a lone network.
    void setConnectedProgramIds(const QString& ammProgramId, const QString& tokenProgramId);

    // Whether a local-file source (TOKENS_CONFIG / AMM_POOLS_CONFIG) is
    // configured — it takes precedence over the remote registry. The backend uses
    // this to skip the sequencer-touching configAccount read for local dev.
    static bool hasLocalSource();

public slots:
    void refresh();

signals:
    void changed();

private:
    void loadLocal();
    void startRemote(const QUrl& url);
    // Parse the registry body, select + guard the active network, filter, and
    // publish. Returns true when a snapshot was applied.
    bool applyRegistry(const QByteArray& body, const QString& source);
    QString selectActiveNetwork(const QJsonArray& networks) const;
    bool deploymentOk(const QJsonObject& network) const;

    void publish(const QVariantList& tokens, const QVariantList& pools,
                 const QString& source, const QString& network);

    void loadDiskCache(const QString& url);
    void saveDiskCache(const QString& url, const QString& stamp, const QByteArray& body) const;
    static QString cachePath();

    QNetworkAccessManager* nam();

    QVariantList m_tokens;
    QVariantList m_pools;
    int m_revision = 0;
    QString m_source = QStringLiteral("none");
    QString m_activeNetwork;

    QString m_connectedAmm;
    QString m_connectedToken;

    // Registry `timestamp` reflected in the snapshot — lets a revalidation skip
    // re-applying an unchanged document.
    QString m_stamp;
    // Guards against overlapping refreshes: a reply from an older refresh is
    // dropped once a newer refresh has started.
    quint64 m_generation = 0;

    QNetworkAccessManager* m_nam = nullptr;  // lazily created
};

#endif // AMM_UI_REGISTRY_LOADER_H
