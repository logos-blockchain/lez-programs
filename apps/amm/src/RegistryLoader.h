#ifndef AMM_UI_REGISTRY_LOADER_H
#define AMM_UI_REGISTRY_LOADER_H

#include <QObject>
#include <QString>
#include <QUrl>
#include <QVariantList>

#include <QJsonObject>

class QNetworkAccessManager;
class QJsonArray;

// Loads the AMM app's "known tokens" and "known pools" and serves them as an
// in-memory snapshot the backend's QtRO slots read synchronously.
//
// Source, resolved per refresh() (local-replaces-remote):
//   * Either nonempty TOKENS_CONFIG / AMM_POOLS_CONFIG selects local files for
//     both lists (bare `[...]` arrays), parsed synchronously without filtering.
//     Each missing/unreadable/invalid file yields an empty list; no remote fallback.
//   * Else nonempty AMM_REGISTRY_URL overrides setConfiguredUrl(). A selected URL
//     supplies a multi-network document: `{ networks:[{id, programIds}],
//     tokens:[{network, ...}], pools:[{network, ...}] }`, fetched asynchronously.
//     Only when both current lists are empty, try the disk cache first; its stored
//     URL must match the requested URL. Otherwise retain the current snapshot
//     while fetching, even when the URL changes. Network errors and non-object
//     JSON retain it; an object without a usable network publishes empty lists.
//     Registry/cache entries are filtered to the active network.
//   * No local source or URL publishes empty lists with source "none".
//
// Source labels are "local", "remote", "cache" and "none". Cache is local disk
// storage of registry content, not an authenticity guarantee. The loader checks
// no registry signature or on-chain identities for list metadata; token names,
// symbols and pool labels remain source-provided.
//
// Active network = the user's selection (selectNetwork), else AMM_NETWORK if it
// names a declared network, else the first declared network. Network identity can't
// be detected from the connection (program ids and account ids are deterministic and
// can be identical across networks), so the user picks; networks() lists them for
// the picker. selectNetwork() re-filters the last-loaded registry with no re-fetch.
// The active network's AMM program id is exposed via activeAmmProgramId() so the
// backend can adopt it (no AMM_PROGRAM_BIN needed).
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
    // The registry's declared networks as [{ id, name }] for the picker (empty for
    // local / none). The active one is activeNetwork().
    QVariantList networks() const;
    // The active network's declared AMM program id (empty for local / none / a
    // network that declares none). The backend adopts it via setAmmProgramId so ops
    // target this network without an AMM_PROGRAM_BIN.
    QString activeAmmProgramId() const { return m_activeAmmProgramId; }

    // The active network's AMM instance, identified by the account id of its config
    // PDA (registry field `ammConfigId`). The backend hands it to the module
    // (setConfigId) so ops target that instance. Empty when the network omits it.
    QString activeAmmConfigId() const { return m_activeAmmConfigId; }

    // Whether a local-file source (TOKENS_CONFIG / AMM_POOLS_CONFIG) is configured —
    // it takes precedence over the remote registry (local-replaces-remote).
    static bool hasLocalSource();

    // The registry URL to fetch when AMM_REGISTRY_URL is empty — the value the user
    // configured in the wallet config UI (persisted by the backend). Empty ⇒ no
    // remote source unless the env overrides. Local files win over either URL.
    // Takes effect on the next refresh().
    void setConfiguredUrl(const QString& url) { m_configuredUrl = url; }

public slots:
    void refresh();
    // Pick a network by id (from networks()). Re-filters the last-loaded registry
    // and re-adopts its program id with no re-fetch; ignored if no registry is
    // loaded yet (the pick is remembered and applied when one loads).
    void selectNetwork(const QString& id);

signals:
    void changed();

private:
    void loadLocal();
    void startRemote(const QUrl& url);
    // Parse the registry body into m_registryObj, then applySelection(). Returns
    // true when a snapshot was applied.
    bool applyRegistry(const QByteArray& body, const QString& source);
    // Select the active network from the stored registry, filter its tokens/pools,
    // adopt its program id, and publish. Returns true when a network was applied.
    bool applySelection();
    QString selectActiveNetwork(const QJsonArray& networks) const;

    void publish(const QVariantList& tokens, const QVariantList& pools,
                 const QString& source, const QString& network);

    void loadDiskCache(const QString& url);
    void saveDiskCache(const QString& url, const QByteArray& body) const;
    static QString cachePath();

    QNetworkAccessManager* nam();

    QVariantList m_tokens;
    QVariantList m_pools;
    int m_revision = 0;
    QString m_source = QStringLiteral("none");
    QString m_activeNetwork;
    QString m_activeAmmProgramId;
    QString m_activeAmmConfigId;
    QString m_configuredUrl;  // UI-configured registry URL (env overrides)

    // The last-loaded registry document, kept so selectNetwork() can re-filter to a
    // different network without re-fetching. Empty for local / none sources.
    QJsonObject m_registryObj;
    QString m_lastSource;       // source label of m_registryObj ("remote"/"cache")
    QString m_selectedNetwork;  // the user's picked network id (empty ⇒ default)

    // Guards against overlapping refreshes: a reply from an older refresh is
    // dropped once a newer refresh has started.
    quint64 m_generation = 0;

    QNetworkAccessManager* m_nam = nullptr;  // lazily created
};

#endif // AMM_UI_REGISTRY_LOADER_H
