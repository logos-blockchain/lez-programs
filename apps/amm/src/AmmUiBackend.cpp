#include "AmmUiBackend.h"

#include <cmath>
#include <initializer_list>
#include <utility>

#include <QByteArray>
#include <QCoreApplication>
#include <QDebug>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QJsonArray>
#include <QJsonDocument>
#include <QJsonObject>
#include <QJsonParseError>
#include <QJsonValue>
#include <QMetaType>
#include <QNetworkAccessManager>
#include <QNetworkReply>
#include <QNetworkRequest>
#include <QSaveFile>
#include <QSettings>
#include <QStringList>
#include <QTimer>
#include <QUrl>
#include <QVariantList>
#include <QVariantMap>

#ifdef Q_OS_UNIX
#include <dlfcn.h>
#endif

#include "logos_api.h"
#include "logos_sdk.h"

namespace {
    const char SETTINGS_ORG[] = "Logos";
    const char SETTINGS_APP[] = "AmmUI";
    // Sticky "user pressed Disconnect" flag so the wallet stays locked across
    // relaunches until the user reconnects.
    const char DISCONNECTED_KEY[] = "disconnected";
    const int WALLET_FFI_SUCCESS = 0;

    // Wallet home env override. Prefer the wallet CLI var, but keep the LEZ UI
    // var as a compatibility fallback.
    const char WALLET_HOME_ENV[] = "NSSA_WALLET_HOME_DIR";
    const char LEGACY_WALLET_HOME_ENV[] = "LEE_WALLET_HOME_DIR";
    const char DEPLOYMENT_CONFIG_DIR_ENV[] = "AMM_UI_CONFIG_DIR";
    const char DEPLOYMENT_PROGRAM_DIR_ENV[] = "AMM_UI_PROGRAM_DIR";
    const char DEFAULT_SEQUENCER[] = "https://testnet.lez.logos.co";
    const char LEGACY_AMM_ABI[] = "legacy-v0.2.0-rc3";
    const char ACCOUNT_ID_KEY[] = "account_id";
    const char DISPLAY_ACCOUNT_ID_KEY[] = "display_account_id";
    const double MAX_SAFE_QML_INTEGER = 9007199254740991.0;
    const int CHAIN_IDENTITY_BLOCK = 1;
    const int BLOCK_HASH_OFFSET = 40;
    const int BLOCK_HASH_SIZE = 32;
    const int BLOCK_SIGNATURE_OFFSET = 80;
    const int BLOCK_SIGNATURE_SIZE = 64;

    struct ChainFingerprint {
        QString blockHash;
        QString blockSignature;

        bool isValid() const
        {
            return !blockHash.isEmpty() && !blockSignature.isEmpty();
        }
    };

    QString txResultJson(bool success, const QString& txHash, const QString& error)
    {
        QJsonObject object;
        object.insert(QStringLiteral("success"), success);
        object.insert(QStringLiteral("tx_hash"), txHash);
        object.insert(QStringLiteral("error"), error);
        return QString::fromUtf8(QJsonDocument(object).toJson(QJsonDocument::Compact));
    }

    QString txError(const QString& error)
    {
        return txResultJson(false, {}, error);
    }

    // Normalise file:// URLs to a plain local path; leave other inputs intact so
    // invalid URLs don't silently collapse to an empty path.
    QString toLocalPath(const QString& path)
    {
        const QString trimmed = path.trimmed();
        const QUrl url(trimmed);
        if (url.scheme().compare(QStringLiteral("file"), Qt::CaseInsensitive) == 0) {
            const QString local = url.toLocalFile();
            return local.isEmpty() ? trimmed : local;
        }
        return trimmed;
    }

    QString pluginDirPath()
    {
#ifdef Q_OS_UNIX
        Dl_info info;
        if (dladdr(reinterpret_cast<void*>(&pluginDirPath), &info) != 0 && info.dli_fname != nullptr)
            return QFileInfo(QString::fromLocal8Bit(info.dli_fname)).absolutePath();
#endif
        return QCoreApplication::applicationDirPath();
    }

    QString assetDirPath()
    {
#ifdef AMM_UI_ASSET_DIR
        return QStringLiteral(AMM_UI_ASSET_DIR);
#else
        return {};
#endif
    }

    QJsonObject loadConfigObject(const QString& fileName)
    {
        QStringList candidates;
        const QString envConfigDir = QString::fromLocal8Bit(qgetenv(DEPLOYMENT_CONFIG_DIR_ENV));
        if (!envConfigDir.isEmpty())
            candidates.append(QDir(envConfigDir).filePath(fileName));
        const QString assetDir = assetDirPath();
        if (!assetDir.isEmpty())
            candidates.append(QDir(assetDir).filePath(QStringLiteral("config/") + fileName));
        candidates.append({
            QDir(pluginDirPath()).filePath(QStringLiteral("config/") + fileName),
            QDir(QCoreApplication::applicationDirPath()).filePath(QStringLiteral("config/") + fileName),
            QDir(QCoreApplication::applicationDirPath()).filePath(QStringLiteral("../lib/config/") + fileName),
            QDir(QCoreApplication::applicationDirPath()).filePath(QStringLiteral("../lib64/config/") + fileName),
        });

        for (const QString& path : candidates) {
            QFile file(path);
            if (!file.exists())
                continue;
            if (!file.open(QIODevice::ReadOnly)) {
                qWarning() << "AmmUiBackend: cannot open deployment config" << path;
                continue;
            }

            QJsonParseError error;
            const QJsonDocument doc = QJsonDocument::fromJson(file.readAll(), &error);
            if (error.error != QJsonParseError::NoError || !doc.isObject()) {
                qWarning() << "AmmUiBackend: invalid deployment config" << path << error.errorString();
                continue;
            }

            return doc.object();
        }

        qWarning() << "AmmUiBackend: deployment config not found" << fileName;
        return {};
    }

    QByteArray loadProgramBinary(const QString& fileName, QString* error)
    {
        QStringList candidates;
        const QString envProgramDir = QString::fromLocal8Bit(qgetenv(DEPLOYMENT_PROGRAM_DIR_ENV));
        if (!envProgramDir.isEmpty())
            candidates.append(QDir(envProgramDir).filePath(fileName));
        const QString assetDir = assetDirPath();
        if (!assetDir.isEmpty())
            candidates.append(QDir(assetDir).filePath(QStringLiteral("programs/") + fileName));
        candidates.append({
            QDir(pluginDirPath()).filePath(QStringLiteral("programs/") + fileName),
            QDir(QCoreApplication::applicationDirPath()).filePath(QStringLiteral("programs/") + fileName),
            QDir(QCoreApplication::applicationDirPath()).filePath(QStringLiteral("../lib/programs/") + fileName),
            QDir(QCoreApplication::applicationDirPath()).filePath(QStringLiteral("../lib64/programs/") + fileName),
        });

        for (const QString& path : candidates) {
            QFile file(path);
            if (!file.exists())
                continue;
            if (!file.open(QIODevice::ReadOnly)) {
                qWarning() << "AmmUiBackend: cannot open program binary" << path;
                continue;
            }

            const QByteArray bytes = file.readAll();
            if (!bytes.isEmpty())
                return bytes;
        }

        if (error)
            *error = QStringLiteral("Program binary not found: %1").arg(fileName);
        return {};
    }

    QJsonObject objectAt(const QJsonArray& array, int index)
    {
        if (index < 0 || index >= array.size())
            return {};
        return array.at(index).toObject();
    }

    QJsonObject firstObject(const QJsonObject& object, const QString& arrayKey)
    {
        return objectAt(object.value(arrayKey).toArray(), 0);
    }

    QString stringValue(const QJsonObject& object, const QString& key, const QString& fallback = {})
    {
        const QString value = object.value(key).toString().trimmed();
        return value.isEmpty() ? fallback : value;
    }

    QJsonObject tokenDefinition(const QJsonArray& definitions, const QString& symbol, int fallbackIndex)
    {
        for (const QJsonValue& value : definitions) {
            const QJsonObject definition = value.toObject();
            if (stringValue(definition, QStringLiteral("symbol")) == symbol)
                return definition;
        }
        return objectAt(definitions, fallbackIndex);
    }

    QString normalizedUrl(QString url)
    {
        url = url.trimmed();
        while (url.endsWith(QLatin1Char('/')))
            url.chop(1);
        return url;
    }

    QString canonicalHex(QString value)
    {
        value = value.trimmed().toLower();
        if (value.startsWith(QStringLiteral("0x")))
            value.remove(0, 2);
        return value;
    }

    void appendUnique(QStringList* values, const QString& value)
    {
        const QString normalized = canonicalHex(value);
        if (!normalized.isEmpty() && !values->contains(normalized))
            values->append(normalized);
    }

    QStringList deploymentTransactionHashes(const QJsonObject& program)
    {
        QStringList hashes;
        appendUnique(&hashes, stringValue(program, QStringLiteral("deploymentTransaction")));
        appendUnique(&hashes, stringValue(program, QStringLiteral("deploymentTx")));
        appendUnique(&hashes, stringValue(program, QStringLiteral("transaction")));

        const QJsonArray array = program.value(QStringLiteral("deploymentTransactions")).toArray();
        for (const QJsonValue& value : array)
            appendUnique(&hashes, value.toString());

        return hashes;
    }

    QStringList poolCreationTransactionHashes(const QJsonObject& pool)
    {
        QStringList hashes;
        appendUnique(&hashes, stringValue(pool, QStringLiteral("creationTransaction")));
        appendUnique(&hashes, stringValue(pool, QStringLiteral("creationTx")));
        appendUnique(&hashes, stringValue(pool, QStringLiteral("transaction")));

        const QJsonArray array = pool.value(QStringLiteral("creationTransactions")).toArray();
        for (const QJsonValue& value : array)
            appendUnique(&hashes, value.toString());

        return hashes;
    }

    void appendDeploymentTransactions(const QJsonObject& chain, QStringList* hashes)
    {
        const QJsonArray programs = chain.value(QStringLiteral("programs")).toArray();
        for (const QJsonValue& value : programs) {
            const QJsonObject program = value.toObject();
            for (const QString& hash : deploymentTransactionHashes(program))
                appendUnique(hashes, hash);
            const QJsonArray pools = program.value(QStringLiteral("pools")).toArray();
            for (const QJsonValue& poolValue : pools) {
                for (const QString& hash : poolCreationTransactionHashes(poolValue.toObject()))
                    appendUnique(hashes, hash);
            }
        }
    }

    QString hexSlice(const QByteArray& bytes, int offset, int size)
    {
        if (bytes.size() < offset + size)
            return {};
        return QString::fromLatin1(bytes.mid(offset, size).toHex());
    }

    QByteArray jsonRpcBody(const QString& method, const QJsonArray& params)
    {
        QJsonObject body;
        body.insert(QStringLiteral("jsonrpc"), QStringLiteral("2.0"));
        body.insert(QStringLiteral("id"), 1);
        body.insert(QStringLiteral("method"), method);
        body.insert(QStringLiteral("params"), params);
        return QJsonDocument(body).toJson(QJsonDocument::Compact);
    }

    ChainFingerprint chainFingerprintFromGetBlockResponse(const QByteArray& payload)
    {
        QJsonParseError error;
        const QJsonDocument doc = QJsonDocument::fromJson(payload, &error);
        if (error.error != QJsonParseError::NoError || !doc.isObject())
            return {};

        const QString block = doc.object().value(QStringLiteral("result")).toString();
        if (block.isEmpty())
            return {};

        const QByteArray raw = QByteArray::fromBase64(block.toLatin1());
        return {
            hexSlice(raw, BLOCK_HASH_OFFSET, BLOCK_HASH_SIZE),
            hexSlice(raw, BLOCK_SIGNATURE_OFFSET, BLOCK_SIGNATURE_SIZE),
        };
    }

    enum class TransactionLookupStatus {
        Found,
        Missing,
        TransientFailure,
    };

    TransactionLookupStatus transactionLookupStatus(const QByteArray& payload)
    {
        QJsonParseError error;
        const QJsonDocument doc = QJsonDocument::fromJson(payload, &error);
        if (error.error != QJsonParseError::NoError || !doc.isObject())
            return TransactionLookupStatus::TransientFailure;
        const QJsonObject object = doc.object();
        if (object.contains(QStringLiteral("error")))
            return TransactionLookupStatus::TransientFailure;
        const QJsonValue result = object.value(QStringLiteral("result"));
        return !result.isNull() && !result.isUndefined()
            ? TransactionLookupStatus::Found
            : TransactionLookupStatus::Missing;
    }

    QJsonObject supportedChainByRef(const QJsonArray& supportedChains, const QString& chainRef)
    {
        const QString expected = chainRef.trimmed();
        for (const QJsonValue& value : supportedChains) {
            const QJsonObject chain = value.toObject();
            if (stringValue(chain, QStringLiteral("alias")) == expected)
                return chain;
        }

        return {};
    }

    QJsonObject resolveSupportedChain(const QJsonObject& chain, const QJsonArray& supportedChains)
    {
        const QString chainRef = stringValue(chain, QStringLiteral("chainRef"));
        if (chainRef.isEmpty())
            return chain;

        QJsonObject resolved = supportedChainByRef(supportedChains, chainRef);
        if (resolved.isEmpty()) {
            qWarning() << "AmmUiBackend: unsupported chainRef in deployment config" << chainRef;
            return {};
        }

        for (auto it = chain.begin(); it != chain.end(); ++it) {
            if (it.key() != QStringLiteral("chainRef"))
                resolved.insert(it.key(), it.value());
        }

        return resolved;
    }

    QJsonArray deploymentChains(const QJsonObject& root,
                                const QString& fallbackNetwork,
                                const QJsonArray& supportedChains)
    {
        const QJsonArray chains = root.value(QStringLiteral("chains")).toArray();
        if (!chains.isEmpty()) {
            QJsonArray result;
            for (const QJsonValue& value : chains) {
                const QJsonObject chain =
                    resolveSupportedChain(value.toObject(), supportedChains);
                if (!chain.isEmpty())
                    result.append(chain);
            }
            return result;
        }

        const QJsonArray programs = root.value(QStringLiteral("programs")).toArray();
        if (programs.isEmpty())
            return {};

        QJsonObject chain;
        const QString chainRef = stringValue(root, QStringLiteral("chainRef"));
        const QString network = stringValue(
            root, QStringLiteral("network"), chainRef.isEmpty() ? fallbackNetwork : QString{});
        if (!network.isEmpty())
            chain.insert(QStringLiteral("network"), network);
        chain.insert(QStringLiteral("programs"), programs);
        if (!chainRef.isEmpty())
            chain.insert(QStringLiteral("chainRef"), chainRef);

        QJsonArray result;
        const QJsonObject resolved = resolveSupportedChain(chain, supportedChains);
        if (!resolved.isEmpty())
            result.append(resolved);
        return result;
    }

    QString chainNetwork(const QJsonObject& chain)
    {
        const QString network = normalizedUrl(stringValue(chain, QStringLiteral("network")));
        if (!network.isEmpty())
            return network;

        const QJsonArray sequencers = chain.value(QStringLiteral("sequencers")).toArray();
        for (const QJsonValue& value : sequencers) {
            const QString sequencer = normalizedUrl(value.toString());
            if (!sequencer.isEmpty())
                return sequencer;
        }

        return {};
    }

    bool chainMatchesNetwork(const QJsonObject& chain, const QString& network)
    {
        const QString expected = normalizedUrl(network);
        if (chainNetwork(chain) == expected)
            return true;

        for (const QString& key : { QStringLiteral("networks"), QStringLiteral("sequencers") }) {
            const QJsonArray values = chain.value(key).toArray();
            for (const QJsonValue& value : values) {
                if (normalizedUrl(value.toString()) == expected)
                    return true;
            }
        }

        return false;
    }

    QString configuredChainFingerprint(const QJsonObject& chain)
    {
        const QString explicitFingerprint =
            stringValue(chain, QStringLiteral("chainFingerprint")).toLower();
        if (!explicitFingerprint.isEmpty())
            return explicitFingerprint;

        const QString hash = canonicalHex(stringValue(chain, QStringLiteral("genesisBlockHash")));
        const QString signature =
            canonicalHex(stringValue(chain, QStringLiteral("genesisBlockSignature")));
        if (!hash.isEmpty() && !signature.isEmpty())
            return hash + QStringLiteral(":") + signature;

        return {};
    }

    bool chainHasDeterministicIdentity(const QJsonObject& chain)
    {
        return !configuredChainFingerprint(chain).isEmpty()
               || !stringValue(chain, QStringLiteral("genesisBlockHash")).isEmpty()
               || !stringValue(chain, QStringLiteral("genesisBlockSignature")).isEmpty();
    }

    bool chainsHaveDeterministicIdentity(const QJsonArray& chains)
    {
        for (const QJsonValue& value : chains) {
            if (chainHasDeterministicIdentity(value.toObject()))
                return true;
        }
        return false;
    }

    bool chainMatchesFingerprint(const QJsonObject& chain,
                                 const QString& blockHash,
                                 const QString& blockSignature)
    {
        const QString observedHash = canonicalHex(blockHash);
        const QString observedSignature = canonicalHex(blockSignature);
        const QString fingerprint = configuredChainFingerprint(chain);
        if (!fingerprint.isEmpty())
            return fingerprint == observedHash + QStringLiteral(":") + observedSignature;

        const QString expectedHash =
            canonicalHex(stringValue(chain, QStringLiteral("genesisBlockHash")));
        if (!expectedHash.isEmpty() && expectedHash != observedHash)
            return false;

        const QString expectedSignature =
            canonicalHex(stringValue(chain, QStringLiteral("genesisBlockSignature")));
        if (!expectedSignature.isEmpty() && expectedSignature != observedSignature)
            return false;

        return chainHasDeterministicIdentity(chain);
    }

    QJsonObject chainForNetwork(const QJsonArray& chains, const QString& network)
    {
        const QString requested = normalizedUrl(network);
        if (requested.isEmpty()) {
            for (const QJsonValue& value : chains) {
                const QJsonObject chain = value.toObject();
                if (chain.value(QStringLiteral("default")).toBool())
                    return chain;
            }
            return chains.isEmpty() ? QJsonObject{} : chains.at(0).toObject();
        }

        for (const QJsonValue& value : chains) {
            const QJsonObject chain = value.toObject();
            if (chainMatchesNetwork(chain, requested))
                return chain;
        }

        return {};
    }

    QJsonObject chainForFingerprint(const QJsonArray& chains,
                                    const QString& network,
                                    const QString& blockHash,
                                    const QString& blockSignature)
    {
        const QString requested = normalizedUrl(network);
        if (!blockHash.isEmpty() || !blockSignature.isEmpty()) {
            for (const QJsonValue& value : chains) {
                const QJsonObject chain = value.toObject();
                if (chainMatchesFingerprint(chain, blockHash, blockSignature))
                    return chain;
            }
            return {};
        }

        // Legacy configs had only URLs. Once a config declares deterministic
        // chain identity, never silently trust the endpoint URL alone.
        if (!requested.isEmpty() && chainsHaveDeterministicIdentity(chains))
            return {};

        return chainForNetwork(chains, requested);
    }

    double numberValue(const QJsonObject& object, const QString& key, double fallback = 0)
    {
        const QJsonValue value = object.value(key);
        return value.isDouble() ? value.toDouble() : fallback;
    }

    double u128LeToDouble(const QByteArray& bytes)
    {
        long double value = 0;
        long double multiplier = 1;
        for (int i = 0; i < 16; ++i) {
            value += static_cast<unsigned char>(bytes.at(i)) * multiplier;
            multiplier *= 256;
            if (value > MAX_SAFE_QML_INTEGER)
                return MAX_SAFE_QML_INTEGER;
        }
        return static_cast<double>(value);
    }

    struct DecodedPoolDefinition {
        QString definitionTokenAIdHex;
        QString definitionTokenBIdHex;
        QString vaultAIdHex;
        QString vaultBIdHex;
        QString liquidityPoolIdHex;
        double liquidityPoolSupply = 0;
        double reserveA = 0;
        double reserveB = 0;
        double fees = 0;
        bool active = true;
    };

    bool decodePoolDefinitionData(const QString& dataHex, DecodedPoolDefinition& pool)
    {
        const QString trimmed = dataHex.trimmed();
        // PoolDefinition Borsh struct:
        // 5 AccountId fields + liquidity_pool_supply/reserve_a/reserve_b/fees as u128.
        // The deployed legacy AMM also has a trailing active bool.
        if (trimmed.size() != 448 && trimmed.size() != 450)
            return false;
        const QByteArray data = QByteArray::fromHex(trimmed.toLatin1());
        if (data.size() != 224 && data.size() != 225)
            return false;

        pool.definitionTokenAIdHex = QString::fromLatin1(data.mid(0, 32).toHex());
        pool.definitionTokenBIdHex = QString::fromLatin1(data.mid(32, 32).toHex());
        pool.vaultAIdHex = QString::fromLatin1(data.mid(64, 32).toHex());
        pool.vaultBIdHex = QString::fromLatin1(data.mid(96, 32).toHex());
        pool.liquidityPoolIdHex = QString::fromLatin1(data.mid(128, 32).toHex());
        pool.liquidityPoolSupply = u128LeToDouble(data.mid(160, 16));
        pool.reserveA = u128LeToDouble(data.mid(176, 16));
        pool.reserveB = u128LeToDouble(data.mid(192, 16));
        pool.fees = u128LeToDouble(data.mid(208, 16));
        pool.active = data.size() == 224 || data.at(224) != 0;
        return true;
    }

    bool decodeFungibleHoldingData(const QString& dataHex, QString& definitionIdHex, double& balance)
    {
        const QString trimmed = dataHex.trimmed();
        // Borsh enum discriminant (u8) + AccountId (32 bytes) + u128 balance.
        if (trimmed.size() != 98)
            return false;
        const QByteArray data = QByteArray::fromHex(trimmed.toLatin1());
        if (data.size() != 49 || static_cast<unsigned char>(data.at(0)) != 0)
            return false;
        definitionIdHex = QString::fromLatin1(data.mid(1, 32).toHex());
        balance = u128LeToDouble(data.mid(33, 16));
        return true;
    }

    QString feeTierText(double feeBps)
    {
        QString percent = QString::number(feeBps / 100.0, 'f', 2);
        while (percent.contains(QLatin1Char('.')) && percent.endsWith(QLatin1Char('0')))
            percent.chop(1);
        if (percent.endsWith(QLatin1Char('.')))
            percent.chop(1);
        return percent + QStringLiteral("%");
    }

    QVariantMap tokenView(const QJsonObject& definition,
                          double reserve,
                          double balance,
                          const QString& holdingAccount)
    {
        const QString symbol = stringValue(definition, QStringLiteral("symbol"));
        return {
            { QStringLiteral("symbol"), symbol },
            { QStringLiteral("name"), stringValue(definition, QStringLiteral("name"), symbol) },
            { QStringLiteral("color"), stringValue(definition, QStringLiteral("color"), QStringLiteral("#627eea")) },
            { QStringLiteral("letter"), stringValue(definition, QStringLiteral("letter"), symbol.left(1)) },
            { QStringLiteral("address"), stringValue(definition, QStringLiteral("definitionAccount")) },
            { QStringLiteral("holdingAccount"), holdingAccount },
            { QStringLiteral("usdPrice"), numberValue(definition, QStringLiteral("usdPrice"), 1) },
            { QStringLiteral("balance"), balance },
            { QStringLiteral("reserve"), reserve }
        };
    }

    bool isHexAccountId(const QString& value)
    {
        const QString trimmed = value.trimmed();
        if (trimmed.size() != 64)
            return false;
        for (const QChar ch : trimmed) {
            if (!ch.isDigit()
                && (ch < QLatin1Char('a') || ch > QLatin1Char('f'))
                && (ch < QLatin1Char('A') || ch > QLatin1Char('F'))) {
                return false;
            }
        }
        return true;
    }

    void appendU32(QVariantList& words, quint32 word)
    {
        words.append(QVariant::fromValue(word));
    }

    void appendU128(QVariantList& words, quint64 value)
    {
        appendU32(words, static_cast<quint32>(value & 0xffffffffULL));
        appendU32(words, static_cast<quint32>((value >> 32) & 0xffffffffULL));
        appendU32(words, 0);
        appendU32(words, 0);
    }

    void appendString(QVariantList& words, const QString& value)
    {
        const QByteArray bytes = value.toUtf8();
        appendU32(words, static_cast<quint32>(bytes.size()));
        for (int i = 0; i < bytes.size(); i += 4) {
            quint32 word = 0;
            for (int j = 0; j < 4 && i + j < bytes.size(); ++j)
                word |= static_cast<quint32>(static_cast<unsigned char>(bytes.at(i + j))) << (j * 8);
            appendU32(words, word);
        }
    }

    QVariantList boolList(std::initializer_list<bool> values)
    {
        QVariantList result;
        for (const bool value : values)
            result.append(value);
        return result;
    }

    QVariantList byteList(const QByteArray& bytes)
    {
        QVariantList result;
        result.reserve(bytes.size());
        for (const char byte : bytes)
            result.append(static_cast<quint32>(static_cast<unsigned char>(byte)));
        return result;
    }

    bool walletOwnsPublicAccount(const AccountModel& model, const QString& accountIdHex)
    {
        for (int i = 0; i < model.count(); ++i) {
            const QModelIndex idx = model.index(i, 0);
            const QString address = model.data(idx, AccountModel::AddressRole).toString();
            const bool isPublic = model.data(idx, AccountModel::IsPublicRole).toBool();
            if (isPublic && address.compare(accountIdHex, Qt::CaseInsensitive) == 0)
                return true;
        }
        return false;
    }

    bool writeFileAtomically(const QString& path, const QByteArray& bytes)
    {
        const QFileInfo info(path);
        if (!QDir().mkpath(info.absolutePath()))
            return false;

        QSaveFile file(path);
        if (!file.open(QIODevice::WriteOnly))
            return false;
        if (file.write(bytes) != bytes.size())
            return false;
        return file.commit();
    }

    bool restoreFile(const QString& path, const QByteArray& bytes, bool existed)
    {
        if (existed)
            return writeFileAtomically(path, bytes);
        if (!QFileInfo::exists(path))
            return true;
        return QFile::remove(path);
    }

    QString canonicalTargetPath(const QString& path)
    {
        const QFileInfo fileInfo(path);
        QDir existingDir = fileInfo.absoluteDir();
        QStringList missingDirs;
        while (!existingDir.exists()) {
            const QString dirName = QFileInfo(existingDir.path()).fileName();
            if (dirName.isEmpty() || !existingDir.cdUp())
                return {};
            missingDirs.prepend(dirName);
        }

        QString targetPath = existingDir.canonicalPath();
        if (targetPath.isEmpty())
            return {};
        for (const QString& dirName : std::as_const(missingDirs))
            targetPath = QDir(targetPath).filePath(dirName);
        return QDir::cleanPath(QDir(targetPath).filePath(fileInfo.fileName()));
    }

    bool pathWithinDirectory(const QString& path, const QString& directory)
    {
        if (path == directory)
            return true;

        QString prefix = directory;
        if (!prefix.endsWith(QDir::separator()))
            prefix.append(QDir::separator());
        return path.startsWith(prefix);
    }

    QString validatedWalletFilePath(const QString& path,
                                    const QString& walletHome,
                                    const QString& label,
                                    QString* error)
    {
        const QString localPath = toLocalPath(path);
        if (localPath.isEmpty()) {
            if (error)
                *error = QStringLiteral("%1 path cannot be empty").arg(label);
            return {};
        }

        if (!QDir().mkpath(walletHome)) {
            if (error)
                *error = QStringLiteral("Cannot create wallet home");
            return {};
        }

        const QString walletRoot = QDir(walletHome).canonicalPath();
        if (walletRoot.isEmpty()) {
            if (error)
                *error = QStringLiteral("Wallet home path is invalid");
            return {};
        }

        if (QFileInfo(localPath).isSymLink()) {
            if (error)
                *error = QStringLiteral("%1 path cannot be a symbolic link").arg(label);
            return {};
        }

        const QString targetPath = canonicalTargetPath(localPath);
        if (targetPath.isEmpty() || !pathWithinDirectory(targetPath, walletRoot)) {
            if (error)
                *error = QStringLiteral("%1 path must be inside wallet home").arg(label);
            return {};
        }

        return targetPath;
    }

    QString validatedSequencerUrl(const QString& input, QString* error)
    {
        const QString trimmed = normalizedUrl(input);
        const QUrl url(trimmed, QUrl::StrictMode);
        const QString scheme = url.scheme().toLower();
        if (trimmed.isEmpty()) {
            if (error)
                *error = QStringLiteral("sequencer_addr cannot be empty");
            return {};
        }
        if (!url.isValid() || url.host().isEmpty()) {
            if (error)
                *error = QStringLiteral("sequencer_addr must be a valid URL with a host");
            return {};
        }
        if (scheme != QStringLiteral("http") && scheme != QStringLiteral("https")) {
            if (error)
                *error = QStringLiteral("sequencer_addr must use http or https");
            return {};
        }
        return trimmed;
    }

    // Matches the legacy AMM instruction enum encoded by the pinned LEZ binary.
    QVariantList addLiquidityInstruction(quint64 minLp, quint64 maxA, quint64 maxB)
    {
        QVariantList words;
        appendU32(words, 1);
        appendU128(words, minLp);
        appendU128(words, maxA);
        appendU128(words, maxB);
        return words;
    }

    QVariantList removeLiquidityInstruction(quint64 burnLp, quint64 minA, quint64 minB)
    {
        QVariantList words;
        appendU32(words, 2);
        appendU128(words, burnLp);
        appendU128(words, minA);
        appendU128(words, minB);
        return words;
    }

    QVariantList swapExactInputInstruction(quint64 amountIn, quint64 minOut, const QString& tokenDefinitionIn)
    {
        QVariantList words;
        appendU32(words, 3);
        appendU128(words, amountIn);
        appendU128(words, minOut);
        appendString(words, tokenDefinitionIn);
        return words;
    }

    QVariantList swapExactOutputInstruction(quint64 exactOut, quint64 maxIn, const QString& tokenDefinitionIn)
    {
        QVariantList words;
        appendU32(words, 4);
        appendU128(words, exactOut);
        appendU128(words, maxIn);
        appendString(words, tokenDefinitionIn);
        return words;
    }

    quint64 snapshotAmount(const QVariantMap& snapshot, const QString& key, QString* error)
    {
        bool ok = false;

        const QVariant value = snapshot.value(key);
        if (value.userType() == QMetaType::QString) {
            const QString text = value.toString().trimmed();
            if (text.isEmpty()) {
                ok = false;
            } else {
                ok = true;
                for (const QChar ch : text) {
                    if (!ch.isDigit()) {
                        ok = false;
                        break;
                    }
                }
            }

            if (ok) {
                const quint64 amount = text.toULongLong(&ok);
                if (ok && amount <= static_cast<quint64>(MAX_SAFE_QML_INTEGER))
                    return amount;
            }
        } else {
            const double amount = value.toDouble(&ok);
            if (ok
                && std::isfinite(amount)
                && amount >= 0
                && amount <= MAX_SAFE_QML_INTEGER
                && std::floor(amount) == amount) {
                return static_cast<quint64>(amount);
            }
        }

        if (error)
            *error = QStringLiteral("Invalid transaction amount: %1").arg(key);
        return 0;
    }
}

QString AmmUiBackend::defaultWalletHome()
{
    const QByteArray override = qgetenv(WALLET_HOME_ENV);
    if (!override.isEmpty())
        return QString::fromLocal8Bit(override);
    const QByteArray legacyOverride = qgetenv(LEGACY_WALLET_HOME_ENV);
    if (!legacyOverride.isEmpty())
        return QString::fromLocal8Bit(legacyOverride);
    // LEZ's canonical wallet home, shared with the wallet UI and other LEZ apps
    // (matches lez/wallet get_home_default_path()).
    return QDir::homePath() + QStringLiteral("/.lee/wallet");
}

QString AmmUiBackend::defaultConfigPath() const
{
    const QString path = defaultWalletHome() + QStringLiteral("/wallet_config.json");
    const QString canonical = canonicalTargetPath(path);
    return canonical.isEmpty() ? QDir::cleanPath(path) : canonical;
}

QString AmmUiBackend::defaultStoragePath() const
{
    const QString path = defaultWalletHome() + QStringLiteral("/storage.json");
    const QString canonical = canonicalTargetPath(path);
    return canonical.isEmpty() ? QDir::cleanPath(path) : canonical;
}

AmmUiBackend::AmmUiBackend(LogosAPI* logosAPI, QObject* parent)
    : AmmUiBackendSimpleSource(parent),
      m_accountModel(new AccountModel(this)),
      m_logosAPI(logosAPI ? logosAPI : new LogosAPI("amm_ui", this)),
      m_logos(new LogosModules(m_logosAPI)),
      m_net(new QNetworkAccessManager(this)),
      m_reachabilityTimer(new QTimer(this))
{
    // PROP defaults via the generated setters.
    setIsWalletOpen(false);
    setLastSyncedBlock(0);
    setCurrentBlockHeight(0);
    setWalletHome(defaultWalletHome());
    // Assume reachable until a probe proves otherwise (avoids a startup flash).
    setSequencerReachable(true);
    setDeploymentTokens({});
    setDeploymentPool({});
    setDeploymentNetworkMatched(true);
    setDeploymentIdentityPending(false);
    loadDeploymentConfig();

    // Periodically re-probe the sequencer so the banner reacts to a node going
    // up/down while the app is running. Probes are no-ops until a wallet (and
    // thus a sequencer address) is open.
    m_reachabilityTimer->setInterval(10000);
    connect(m_reachabilityTimer, &QTimer::timeout, this, [this]() { checkReachability(); });

    // Always resolve against the canonical wallet home (NSSA_WALLET_HOME_DIR,
    // LEE_WALLET_HOME_DIR, or ~/.lee/wallet). We intentionally don't seed config/storage paths from
    // QSettings anymore: a previously-persisted per-app path (~/.lee/amm-wallet)
    // would otherwise override the default and pin the app to the old keystore.

    // A wallet exists on disk if either canonical file is present (drives whether
    // the navbar "Connect" reconnects or offers to create a wallet).
    const QString effectiveConfig = configPath().isEmpty() ? defaultConfigPath() : configPath();
    const QString effectiveStorage = storagePath().isEmpty() ? defaultStoragePath() : storagePath();
    setWalletExists(QFileInfo::exists(effectiveConfig) || QFileInfo::exists(effectiveStorage));

    // ui-host runs our constructor inside initLogos(), synchronously, BEFORE
    // it enables remoting and emits READY. Any blocking RPC here would stall
    // ui-host startup past its ready watchdog. Defer the open+refresh chain to
    // the first event-loop tick so ui-host finishes wiring itself up first.
    QTimer::singleShot(0, this, [this]() { openOrAdoptWallet(); });

    // Save wallet on quit; host may not call destructors so this is best-effort.
    connect(qApp, &QCoreApplication::aboutToQuit, this,
            [this]() { saveWallet(); }, Qt::DirectConnection);
}

AmmUiBackend::~AmmUiBackend()
{
    saveWallet();
    delete m_logos;
}

void AmmUiBackend::openOrAdoptWallet()
{
    // Respect an explicit user disconnect: stay locked, show "Connect".
    if (QSettings(SETTINGS_ORG, SETTINGS_APP).value(DISCONNECTED_KEY, false).toBool())
        return;

    // Standalone (own core instance): auto-open a previously-created wallet.
    // If no local storage exists, still try adopting a shared Basecamp wallet.
    const QString cfg = configPath().isEmpty() ? defaultConfigPath() : configPath();
    const QString stg = storagePath().isEmpty() ? defaultStoragePath() : storagePath();
    if (!QFileInfo::exists(stg)) {
        adoptOpenWallet();
        return;
    }

    qDebug() << "AmmUiBackend: opening wallet with config" << cfg << "storage" << stg;
    const int err = m_logos->logos_execution_zone.open(cfg, stg);
    if (err == WALLET_FFI_SUCCESS) {
        persistConfigPath(cfg);
        persistStoragePath(stg);
        setIsWalletOpen(true);
        if (!m_reachabilityTimer->isActive())
            m_reachabilityTimer->start();
        refreshSequencerAddr();
        refreshAccounts();
        return;
    }

    // In Basecamp the logos_execution_zone module may already have an open
    // wallet. If opening the same disk wallet failed, try mirroring that state.
    if (adoptOpenWallet())
        return;

    qWarning() << "AmmUiBackend: wallet open failed, code" << err;
}

bool AmmUiBackend::adoptOpenWallet()
{
    const QJsonArray existing = listAccounts();
    if (existing.isEmpty())
        return false;

    qDebug() << "AmmUiBackend: adopting already-open shared wallet"
             << existing.size() << "accounts";
    setIsWalletOpen(true);
    if (!m_reachabilityTimer->isActive())
        m_reachabilityTimer->start();
    m_accountModel->replaceFromJsonArray(existing);
    refreshSequencerAddr();
    refreshBalances();
    QSettings(SETTINGS_ORG, SETTINGS_APP).setValue(DISCONNECTED_KEY, false);
    return true;
}

QString AmmUiBackend::createNewDefault(QString password)
{
    return createNew(defaultConfigPath(), defaultStoragePath(), password);
}

QString AmmUiBackend::createNew(QString configPath, QString storagePath, QString password)
{
    QString pathError;
    const QString localConfig =
        validatedWalletFilePath(configPath, defaultWalletHome(), QStringLiteral("Config"), &pathError);
    const QString localStorage =
        validatedWalletFilePath(storagePath, defaultWalletHome(), QStringLiteral("Storage"), &pathError);
    if (!pathError.isEmpty()) {
        qWarning() << "AmmUiBackend: refusing wallet path:" << pathError;
        return QString();
    }
    if (localConfig != defaultConfigPath() || localStorage != defaultStoragePath()) {
        qWarning() << "AmmUiBackend: refusing non-canonical wallet paths";
        return QString();
    }
    if (QFileInfo::exists(localConfig) || QFileInfo::exists(localStorage)) {
        qWarning() << "AmmUiBackend: refusing to create wallet over existing files";
        return QString();
    }

    const QString mnemonic = m_logos->logos_execution_zone.create_new(localConfig, localStorage, password);
    if (mnemonic.isEmpty()) {
        qWarning() << "AmmUiBackend: create_new failed (empty mnemonic)";
        return QString();
    }

    persistConfigPath(localConfig);
    persistStoragePath(localStorage);
    setWalletExists(true);
    QSettings(SETTINGS_ORG, SETTINGS_APP).setValue(DISCONNECTED_KEY, false);
    setIsWalletOpen(true);
    if (!m_reachabilityTimer->isActive())
        m_reachabilityTimer->start();
    refreshSequencerAddr();
    refreshAccounts();
    return mnemonic;
}

bool AmmUiBackend::openExisting()
{
    const QString cfg = configPath().isEmpty() ? defaultConfigPath() : configPath();
    const QString stg = storagePath().isEmpty() ? defaultStoragePath() : storagePath();
    if (!QFileInfo::exists(stg))
        return adoptOpenWallet();

    const int err = m_logos->logos_execution_zone.open(cfg, stg);
    if (err != WALLET_FFI_SUCCESS) {
        if (adoptOpenWallet())
            return true;
        qWarning() << "AmmUiBackend: openExisting failed, code" << err;
        return false;
    }
    persistConfigPath(cfg);
    persistStoragePath(stg);
    setIsWalletOpen(true);
    if (!m_reachabilityTimer->isActive())
        m_reachabilityTimer->start();
    QSettings(SETTINGS_ORG, SETTINGS_APP).setValue(DISCONNECTED_KEY, false);
    refreshSequencerAddr();
    refreshAccounts();
    return true;
}

void AmmUiBackend::disconnectWallet()
{
    // UI-local lock: persist wallet state, drop our view of it, and remember
    // the choice. We do NOT close the core module's wallet handle — in Basecamp
    // that instance is shared with other apps.
    saveWallet();
    ++m_reachabilityProbeGeneration;
    ++m_chainIdentityProbeGeneration;
    ++m_deploymentCheckGeneration;
    m_reachabilityTimer->stop();
    setIsWalletOpen(false);
    setSequencerAddr({});
    setSequencerReachable(true);
    m_accountModel->replaceFromJsonArray(QJsonArray());
    selectDeploymentForNetwork({});
    QSettings(SETTINGS_ORG, SETTINGS_APP).setValue(DISCONNECTED_KEY, true);
}

QString AmmUiBackend::createAccountPublic()
{
    const QString result = m_logos->logos_execution_zone.create_account_public();
    if (!result.isEmpty())
        refreshAccounts();
    return result;
}

QString AmmUiBackend::createAccountPrivate()
{
    const QString result = m_logos->logos_execution_zone.create_account_private();
    if (!result.isEmpty())
        refreshAccounts();
    return result;
}

void AmmUiBackend::refreshAccounts()
{
    const QJsonArray arr = listAccounts();
    m_accountModel->replaceFromJsonArray(arr);
    refreshBalances();
}

void AmmUiBackend::refreshBalances()
{
    refreshBlockHeights();
    if (currentBlockHeight() > 0)
        m_logos->logos_execution_zone.sync_to_block(static_cast<quint64>(currentBlockHeight()));

    for (int i = 0; i < m_accountModel->count(); ++i) {
        const QModelIndex idx = m_accountModel->index(i, 0);
        const QString addr = m_accountModel->data(idx, AccountModel::AddressRole).toString();
        const bool isPub = m_accountModel->data(idx, AccountModel::IsPublicRole).toBool();
        m_accountModel->setBalanceByAddress(addr, getBalance(addr, isPub));
    }
    refreshDeploymentWalletState();
    saveWallet();
}

QString AmmUiBackend::getBalance(QString accountIdHex, bool isPublic)
{
    return m_logos->logos_execution_zone.get_balance(accountIdHex, isPublic);
}

void AmmUiBackend::refreshBlockHeights()
{
    const int lastVal = m_logos->logos_execution_zone.get_last_synced_block();
    const int currentVal = m_logos->logos_execution_zone.get_current_block_height();
    if (lastSyncedBlock() != lastVal)
        setLastSyncedBlock(lastVal);
    if (currentBlockHeight() != currentVal)
        setCurrentBlockHeight(currentVal);
}

void AmmUiBackend::refreshSequencerAddr()
{
    if (!isWalletOpen()) {
        if (!sequencerAddr().isEmpty())
            setSequencerAddr({});
        setSequencerReachable(true);
        selectDeploymentForNetwork({});
        return;
    }

    const QString addr = m_logos->logos_execution_zone.get_sequencer_addr();
    if (sequencerAddr() != addr)
        setSequencerAddr(addr);
    if (addr.isEmpty()) {
        selectDeploymentForNetwork({});
        return;
    }
    clearDeploymentSelection(addr);
    // Probe right away so the banner reflects the connected chain without
    // trusting the endpoint URL as identity.
    probeChainIdentity(addr);
}

void AmmUiBackend::loadDeploymentConfig()
{
    const QJsonObject supportedChainsRoot =
        loadConfigObject(QStringLiteral("supported-chains.json"));
    const QJsonArray supportedChains =
        supportedChainsRoot.value(QStringLiteral("chains")).toArray();
    const QJsonObject tokenRoot = loadConfigObject(QStringLiteral("token-programs.json"));
    const QJsonObject ammRoot = loadConfigObject(QStringLiteral("amm-programs.json"));
    const QJsonObject ataRoot = loadConfigObject(QStringLiteral("ata-programs.json"));
    const QJsonObject twapOracleRoot =
        loadConfigObject(QStringLiteral("twap-oracle-programs.json"));
    const QJsonObject stablecoinRoot =
        loadConfigObject(QStringLiteral("stablecoin-programs.json"));

    const QString legacyNetwork = normalizedUrl(
        stringValue(tokenRoot, QStringLiteral("network"), QString::fromLatin1(DEFAULT_SEQUENCER)));
    m_tokenChains = deploymentChains(tokenRoot, legacyNetwork, supportedChains);
    m_ammChains = deploymentChains(ammRoot, legacyNetwork, supportedChains);
    m_programChainGroups = {};
    for (const QJsonObject& root :
         { tokenRoot, ammRoot, ataRoot, twapOracleRoot, stablecoinRoot }) {
        const QJsonArray chains = deploymentChains(root, legacyNetwork, supportedChains);
        if (!chains.isEmpty())
            m_programChainGroups.append(chains);
    }
    selectDeploymentForNetwork({});
}

void AmmUiBackend::selectDeploymentForNetwork(const QString& network)
{
    selectDeploymentForChain(network, {}, {});
}

void AmmUiBackend::clearDeploymentSelection(const QString& network)
{
    m_activeDeploymentConfigured = false;
    m_activeDeploymentDeployed = false;
    m_identityProbeInFlight = false;
    m_activeDeploymentNetwork = normalizedUrl(network);
    m_tokenDefinitions = {};
    m_poolConfig = {};
    m_requiredDeploymentTransactions = {};
    m_pendingDeploymentChecks = 0;
    m_deploymentChecksFailed = false;
    setDeploymentTokens({});
    setDeploymentPool({});
    setDeploymentIdentityPending(false);
    updateDeploymentNetworkMatched();
}

void AmmUiBackend::setDeploymentIdentityPendingIfNeeded(bool pending)
{
    if (deploymentIdentityPending() != pending) {
        setDeploymentIdentityPending(pending);
        updateDeploymentNetworkMatched();
    }
}

void AmmUiBackend::selectDeploymentForChain(const QString& network,
                                            const QString& blockHash,
                                            const QString& blockSignature)
{
    const QString requestedNetwork = normalizedUrl(network);
    const QJsonObject tokenChain =
        chainForFingerprint(m_tokenChains, requestedNetwork, blockHash, blockSignature);
    const QJsonObject ammChain =
        chainForFingerprint(m_ammChains, requestedNetwork, blockHash, blockSignature);

    m_activeDeploymentConfigured = false;
    m_activeDeploymentDeployed = false;
    m_identityProbeInFlight = false;
    m_activeDeploymentNetwork = requestedNetwork.isEmpty() ? chainNetwork(ammChain) : requestedNetwork;
    m_tokenDefinitions = {};
    m_poolConfig = {};
    m_requiredDeploymentTransactions = {};
    m_pendingDeploymentChecks = 0;
    m_deploymentChecksFailed = false;
    setDeploymentIdentityPending(false);

    if (tokenChain.isEmpty() || ammChain.isEmpty()) {
        if (!requestedNetwork.isEmpty())
            qWarning() << "AmmUiBackend: no AMM deployment configured for chain" << requestedNetwork;
        setDeploymentTokens({});
        setDeploymentPool({});
        updateDeploymentNetworkMatched();
        return;
    }

    if (m_activeDeploymentNetwork.isEmpty())
        m_activeDeploymentNetwork = chainNetwork(tokenChain);

    const QJsonObject tokenProgram = firstObject(tokenChain, QStringLiteral("programs"));
    const QJsonArray definitions = tokenProgram.value(QStringLiteral("definitions")).toArray();
    const QJsonObject ammProgram = firstObject(ammChain, QStringLiteral("programs"));
    const QString abi = stringValue(ammProgram, QStringLiteral("abi"));
    if (abi != QString::fromLatin1(LEGACY_AMM_ABI)) {
        qWarning() << "AmmUiBackend: unsupported AMM ABI" << abi;
        setDeploymentTokens({});
        setDeploymentPool({});
        updateDeploymentNetworkMatched();
        return;
    }
    const QJsonObject pool = firstObject(ammProgram, QStringLiteral("pools"));
    if (definitions.isEmpty() || pool.isEmpty()) {
        qWarning() << "AmmUiBackend: incomplete AMM deployment config for chain"
                   << m_activeDeploymentNetwork;
        setDeploymentTokens({});
        setDeploymentPool({});
        updateDeploymentNetworkMatched();
        return;
    }

    m_tokenDefinitions = definitions;
    m_poolConfig = pool;
    for (const QJsonValue& value : m_programChainGroups) {
        const QJsonObject chain =
            chainForFingerprint(value.toArray(), requestedNetwork, blockHash, blockSignature);
        appendDeploymentTransactions(chain, &m_requiredDeploymentTransactions);
    }
    m_activeDeploymentConfigured = true;
    verifyDeploymentTransactions();
    updateDeploymentNetworkMatched();
}

void AmmUiBackend::verifyDeploymentTransactions()
{
    const quint64 generation = ++m_deploymentCheckGeneration;
    if (!isWalletOpen() || m_requiredDeploymentTransactions.isEmpty()) {
        m_pendingDeploymentChecks = 0;
        m_deploymentChecksFailed = false;
        m_activeDeploymentDeployed = true;
        refreshDeploymentWalletState();
        return;
    }

    m_activeDeploymentDeployed = false;
    m_deploymentChecksFailed = false;
    m_pendingDeploymentChecks = m_requiredDeploymentTransactions.size();
    updateDeploymentNetworkMatched();

    const QString requested = normalizedUrl(sequencerAddr());
    for (const QString& hash : std::as_const(m_requiredDeploymentTransactions)) {
        QJsonArray params;
        params.append(hash);

        QNetworkRequest req{QUrl(requested)};
        req.setHeader(QNetworkRequest::ContentTypeHeader, QStringLiteral("application/json"));
        req.setTransferTimeout(4000);
        QNetworkReply* reply =
            m_net->post(req, jsonRpcBody(QStringLiteral("getTransaction"), params));
        connect(reply, &QNetworkReply::finished, this, [this, reply, requested, generation, hash]() {
            if (generation != m_deploymentCheckGeneration
                || normalizedUrl(sequencerAddr()) != requested) {
                reply->deleteLater();
                return;
            }

            const bool gotHttpStatus =
                reply->attribute(QNetworkRequest::HttpStatusCodeAttribute).isValid();
            const bool transportOk = gotHttpStatus || reply->error() == QNetworkReply::NoError;
            const TransactionLookupStatus status = transportOk
                ? transactionLookupStatus(reply->readAll())
                : TransactionLookupStatus::TransientFailure;
            if (status == TransactionLookupStatus::TransientFailure) {
                qWarning() << "AmmUiBackend: deployment transaction check failed"
                           << hash << "on" << requested;
                m_deploymentChecksFailed = true;
                setDeploymentIdentityPendingIfNeeded(true);
            } else if (status == TransactionLookupStatus::Missing) {
                qWarning() << "AmmUiBackend: deployment transaction not found"
                           << hash << "on" << requested;
                m_deploymentChecksFailed = true;
            }

            --m_pendingDeploymentChecks;
            if (m_pendingDeploymentChecks == 0) {
                m_activeDeploymentDeployed = !m_deploymentChecksFailed;
                if (m_activeDeploymentDeployed)
                    refreshDeploymentWalletState();
                else if (!deploymentIdentityPending()) {
                    setDeploymentTokens({});
                    setDeploymentPool({});
                    updateDeploymentNetworkMatched();
                }
            }
            reply->deleteLater();
        });
    }
}

void AmmUiBackend::refreshDeploymentWalletState()
{
    const QJsonObject tokenA = configuredTokenDefinition(
        stringValue(m_poolConfig, QStringLiteral("tokenA")), 0);
    const QJsonObject tokenB = configuredTokenDefinition(
        stringValue(m_poolConfig, QStringLiteral("tokenB")), 1);
    if (m_poolConfig.isEmpty() || tokenA.isEmpty() || tokenB.isEmpty()) {
        m_activeDeploymentDeployed = false;
        setDeploymentTokens({});
        setDeploymentPool({});
        updateDeploymentNetworkMatched();
        return;
    }

    const WalletFungibleHolding holdingA = walletFungibleHolding(
        stringValue(tokenA, QStringLiteral("definitionAccount")));
    const WalletFungibleHolding holdingB = walletFungibleHolding(
        stringValue(tokenB, QStringLiteral("definitionAccount")));
    const WalletFungibleHolding holdingLp = walletFungibleHolding(
        stringValue(m_poolConfig, QStringLiteral("lpDefinitionAccount")));
    const PoolChainState pool = poolChainState();
    if (isWalletOpen() && !pool.found) {
        qWarning() << "AmmUiBackend: AMM pool not deployed on chain"
                   << normalizedUrl(sequencerAddr());
        m_activeDeploymentDeployed = false;
        setDeploymentTokens({});
        setDeploymentPool({});
        updateDeploymentNetworkMatched();
        return;
    }

    m_activeDeploymentDeployed = true;

    QVariantList tokens;
    tokens.append(tokenView(tokenA, pool.reserveA, holdingA.balance, holdingA.accountIdHex));
    tokens.append(tokenView(tokenB, pool.reserveB, holdingB.balance, holdingB.accountIdHex));
    setDeploymentTokens(tokens);

    const int feeBps = static_cast<int>(pool.feeBps);
    const QVariantMap poolView{
        { QStringLiteral("account"), stringValue(m_poolConfig, QStringLiteral("account")) },
        { QStringLiteral("network"), m_activeDeploymentNetwork },
        { QStringLiteral("tokenA"), stringValue(m_poolConfig, QStringLiteral("tokenA")) },
        { QStringLiteral("tokenB"), stringValue(m_poolConfig, QStringLiteral("tokenB")) },
        { QStringLiteral("feeBps"), feeBps },
        { QStringLiteral("feeTier"), feeTierText(pool.feeBps) },
        { QStringLiteral("userLpBalance"), holdingLp.balance },
        { QStringLiteral("reserveA"), pool.reserveA },
        { QStringLiteral("reserveB"), pool.reserveB },
        { QStringLiteral("totalLpSupply"), pool.totalLpSupply },
        { QStringLiteral("walletBalanceA"), holdingA.balance },
        { QStringLiteral("walletBalanceB"), holdingB.balance }
    };
    setDeploymentPool(poolView);
    updateDeploymentNetworkMatched();
}

void AmmUiBackend::updateDeploymentNetworkMatched()
{
    const bool matched = !isWalletOpen()
                         || deploymentIdentityPending()
                         || (m_activeDeploymentConfigured && m_activeDeploymentDeployed);
    if (deploymentNetworkMatched() != matched)
        setDeploymentNetworkMatched(matched);
}

QJsonObject AmmUiBackend::configuredTokenDefinition(const QString& symbol, int fallbackIndex) const
{
    return tokenDefinition(m_tokenDefinitions, symbol, fallbackIndex);
}

QString AmmUiBackend::accountIdHex(const QString& accountId) const
{
    const QString trimmed = accountId.trimmed();
    if (trimmed.isEmpty())
        return {};
    if (isHexAccountId(trimmed))
        return trimmed.toLower();
    return m_logos->logos_execution_zone.account_id_from_base58(trimmed);
}

QStringList AmmUiBackend::accountIdHexList(const QStringList& accountIds, QString* error) const
{
    QStringList result;
    result.reserve(accountIds.size());
    for (const QString& accountId : accountIds) {
        const QString hex = accountIdHex(accountId);
        if (hex.isEmpty()) {
            if (error)
                *error = QStringLiteral("Invalid account id in AMM config: %1").arg(accountId);
            return {};
        }
        result.append(hex);
    }
    return result;
}

AmmUiBackend::PoolChainState AmmUiBackend::poolChainState() const
{
    PoolChainState result;
    if (!isWalletOpen())
        return result;

    const QString poolAccountIdHex = accountIdHex(stringValue(m_poolConfig, QStringLiteral("account")));
    if (poolAccountIdHex.isEmpty())
        return result;

    const QString accountJson = m_logos->logos_execution_zone.get_account_public(poolAccountIdHex);
    const QJsonDocument doc = QJsonDocument::fromJson(accountJson.toUtf8());
    if (!doc.isObject())
        return result;

    DecodedPoolDefinition decoded;
    if (!decodePoolDefinitionData(
            doc.object().value(QStringLiteral("data")).toString(),
            decoded)) {
        return result;
    }

    const QJsonObject tokenA = configuredTokenDefinition(
        stringValue(m_poolConfig, QStringLiteral("tokenA")), 0);
    const QJsonObject tokenB = configuredTokenDefinition(
        stringValue(m_poolConfig, QStringLiteral("tokenB")), 1);
    const QString definitionAIdHex = accountIdHex(stringValue(tokenA, QStringLiteral("definitionAccount")));
    const QString definitionBIdHex = accountIdHex(stringValue(tokenB, QStringLiteral("definitionAccount")));
    const QString vaultAIdHex = accountIdHex(stringValue(m_poolConfig, QStringLiteral("vaultA")));
    const QString vaultBIdHex = accountIdHex(stringValue(m_poolConfig, QStringLiteral("vaultB")));
    const QString lpDefinitionIdHex = accountIdHex(
        stringValue(m_poolConfig, QStringLiteral("lpDefinitionAccount")));

    const bool matchesConfig =
        decoded.definitionTokenAIdHex.compare(definitionAIdHex, Qt::CaseInsensitive) == 0
        && decoded.definitionTokenBIdHex.compare(definitionBIdHex, Qt::CaseInsensitive) == 0
        && decoded.vaultAIdHex.compare(vaultAIdHex, Qt::CaseInsensitive) == 0
        && decoded.vaultBIdHex.compare(vaultBIdHex, Qt::CaseInsensitive) == 0
        && decoded.liquidityPoolIdHex.compare(lpDefinitionIdHex, Qt::CaseInsensitive) == 0;
    if (!matchesConfig) {
        qWarning() << "AmmUiBackend: pool account state does not match deployment config";
        return result;
    }
    if (!decoded.active) {
        qWarning() << "AmmUiBackend: pool account is inactive";
        return result;
    }

    result.reserveA = decoded.reserveA;
    result.reserveB = decoded.reserveB;
    result.totalLpSupply = decoded.liquidityPoolSupply;
    result.feeBps = decoded.fees;
    result.found = true;
    return result;
}

AmmUiBackend::WalletFungibleHolding AmmUiBackend::walletFungibleHolding(
    const QString& definitionAccountId,
    const QString& accountIdFilterHex) const
{
    WalletFungibleHolding result;
    const QString definitionIdHex = accountIdHex(definitionAccountId);
    if (definitionIdHex.isEmpty())
        return result;
    const QString requiredAccountIdHex = canonicalHex(accountIdFilterHex);

    for (int i = 0; i < m_accountModel->count(); ++i) {
        const QModelIndex idx = m_accountModel->index(i, 0);
        if (!m_accountModel->data(idx, AccountModel::IsPublicRole).toBool())
            continue;

        const QString accountIdHex = m_accountModel->data(idx, AccountModel::AddressRole).toString();
        if (accountIdHex.isEmpty())
            continue;
        if (!requiredAccountIdHex.isEmpty()
            && accountIdHex.compare(requiredAccountIdHex, Qt::CaseInsensitive) != 0) {
            continue;
        }

        const QString accountJson = m_logos->logos_execution_zone.get_account_public(accountIdHex);
        const QJsonDocument doc = QJsonDocument::fromJson(accountJson.toUtf8());
        if (!doc.isObject())
            continue;

        QString holdingDefinitionIdHex;
        double balance = 0;
        if (!decodeFungibleHoldingData(
                doc.object().value(QStringLiteral("data")).toString(),
                holdingDefinitionIdHex,
                balance)) {
            continue;
        }

        if (holdingDefinitionIdHex.compare(definitionIdHex, Qt::CaseInsensitive) != 0)
            continue;

        if (result.found) {
            result.ambiguous = true;
            return result;
        }
        result.accountIdHex = accountIdHex;
        result.balance = balance;
        result.found = true;
    }

    return result;
}

QString AmmUiBackend::selectedWalletAccountIdHex(const QVariantMap& snapshot, QString* error) const
{
    const QString selected =
        snapshot.value(QStringLiteral("selectedWalletAccount")).toString().trimmed();
    if (selected.isEmpty()) {
        if (error)
            *error = QStringLiteral("No wallet account selected");
        return {};
    }

    const QString selectedHex = accountIdHex(selected);
    if (selectedHex.isEmpty()) {
        if (error)
            *error = QStringLiteral("Selected wallet account is invalid");
        return {};
    }

    if (!walletOwnsPublicAccount(*m_accountModel, selectedHex)) {
        if (error)
            *error = QStringLiteral("Selected wallet account is not a public account controlled by this wallet");
        return {};
    }

    return selectedHex;
}

QString AmmUiBackend::submitAmmTransaction(const QStringList& accountIds,
                                           const QVariantList& signingRequirements,
                                           const QVariantList& instruction)
{
    if (!isWalletOpen())
        return txError(QStringLiteral("Wallet is not connected"));
    if (!deploymentNetworkMatched())
        return txError(QStringLiteral("Unsupported chain"));
    for (int i = 0; i < accountIds.size(); ++i) {
        if (!signingRequirements.at(i).toBool())
            continue;
        if (walletOwnsPublicAccount(*m_accountModel, accountIds.at(i)))
            continue;
        const QString displayAccountId = m_logos->logos_execution_zone.account_id_to_base58(accountIds.at(i));
        if (displayAccountId.isEmpty())
            return txError(QStringLiteral("Internal error: required signer account cannot be displayed"));
        return txError(QStringLiteral("Wallet does not control required signer account: %1")
                           .arg(displayAccountId));
    }

    QString error;
    const QByteArray ammElf = loadProgramBinary(QStringLiteral("amm.bin"), &error);
    if (ammElf.isEmpty())
        return txError(error);
    const QByteArray tokenElf = loadProgramBinary(QStringLiteral("token.bin"), &error);
    if (tokenElf.isEmpty())
        return txError(error);

    QVariantList dependencies;
    dependencies.append(QVariant::fromValue(byteList(tokenElf)));
    const QString result = m_logos->logos_execution_zone.send_generic_public_transaction(
        accountIds, signingRequirements, QVariant::fromValue(instruction), ammElf,
        QVariant::fromValue(dependencies));
    if (result.isEmpty())
        return txError(QStringLiteral("Wallet returned an empty transaction result"));

    const QJsonDocument doc = QJsonDocument::fromJson(result.toUtf8());
    if (doc.isObject() && doc.object().value(QStringLiteral("success")).toBool())
        refreshBalances();
    return result;
}

QString AmmUiBackend::submitSwap(QVariantMap snapshot)
{
    const QJsonObject tokenA = configuredTokenDefinition(
        stringValue(m_poolConfig, QStringLiteral("tokenA")), 0);
    const QJsonObject tokenB = configuredTokenDefinition(
        stringValue(m_poolConfig, QStringLiteral("tokenB")), 1);
    if (m_poolConfig.isEmpty() || tokenA.isEmpty() || tokenB.isEmpty())
        return txError(QStringLiteral("AMM deployment config is incomplete"));

    const QString sellSymbol = snapshot.value(QStringLiteral("sellToken")).toString();
    const bool sellA = sellSymbol == stringValue(tokenA, QStringLiteral("symbol"));
    const bool sellB = sellSymbol == stringValue(tokenB, QStringLiteral("symbol"));
    if (!sellA && !sellB)
        return txError(QStringLiteral("Swap token is not part of the configured pool"));

    QString error;
    const QString selectedAccountIdHex = selectedWalletAccountIdHex(snapshot, &error);
    if (!error.isEmpty())
        return txError(error);

    const QString tokenDefinitionIn = stringValue(
        sellA ? tokenA : tokenB, QStringLiteral("definitionAccount"));
    if (tokenDefinitionIn.isEmpty())
        return txError(QStringLiteral("Token definition account missing from AMM config"));
    QVariantList instruction;
    if (snapshot.value(QStringLiteral("swapMode")).toString() == QStringLiteral("swap-exact-output")) {
        instruction = swapExactOutputInstruction(
            snapshotAmount(snapshot, QStringLiteral("buyAmountValue"), &error),
            snapshotAmount(snapshot, QStringLiteral("maxSentAmountValue"), &error),
            tokenDefinitionIn);
    } else {
        instruction = swapExactInputInstruction(
            snapshotAmount(snapshot, QStringLiteral("sellAmountValue"), &error),
            snapshotAmount(snapshot, QStringLiteral("minReceivedAmountValue"), &error),
            tokenDefinitionIn);
    }
    if (!error.isEmpty())
        return txError(error);

    const WalletFungibleHolding selectedHolding = walletFungibleHolding(
        tokenDefinitionIn, selectedAccountIdHex);
    if (!selectedHolding.found) {
        return txError(QStringLiteral("Selected account is not a %1 holding account").arg(
            stringValue(sellA ? tokenA : tokenB, QStringLiteral("symbol"))));
    }

    const WalletFungibleHolding holdingA = sellA
        ? selectedHolding
        : walletFungibleHolding(stringValue(tokenA, QStringLiteral("definitionAccount")));
    const WalletFungibleHolding holdingB = sellB
        ? selectedHolding
        : walletFungibleHolding(stringValue(tokenB, QStringLiteral("definitionAccount")));
    if (!holdingA.found)
        return txError(QStringLiteral("Wallet has no %1 holding account").arg(
            stringValue(tokenA, QStringLiteral("symbol"))));
    if (!holdingB.found)
        return txError(QStringLiteral("Wallet has no %1 holding account").arg(
            stringValue(tokenB, QStringLiteral("symbol"))));
    if (holdingA.ambiguous)
        return txError(QStringLiteral("Wallet has multiple %1 holding accounts").arg(
            stringValue(tokenA, QStringLiteral("symbol"))));
    if (holdingB.ambiguous)
        return txError(QStringLiteral("Wallet has multiple %1 holding accounts").arg(
            stringValue(tokenB, QStringLiteral("symbol"))));

    QStringList swapAccountIds;
    swapAccountIds << stringValue(m_poolConfig, QStringLiteral("account"))
                   << stringValue(m_poolConfig, QStringLiteral("vaultA"))
                   << stringValue(m_poolConfig, QStringLiteral("vaultB"))
                   << holdingA.accountIdHex
                   << holdingB.accountIdHex;
    const QStringList accountIds = accountIdHexList(swapAccountIds, &error);
    if (!error.isEmpty())
        return txError(error);

    return submitAmmTransaction(
        accountIds,
        boolList({ false, false, false, sellA, sellB }),
        instruction);
}

QString AmmUiBackend::submitLiquidity(QVariantMap snapshot)
{
    const QJsonObject tokenA = configuredTokenDefinition(
        stringValue(m_poolConfig, QStringLiteral("tokenA")), 0);
    const QJsonObject tokenB = configuredTokenDefinition(
        stringValue(m_poolConfig, QStringLiteral("tokenB")), 1);
    if (m_poolConfig.isEmpty() || tokenA.isEmpty() || tokenB.isEmpty())
        return txError(QStringLiteral("AMM deployment config is incomplete"));

    const QString action = snapshot.value(QStringLiteral("action")).toString();
    QString error;
    const QString selectedAccountIdHex = selectedWalletAccountIdHex(snapshot, &error);
    if (!error.isEmpty())
        return txError(error);

    QVariantList instruction;
    QVariantList signingRequirements;
    if (action == QStringLiteral("add")) {
        instruction = addLiquidityInstruction(
            snapshotAmount(snapshot, QStringLiteral("minLpReceivedAmount"), &error),
            snapshotAmount(snapshot, QStringLiteral("actualAValue"), &error),
            snapshotAmount(snapshot, QStringLiteral("actualBValue"), &error));
        signingRequirements = boolList({ false, false, false, false, true, true, false });
    } else if (action == QStringLiteral("remove")) {
        instruction = removeLiquidityInstruction(
            snapshotAmount(snapshot, QStringLiteral("burnAmount"), &error),
            snapshotAmount(snapshot, QStringLiteral("minTokenAReceivedAmount"), &error),
            snapshotAmount(snapshot, QStringLiteral("minTokenBReceivedAmount"), &error));
        signingRequirements = boolList({ false, false, false, false, false, false, true });
    } else {
        return txError(QStringLiteral("Unknown liquidity action"));
    }
    if (!error.isEmpty())
        return txError(error);

    const WalletFungibleHolding selectedHoldingA = walletFungibleHolding(
        stringValue(tokenA, QStringLiteral("definitionAccount")), selectedAccountIdHex);
    const WalletFungibleHolding selectedHoldingB = walletFungibleHolding(
        stringValue(tokenB, QStringLiteral("definitionAccount")), selectedAccountIdHex);
    const WalletFungibleHolding holdingA = selectedHoldingA.found
        ? selectedHoldingA
        : walletFungibleHolding(stringValue(tokenA, QStringLiteral("definitionAccount")));
    const WalletFungibleHolding holdingB = selectedHoldingB.found
        ? selectedHoldingB
        : walletFungibleHolding(stringValue(tokenB, QStringLiteral("definitionAccount")));
    if (!holdingA.found)
        return txError(QStringLiteral("Wallet has no %1 holding account").arg(
            stringValue(tokenA, QStringLiteral("symbol"))));
    if (!holdingB.found)
        return txError(QStringLiteral("Wallet has no %1 holding account").arg(
            stringValue(tokenB, QStringLiteral("symbol"))));
    if (holdingA.ambiguous)
        return txError(QStringLiteral("Wallet has multiple %1 holding accounts").arg(
            stringValue(tokenA, QStringLiteral("symbol"))));
    if (holdingB.ambiguous)
        return txError(QStringLiteral("Wallet has multiple %1 holding accounts").arg(
            stringValue(tokenB, QStringLiteral("symbol"))));

    const WalletFungibleHolding holdingLp = walletFungibleHolding(
        stringValue(m_poolConfig, QStringLiteral("lpDefinitionAccount")),
        action == QStringLiteral("remove") ? selectedAccountIdHex : QString{});
    QString lpHoldingAccountId = holdingLp.accountIdHex;
    if (holdingLp.ambiguous)
        return txError(QStringLiteral("Wallet has multiple LP holding accounts for this pool"));
    if (action == QStringLiteral("add") && lpHoldingAccountId.isEmpty()) {
        lpHoldingAccountId = createAccountPublic();
        if (lpHoldingAccountId.isEmpty())
            return txError(QStringLiteral("Could not create LP holding account"));
    } else if (action == QStringLiteral("remove") && !holdingLp.found) {
        return txError(QStringLiteral("Selected account is not an LP holding account for this pool"));
    }

    QStringList liquidityAccountIds;
    liquidityAccountIds << stringValue(m_poolConfig, QStringLiteral("account"))
                        << stringValue(m_poolConfig, QStringLiteral("vaultA"))
                        << stringValue(m_poolConfig, QStringLiteral("vaultB"))
                        << stringValue(m_poolConfig, QStringLiteral("lpDefinitionAccount"))
                        << holdingA.accountIdHex
                        << holdingB.accountIdHex
                        << lpHoldingAccountId;
    const QStringList accountIds = accountIdHexList(liquidityAccountIds, &error);
    if (!error.isEmpty())
        return txError(error);

    return submitAmmTransaction(accountIds, signingRequirements, instruction);
}

void AmmUiBackend::checkReachability()
{
    const QString requested = normalizedUrl(sequencerAddr());
    if (requested.isEmpty())
        return;
    const quint64 generation = ++m_reachabilityProbeGeneration;

    QNetworkRequest req{QUrl(requested)};
    req.setTransferTimeout(4000);
    QNetworkReply* reply = m_net->get(req);
    connect(reply, &QNetworkReply::finished, this, [this, reply, requested, generation]() {
        if (generation != m_reachabilityProbeGeneration
            || normalizedUrl(sequencerAddr()) != requested) {
            reply->deleteLater();
            return;
        }

        // Any HTTP response (even a 404) means the node is up; only a transport
        // failure (connection refused, host not found, timeout) counts as down.
        const bool gotHttpStatus =
            reply->attribute(QNetworkRequest::HttpStatusCodeAttribute).isValid();
        const bool reachable = gotHttpStatus || reply->error() == QNetworkReply::NoError;
        const bool wasReachable = sequencerReachable();
        if (wasReachable != reachable)
            setSequencerReachable(reachable);
        if (reachable
            && (!wasReachable || deploymentIdentityPending())
            && !m_identityProbeInFlight) {
            probeChainIdentity(requested);
        }
        reply->deleteLater();
    });
}

void AmmUiBackend::probeChainIdentity(const QString& network)
{
    const QString requested = normalizedUrl(network);
    const quint64 generation = ++m_chainIdentityProbeGeneration;
    clearDeploymentSelection(requested);
    m_identityProbeInFlight = true;
    setDeploymentIdentityPendingIfNeeded(true);

    QJsonArray params;
    params.append(CHAIN_IDENTITY_BLOCK);

    QNetworkRequest req{QUrl(requested)};
    req.setHeader(QNetworkRequest::ContentTypeHeader, QStringLiteral("application/json"));
    req.setTransferTimeout(4000);
    QNetworkReply* reply =
        m_net->post(req, jsonRpcBody(QStringLiteral("getBlock"), params));
    connect(reply, &QNetworkReply::finished, this, [this, reply, requested, generation]() {
        if (generation != m_chainIdentityProbeGeneration
            || normalizedUrl(sequencerAddr()) != requested) {
            if (generation == m_chainIdentityProbeGeneration)
                m_identityProbeInFlight = false;
            reply->deleteLater();
            return;
        }
        m_identityProbeInFlight = false;

        const bool gotHttpStatus =
            reply->attribute(QNetworkRequest::HttpStatusCodeAttribute).isValid();
        const bool reachable = gotHttpStatus || reply->error() == QNetworkReply::NoError;
        if (sequencerReachable() != reachable)
            setSequencerReachable(reachable);

        const QByteArray payload = reply->readAll();
        const ChainFingerprint fingerprint = chainFingerprintFromGetBlockResponse(payload);
        if (!fingerprint.isValid()) {
            qWarning() << "AmmUiBackend: could not read chain identity from" << requested;
            setDeploymentIdentityPendingIfNeeded(false);
            reply->deleteLater();
            return;
        }

        setDeploymentIdentityPendingIfNeeded(false);
        selectDeploymentForChain(
            requested, fingerprint.blockHash, fingerprint.blockSignature);
        reply->deleteLater();
    });
}

void AmmUiBackend::saveWallet()
{
    if (isWalletOpen())
        m_logos->logos_execution_zone.save();
}

// These only update the in-session PROPs (so subsequent open/refresh calls
// reuse the same path). They are no longer written to QSettings: the app
// always resolves against the canonical wallet home, so there's nothing to
// remember across launches.
void AmmUiBackend::persistConfigPath(const QString& path)
{
    setConfigPath(toLocalPath(path));
}

void AmmUiBackend::persistStoragePath(const QString& path)
{
    setStoragePath(toLocalPath(path));
}

QJsonArray AmmUiBackend::listAccounts()
{
    const QJsonArray raw = QJsonArray::fromVariantList(m_logos->logos_execution_zone.list_accounts());
    QJsonArray accounts;

    for (const QJsonValue& value : raw) {
        QJsonObject account = value.isObject() ? value.toObject() : QJsonObject{};
        if (!value.isObject())
            account.insert(QString::fromLatin1(ACCOUNT_ID_KEY), value.toString());

        const QString accountIdHex = account.value(QString::fromLatin1(ACCOUNT_ID_KEY)).toString();
        const QString accountIdBase58 = m_logos->logos_execution_zone.account_id_to_base58(accountIdHex);
        account.insert(QString::fromLatin1(DISPLAY_ACCOUNT_ID_KEY),
                       accountIdBase58.isEmpty() ? accountIdHex : accountIdBase58);
        accounts.append(account);
    }

    return accounts;
}

bool AmmUiBackend::changeSequencerAddr(QString url)
{
    QString validationError;
    const QString validated = validatedSequencerUrl(url, &validationError);
    if (validated.isEmpty()) {
        qWarning() << "AmmUiBackend: refusing sequencer_addr:" << validationError;
        return false;
    }

    QString pathError;
    const QString cfg = validatedWalletFilePath(
        configPath().isEmpty() ? defaultConfigPath() : configPath(),
        defaultWalletHome(),
        QStringLiteral("Config"),
        &pathError);
    const QString stg = validatedWalletFilePath(
        storagePath().isEmpty() ? defaultStoragePath() : storagePath(),
        defaultWalletHome(),
        QStringLiteral("Storage"),
        &pathError);
    if (!pathError.isEmpty()) {
        qWarning() << "AmmUiBackend: refusing wallet path:" << pathError;
        return false;
    }
    // Preserve the other config fields (poll timeouts, retries) — only swap the
    // endpoint. The wallet reads this file on open via from_path_or_initialize_default.
    QJsonObject obj;
    QByteArray oldConfigBytes;
    const bool oldConfigExists = QFileInfo::exists(cfg);
    QFile in(cfg);
    if (oldConfigExists) {
        if (!in.open(QIODevice::ReadOnly)) {
            qWarning() << "AmmUiBackend: cannot read wallet config" << cfg;
            return false;
        }

        oldConfigBytes = in.readAll();
        QJsonParseError parseError;
        const QJsonDocument doc = QJsonDocument::fromJson(oldConfigBytes, &parseError);
        in.close();
        if (parseError.error != QJsonParseError::NoError || !doc.isObject()) {
            qWarning() << "AmmUiBackend: invalid wallet config" << cfg
                       << parseError.errorString();
            return false;
        }
        obj = doc.object();
    }
    obj.insert(QStringLiteral("sequencer_addr"), validated);
    const QByteArray newConfigBytes = QJsonDocument(obj).toJson(QJsonDocument::Indented);

    if (!writeFileAtomically(cfg, newConfigBytes)) {
        qWarning() << "AmmUiBackend: cannot atomically write wallet config" << cfg;
        return false;
    }

    // Re-open from the final config path so later wallet saves keep using it.
    if (isWalletOpen()) {
        const int err = m_logos->logos_execution_zone.open(cfg, stg);
        if (err != WALLET_FFI_SUCCESS) {
            qWarning() << "AmmUiBackend: final reopen after sequencer change failed, code" << err;
            if (!restoreFile(cfg, oldConfigBytes, oldConfigExists)) {
                qWarning() << "AmmUiBackend: rollback after sequencer change failed";
                return false;
            }
            const int restoreErr = m_logos->logos_execution_zone.open(cfg, stg);
            if (restoreErr != WALLET_FFI_SUCCESS) {
                qWarning() << "AmmUiBackend: rollback reopen after sequencer change failed, code"
                           << restoreErr;
            }
            return false;
        }
        refreshSequencerAddr();
        refreshAccounts();
    }
    return true;
}
