import QtQml

QtObject {
    id: root

    property var backend: null
    property var runtime: null
    property bool active: false

    readonly property bool walletStateReady: root.backend !== null
                                               && root.backend.walletStateReady === true
    readonly property var newPositionContext: root.walletStateReady
                                              && root.backend.newPositionContext
                                              ? root.backend.newPositionContext
                                              : root.loadingContext()
    readonly property var viewState: ({
        "quote": root.newPositionQuote,
        "contextLoading": root.contextLoading || !root.walletStateReady
                          || root.newPositionContext.status === "loading",
        "quoteLoading": root.quoteLoading,
        "quoteStale": root.quoteStale,
        "submitting": root.submitting,
        "poolCreationPending": root.selectedPoolCreationPending(),
        "transactionId": root.transactionId,
        "errorCode": root.flowErrorCode || root.contextErrorCode
                     || root.quoteErrorCode
    })

    property var newPositionQuote: ({})
    property var resolvedTokenIds: []
    property int contextSerial: 0
    property int quoteSerial: 0
    property bool contextLoading: false
    property bool quoteLoading: false
    property bool quoteStale: true
    property bool submitting: false
    property string transactionId: ""
    property string flowErrorCode: ""
    property string contextErrorCode: ""
    property string quoteErrorCode: ""
    property var pendingQuoteRequest: ({ "ok": false, "request": ({}) })
    property var pendingPoolProbes: []
    property bool poolProbeInFlight: false

    signal tokenResolutionFinished(bool finalResponse)
    signal tokenResolutionFailed(string code)
    signal poolActivated(var quote)
    signal quoteRefreshRequested(bool immediate)
    signal submitSucceeded
    signal submitFailed

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

    onNewPositionContextChanged: root.invalidateQuote()

    onWalletStateReadyChanged: {
        ++root.contextSerial
        if (!root.walletStateReady)
            root.contextLoading = false
        root.invalidateQuote()
    }

    onActiveChanged: {
        if (!root.active)
            return
        Qt.callLater(function() {
            if (root.active && root.walletStateReady)
                root.quoteRefreshRequested(true)
        })
    }

    function contextHints(refreshWalletAccounts) {
        const request = root.pendingQuoteRequest.request || {}
        const recent = []
        if (request.tokenAId)
            recent.push(request.tokenAId)
        if (request.tokenBId && request.tokenBId !== request.tokenAId)
            recent.push(request.tokenBId)
        return {
            "recentTokenIds": recent,
            "resolvedTokenIds": root.resolvedTokenIds,
            "refreshWalletAccounts": refreshWalletAccounts === true
        }
    }

    function refreshContext(refreshWalletAccounts, completed) {
        const serial = ++root.contextSerial
        root.contextLoading = true
        if (!root.walletStateReady || root.runtime === null) {
            root.contextLoading = false
            return
        }

        root.runtime.watch(root.backend.refreshNewPositionContext(
                               root.contextHints(refreshWalletAccounts)),
            function() {
                root.finishContextRefresh(serial, completed)
            },
            function(error) {
                root.failContextRefresh(serial)
            })
    }

    function finishContextRefresh(serial, completed) {
        if (serial !== root.contextSerial)
            return
        root.contextLoading = false
        root.contextErrorCode = ""
        Qt.callLater(function() {
            if (serial !== root.contextSerial)
                return
            root.tokenResolutionFinished(true)
            if (completed)
                completed()
        })
    }

    function failContextRefresh(serial) {
        if (serial !== root.contextSerial)
            return
        root.contextLoading = false
        root.contextErrorCode = "backend_error"
        root.tokenResolutionFailed("backend_error")
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
        root.quoteLoading = root.walletStateReady && root.active
        root.quoteDebounce.stop()
        if (!root.walletStateReady || !root.active)
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
        if (!built.ok) {
            root.quoteLoading = false
            return
        }
        if (!root.walletStateReady || !root.active || root.runtime === null) {
            root.quoteLoading = false
            return
        }

        root.runtime.watch(root.backend.quoteNewPosition(built.request),
            function(quote) {
                if (serial !== root.quoteSerial)
                    return
                root.quoteLoading = false
                root.quoteStale = false
                root.quoteErrorCode = ""
                if (!quote || !quote.status)
                    root.newPositionQuote = root.quoteError("backend_error")
                else
                    root.newPositionQuote = quote
            },
            function(error) {
                if (serial !== root.quoteSerial)
                    return
                root.quoteLoading = false
                root.quoteStale = true
                root.quoteErrorCode = "backend_error"
            })
    }

    function confirm(snapshot) {
        if (root.submitting)
            return
        root.submitting = true
        root.flowErrorCode = ""

        if (!root.backend || root.runtime === null) {
            root.finishSubmitFailure(root.quoteError("wallet_unavailable"))
            return
        }

        root.runtime.watch(root.backend.submitNewPosition(snapshot.request, snapshot.quoteHash),
            function(result) {
                if (result && result.status === "submitted"
                        && /^[1-9A-HJ-NP-Za-km-z]{32,44}$/.test(
                            String(result.transactionId || ""))) {
                    if (snapshot.request.initialPriceRealRaw !== undefined)
                        root.watchPoolCreation(snapshot.poolProbeRequest, result.deadlineMs)
                    root.submitting = false
                    root.transactionId = result.transactionId
                    root.flowErrorCode = ""
                    root.contextErrorCode = ""
                    root.quoteErrorCode = ""
                    root.invalidateQuote()
                    root.submitSucceeded()
                    return
                }
                root.finishSubmitFailure(result || root.quoteError("wallet_submission_failed"))
            },
            function(error) {
                root.finishSubmitFailure(root.quoteError("wallet_submission_failed"))
            })
    }

    function finishSubmitFailure(result) {
        root.submitting = false
        const hasFreshQuote = result && result.quote
                              && result.quote.status
        if (hasFreshQuote) {
            root.newPositionQuote = result.quote
            root.quoteLoading = false
            root.quoteStale = false
        }
        const code = result && result.code ? result.code : "wallet_submission_failed"
        root.flowErrorCode = code
        root.submitFailed()
        if (hasFreshQuote)
            return
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
        pending.push({
            "key": key,
            "request": request,
            "deadlineMs": deadline
        })
        root.pendingPoolProbes = pending
        Qt.callLater(root.pollPendingPool)
    }

    function pollPendingPool() {
        if (root.poolProbeInFlight || root.pendingPoolProbes.length === 0
                || !root.walletStateReady || root.runtime === null) {
            return
        }
        const pending = root.pendingPoolProbes[0]
        root.poolProbeInFlight = true
        root.runtime.watch(root.backend.quoteNewPosition(pending.request),
            function(quote) {
                root.finishPoolProbe(pending, quote)
            },
            function(error) {
                root.finishPoolProbe(pending, null)
            })
    }

    function finishPoolProbe(pending, quote) {
        root.poolProbeInFlight = false
        if (quote && quote.poolStatus === "active_pool") {
            root.removePendingPool(pending.key)
            if (root.matchesSelectedPair(pending.request)) {
                root.poolActivated(quote)
                root.invalidateQuote()
                root.refreshContext(true)
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
        return root.pendingPoolProbes.some(function(item) {
            return item.key === selected
        })
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
        root.quoteDebounce.stop()
        root.quoteLoading = false
        root.quoteStale = true
    }

    function loadingContext() {
        return {
            "status": "loading",
            "tokens": [],
            "feeTiers": []
        }
    }

    function quoteError(code) {
        return {
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
