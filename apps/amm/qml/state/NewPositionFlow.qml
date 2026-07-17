import QtQml

QtObject {
    id: root

    property var backend: null
    property var runtime: null
    property bool active: false

    readonly property bool backendReady: root.backend !== null
    readonly property bool walletCanSubmit: root.backendReady
                                             && root.backend.walletCanSubmit === true
    readonly property var newPositionContext: root.backendReady
                                              && root.backend.newPositionContext
                                              && root.backend.newPositionContext.schema
                                              ? root.backend.newPositionContext
                                              : root.loadingContext()
    readonly property var viewState: ({
        "quote": root.newPositionQuote,
        "contextLoading": root.contextLoading
                          || root.newPositionContext.status === "loading",
        "quoteLoading": root.quoteLoading,
        "quoteStale": root.quoteStale,
        "submitting": root.submitting,
        "walletCanSubmit": root.walletCanSubmit,
        "walletSyncStatus": root.backendReady
                            ? String(root.backend.walletSyncStatus || "closed") : "closed",
        "poolCreationPending": root.selectedPoolCreationPending(),
        "transactionId": root.transactionId,
        "errorCode": root.flowErrorCode || root.contextErrorCode
                     || root.quoteErrorCode
    })

    property var newPositionQuote: ({})
    property var resolvedTokenIds: []
    property int quoteSerial: 0
    property int operationSerial: 0
    property int activeQuoteRequestId: 0
    property int submitRequestId: 0
    property int poolProbeRequestId: 0
    property var pendingSubmitSnapshot: ({})
    property var pendingConfirmationSnapshot: null
    property bool contextLoading: false
    property bool quoteLoading: false
    property bool quoteStale: true
    property bool submitting: false
    property string transactionId: ""
    property string flowErrorCode: ""
    property string contextErrorCode: ""
    property string quoteErrorCode: ""
    property var pendingContextCompletion: null
    property var pendingQuoteRequest: ({ "ok": false, "request": ({}) })
    property var pendingPoolProbes: []
    property bool poolProbeInFlight: false

    signal tokenResolutionFinished(bool finalResponse)
    signal tokenResolutionFailed(string code)
    signal poolActivated(var quote)
    signal quoteRefreshRequested(bool immediate)
    signal submitSucceeded
    signal submitFailed
    signal confirmationQuoteReady(var snapshot)

    objectName: "newPositionFlow"

    property Timer quoteDebounce: Timer {
        interval: 250
        repeat: false
        onTriggered: root.requestQuoteNow(root.quoteSerial)
    }

    property Timer poolPoller: Timer {
        interval: 5000
        repeat: true
        running: root.pendingPoolProbes.length > 0
        onTriggered: root.pollPendingPool()
    }

    property Connections backendConnections: Connections {
        target: root.backend
        ignoreUnknownSignals: true

        function onNewPositionQuoteResultChanged() {
            root.acceptQuoteResult(root.backend.newPositionQuoteResult || ({}))
        }

        function onNewPositionSubmitResultChanged() {
            root.acceptSubmitResult(root.backend.newPositionSubmitResult || ({}))
        }
    }

    onNewPositionContextChanged: {
        root.contextLoading = false
        root.contextErrorCode = root.newPositionContext.status === "error"
                                ? String(root.newPositionContext.code || "backend_error") : ""
        root.invalidateQuote()
        const completed = root.pendingContextCompletion
        root.pendingContextCompletion = null
        root.tokenResolutionFinished(true)
        if (completed)
            completed()
    }

    onActiveChanged: {
        if (root.active && root.backendReady)
            Qt.callLater(function() { root.quoteRefreshRequested(true) })
    }

    onBackendReadyChanged: {
        if (root.backendReady || !root.submitting)
            return
        root.submitRequestId = 0
        root.submitting = false
        root.pendingSubmitSnapshot = ({})
        root.flowErrorCode = "submission_status_unknown"
        root.submitFailed()
    }

    function contextHints(refreshPublicData) {
        const request = root.pendingQuoteRequest.request || {}
        const recent = []
        if (request.tokenAId)
            recent.push(request.tokenAId)
        if (request.tokenBId && request.tokenBId !== request.tokenAId)
            recent.push(request.tokenBId)
        return {
            "recentTokenIds": recent,
            "resolvedTokenIds": root.resolvedTokenIds,
            "refreshWalletAccounts": refreshPublicData === true
        }
    }

    function refreshContext(refreshPublicData, completed) {
        root.contextLoading = true
        root.pendingContextCompletion = completed || null
        if (!root.backendReady) {
            root.contextLoading = false
            return
        }
        try {
            root.backend.refreshNewPositionContext(root.contextHints(refreshPublicData))
        } catch (_error) {
            root.contextLoading = false
            root.contextErrorCode = "backend_error"
            root.tokenResolutionFailed("backend_error")
        }
    }

    function resolveToken(tokenId) {
        const value = String(tokenId || "").trim()
        if (value.length === 0)
            return
        if (root.resolvedTokenIds.indexOf(value) < 0) {
            const next = root.resolvedTokenIds.slice(0)
            next.push(value)
            root.resolvedTokenIds = next
        }
        root.refreshContext(false)
    }

    function scheduleQuote(immediate, quoteRequest) {
        ++root.quoteSerial
        root.pendingQuoteRequest = quoteRequest
        root.quoteStale = true
        root.quoteLoading = root.backendReady && root.active
        root.quoteDebounce.stop()
        if (!root.backendReady || !root.active)
            return
        if (immediate)
            root.requestQuoteNow(root.quoteSerial)
        else
            root.quoteDebounce.restart()
    }

    function requestQuoteNow(serial) {
        if (serial !== root.quoteSerial)
            return
        const built = root.pendingQuoteRequest
        if (!built.ok || !root.backendReady || !root.active) {
            root.quoteLoading = false
            return
        }
        root.activeQuoteRequestId = ++root.operationSerial
        root.backend.requestNewPositionQuote(
            built.request, root.activeQuoteRequestId, true)
    }

    function acceptQuoteResult(result) {
        const requestId = Number(result.requestId || 0)
        if (requestId === root.poolProbeRequestId) {
            root.finishPoolProbe(root.pendingPoolProbes[0], result)
            return
        }
        if (requestId !== root.activeQuoteRequestId)
            return
        root.quoteLoading = false
        root.quoteStale = false
        root.quoteErrorCode = ""
        root.newPositionQuote = result.schema === "new-position.v2"
                                ? result : root.quoteError("unsupported_schema")
        if (root.pendingConfirmationSnapshot) {
            var snapshot = root.pendingConfirmationSnapshot
            root.pendingConfirmationSnapshot = null
            snapshot.quoteHash = String(root.newPositionQuote.quoteHash || "")
            snapshot.expectedLpText = String(root.newPositionQuote.expectedLpRaw || "")
                                  + " raw LP"
            snapshot.instruction = String(root.newPositionQuote.instruction || "")
            snapshot.lpHoldingOptions = root.newPositionQuote.lpHoldingOptions || []
            snapshot.selectedLpHoldingId = String(
                root.newPositionQuote.selectedLpHoldingId || "")
            snapshot.createFreshLp = root.newPositionQuote.requiresFreshLp === true
            snapshot.lpDestinationRequired =
                root.newPositionQuote.lpDestinationRequired === true
            snapshot.quoteReady = root.newPositionQuote.canSubmit === true
            root.confirmationQuoteReady(snapshot)
        }
    }

    function requoteConfirmation(snapshot) {
        root.pendingConfirmationSnapshot = snapshot
        root.scheduleQuote(true, {
            "ok": true,
            "errors": [],
            "request": snapshot.request
        })
    }

    function confirm(snapshot) {
        if (root.submitting)
            return
        if (!root.walletCanSubmit) {
            root.finishSubmitFailure(root.quoteError("wallet_syncing"))
            return
        }
        root.submitting = true
        root.flowErrorCode = ""
        root.submitRequestId = ++root.operationSerial
        root.pendingSubmitSnapshot = snapshot
        root.backend.requestNewPositionSubmit(
            snapshot.request, snapshot.quoteHash, root.submitRequestId)
    }

    function acceptSubmitResult(result) {
        if (Number(result.requestId || 0) !== root.submitRequestId)
            return
        if (result.schema === "new-position.v2"
                && result.status === "submitted"
                && /^[1-9A-HJ-NP-Za-km-z]{32,44}$/.test(
                    String(result.transactionId || ""))) {
            root.submitting = false
            root.transactionId = result.transactionId
            root.flowErrorCode = ""
            root.contextErrorCode = ""
            root.quoteErrorCode = ""
            const poolProbe = root.pendingSubmitSnapshot.poolProbeRequest || null
            if (String(root.pendingSubmitSnapshot.instruction || "")
                    === "NewDefinition" && poolProbe)
                root.watchPoolCreation(poolProbe, result.deadlineMs)
            root.invalidateQuote()
            root.submitSucceeded()
            return
        }
        root.finishSubmitFailure(result || root.quoteError("wallet_submission_failed"))
    }

    function finishSubmitFailure(result) {
        root.submitting = false
        const hasFreshQuote = result && result.quote
                              && result.quote.schema === "new-position.v2"
        if (hasFreshQuote) {
            root.newPositionQuote = result.quote
            root.quoteLoading = false
            root.quoteStale = false
        }
        root.flowErrorCode = result && result.code
                             ? result.code : "wallet_submission_failed"
        root.submitFailed()
        if (!hasFreshQuote)
            root.scheduleQuote(true, root.pendingQuoteRequest)
    }

    function watchPoolCreation(request, deadlineMs) {
        const key = root.pairKey(request)
        let deadline = Number(deadlineMs)
        if (!isFinite(deadline) || deadline <= 0)
            deadline = 0
        const pending = root.pendingPoolProbes.filter(function(item) {
            return item.key !== key
        })
        pending.push({ "key": key, "request": request, "deadlineMs": deadline })
        root.pendingPoolProbes = pending
        Qt.callLater(root.pollPendingPool)
    }

    function pollPendingPool() {
        if (root.poolProbeInFlight || root.pendingPoolProbes.length === 0
                || !root.backendReady)
            return
        const pending = root.pendingPoolProbes[0]
        root.poolProbeInFlight = true
        root.poolProbeRequestId = ++root.operationSerial
        root.backend.requestNewPositionQuote(pending.request, root.poolProbeRequestId, true)
    }

    function finishPoolProbe(pending, quote) {
        if (!pending)
            return
        root.poolProbeInFlight = false
        root.poolProbeRequestId = 0
        if (quote && quote.schema === "new-position.v2"
                && quote.poolStatus === "active_pool") {
            root.removePendingPool(pending.key)
            if (root.matchesSelectedPair(pending.request)) {
                root.poolActivated(quote)
                root.invalidateQuote()
                root.refreshContext(false)
            }
            return
        }
        if (pending.deadlineMs > 0 && Date.now() >= pending.deadlineMs) {
            root.removePendingPool(pending.key)
            return
        }
        root.rotatePendingPool()
    }

    function pairKey(request) {
        return String(request.tokenAId || "") + ":" + String(request.tokenBId || "")
    }

    function matchesSelectedPair(request) {
        return root.pairKey(root.pendingQuoteRequest.request || {}) === root.pairKey(request)
    }

    function selectedPoolCreationPending() {
        const request = root.pendingQuoteRequest.request || {}
        if (!request.tokenAId || !request.tokenBId)
            return false
        const selected = root.pairKey(request)
        return root.pendingPoolProbes.some(function(item) { return item.key === selected })
    }

    function removePendingPool(key) {
        root.pendingPoolProbes = root.pendingPoolProbes.filter(function(item) {
            return item.key !== key
        })
    }

    function rotatePendingPool() {
        if (root.pendingPoolProbes.length < 2)
            return
        const pending = root.pendingPoolProbes.slice(1)
        pending.push(root.pendingPoolProbes[0])
        root.pendingPoolProbes = pending
    }

    function draftChanged() {
        root.invalidateQuote()
        root.transactionId = ""
        root.flowErrorCode = ""
        root.contextErrorCode = ""
        root.quoteErrorCode = ""
    }

    function invalidateQuote() {
        ++root.quoteSerial
        root.activeQuoteRequestId = 0
        root.quoteDebounce.stop()
        root.quoteLoading = false
        root.quoteStale = true
    }

    function loadingContext() {
        return {
            "schema": "new-position.v2",
            "status": "loading",
            "tokens": [],
            "feeTiers": []
        }
    }

    function quoteError(code) {
        return {
            "schema": "new-position.v2",
            "status": "error",
            "canSubmit": false,
            "code": code,
            "poolStatus": "unavailable_pool",
            "errors": [{
                "code": code,
                "blockingFields": [],
                "details": ({})
            }],
            "warnings": [],
            "accountPreview": []
        }
    }
}
