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
// Active network = AMM_NETWORK if it names a declared network, else the lone
// network when the registry declares exactly one. Network identity can't be
// detected from the connection (program ids and account ids are deterministic and
// can be identical across networks), so a multi-network registry needs AMM_NETWORK
// to disambiguate; otherwise nothing is applied. Selecting a network also exposes
// its AMM program id via activeAmmProgramId() so the backend can adopt it (no
// AMM_PROGRAM_BIN needed).
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
    // The active network's declared AMM program id (empty for local / none / a
    // network that declares none). The backend adopts it via setAmmProgramId so ops
    // target this network without an AMM_PROGRAM_BIN.
    QString activeAmmProgramId() const { return m_activeAmmProgramId; }

    // Whether a local-file source (TOKENS_CONFIG / AMM_POOLS_CONFIG) is configured —
    // it takes precedence over the remote registry (local-replaces-remote).
    static bool hasLocalSource();

    // The registry URL to fetch when AMM_REGISTRY_URL is unset — the value the user
    // configured in the wallet config UI (persisted by the backend). Empty ⇒ no
    // remote source. Takes effect on the next refresh().
    void setConfiguredUrl(const QString& url) { m_configuredUrl = url; }

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
    QString m_activeAmmProgramId;
    QString m_configuredUrl;  // UI-configured registry URL (env overrides)

    // Registry `timestamp` reflected in the snapshot — lets a revalidation skip
    // re-applying an unchanged document.
    QString m_stamp;
    // Guards against overlapping refreshes: a reply from an older refresh is
    // dropped once a newer refresh has started.
    quint64 m_generation = 0;

    QNetworkAccessManager* m_nam = nullptr;  // lazily created
};

#endif // AMM_UI_REGISTRY_LOADER_H
