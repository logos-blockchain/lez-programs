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

    signal tokenResolutionFinished(bool finalResponse)
    signal tokenResolutionFailed(string code)
    signal quoteRefreshRequested(bool immediate)
    signal submitSucceeded
    signal submitFailed

    objectName: "newPositionFlow"

    property Timer quoteDebounce: Timer {
        interval: 250
        repeat: false
        onTriggered: root.requestQuoteNow(root.quoteSerial)
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

        // Pool creation (initialPriceRealRaw is set only on the missing-pool path) goes
        // through the new createPool op — hex ids, caller-provided accounts. Add-liquidity
        // keeps the legacy submitNewPosition.
        if (snapshot.request.initialPriceRealRaw !== undefined) {
            root.createPool(snapshot)
            return
        }

        root.runtime.watch(root.backend.submitNewPosition(snapshot.request, snapshot.quoteHash),
            function(result) {
                if (result && result.status === "submitted"
                        && /^[1-9A-HJ-NP-Za-km-z]{32,44}$/.test(
                            String(result.transactionId || ""))) {
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

    // Create a pool via the new createPool op. A new pool has no pre-existing LP
    // holding, so the caller provides a fresh account: create one, then submit. No
    // confirmation poll yet (transactionStatus is pending an upstream dependency).
    function createPool(snapshot) {
        root.runtime.watch(root.backend.createAccountPublic(),
            function(lpId) {
                if (!lpId || String(lpId).length === 0) {
                    root.finishSubmitFailure(root.quoteError("wallet_submission_failed"))
                    return
                }
                root.submitCreatePool(snapshot, String(lpId))
            },
            function(error) {
                root.finishSubmitFailure(root.quoteError("wallet_submission_failed"))
            })
    }

    function submitCreatePool(snapshot, lpHoldingId) {
        var request = {
            "tokenAId": snapshot.request.tokenAId,
            "tokenBId": snapshot.request.tokenBId,
            "holdingAId": snapshot.holdingAId,
            "holdingBId": snapshot.holdingBId,
            "lpHoldingId": lpHoldingId,
            "amountARaw": snapshot.request.amountARaw,
            "amountBRaw": snapshot.request.amountBRaw,
            "feeBps": snapshot.request.feeBps,
            // u64-max sentinel = no deadline, same as the swap submits.
            "deadlineMs": "18446744073709551615"
        }
        root.runtime.watch(root.backend.createPool(request),
            function(result) {
                if (result && result.status === "ok"
                        && String(result.transactionId || "").length > 0) {
                    root.submitting = false
                    root.transactionId = String(result.transactionId)
                    root.flowErrorCode = ""
                    root.contextErrorCode = ""
                    root.quoteErrorCode = ""
                    root.invalidateQuote()
                    root.submitSucceeded()
                    return
                }
                var code = result && result.error ? String(result.error)
                                                  : "wallet_submission_failed"
                root.finishSubmitFailure(root.quoteError(code))
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
