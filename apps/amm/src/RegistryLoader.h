#ifndef AMM_UI_REGISTRY_LOADER_H
#define AMM_UI_REGISTRY_LOADER_H

#include <QObject>
#include <QString>
#include <QUrl>
#include <QVariantList>

class QNetworkAccessManager;
class QNetworkReply;

// Loads the AMM app's "known tokens" and "known pools" and serves them as an
// in-memory snapshot the backend's QtRO slots read synchronously.
//
// Source, resolved per refresh() (local-replaces-remote):
//   * If TOKENS_CONFIG / AMM_POOLS_CONFIG are set, the local JSON files (dev /
//     local-sequencer testing) — parsed synchronously.
//   * Else if AMM_REGISTRY_URL is set, a remote GitHub registry: a manifest
//     (registry.json) naming the tokens/pools files and the deployment the list
//     targets. Fetched asynchronously (QNetworkAccessManager); an on-disk cache
//     is served meanwhile (stale-while-revalidate), and a manifest whose
//     programIds don't match this app's deployment is rejected.
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

    // The program ids the app is connected to (base58, from configAccount()),
    // used to reject a remote manifest built for a different deployment. Empty
    // ⇒ the guard is skipped (permissive).
    void setExpectedProgramIds(const QString& ammProgramId, const QString& tokenProgramId);

    // Whether a local-file source (TOKENS_CONFIG / AMM_POOLS_CONFIG) is
    // configured — it takes precedence over the remote registry. The backend
    // uses this to skip the (sequencer-touching) deployment-guard read when the
    // remote source won't be used anyway.
    static bool hasLocalSource();

public slots:
    void refresh();

signals:
    void changed();

private:
    void loadLocal();

    void startRemote(const QUrl& manifestUrl);
    void fetchLists(const QUrl& tokensUrl, const QUrl& poolsUrl, const QString& stamp,
                    quint64 generation);
    // manifest deployment guard: true ⇒ ok to apply the remote list.
    bool deploymentMatches(const class QJsonObject& manifest) const;

    void publish(const QVariantList& tokens, const QVariantList& pools, const QString& source);

    void loadDiskCache(const QString& url);
    void saveDiskCache(const QString& url, const QString& stamp) const;
    static QString cachePath();

    QNetworkAccessManager* nam();

    QVariantList m_tokens;
    QVariantList m_pools;
    int m_revision = 0;
    QString m_source = QStringLiteral("none");

    QString m_expectedAmmProgramId;
    QString m_expectedTokenProgramId;

    // Manifest freshness stamp (its `timestamp`) currently reflected in the
    // snapshot — lets a revalidation skip re-downloading unchanged lists.
    QString m_stamp;
    // Guards against overlapping refreshes: a reply from an older refresh is
    // dropped once a newer refresh has started.
    quint64 m_generation = 0;

    QNetworkAccessManager* m_nam = nullptr;  // lazily created
};

#endif // AMM_UI_REGISTRY_LOADER_H
