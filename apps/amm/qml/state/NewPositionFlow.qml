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
        // Create-vs-add routing signal, from the resolvePool read: true = add (pool exists),
        // false = create, undefined = not resolved yet (a new pair, still resolving).
        "poolExists": root.poolExists,
        "errorCode": root.flowErrorCode || root.contextErrorCode
                     || root.quoteErrorCode
    })

    property var newPositionQuote: ({})
    // Whether the selected pair's pool exists (from resolvePool); drives create-vs-add.
    // undefined until the first resolve for the current pair lands.
    property var poolExists: undefined
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

        // Route on pool existence (read the pool account), like the swap card. resolvePool
        // returns the reserves oriented to our requested token order (reserveA is tokenAId's).
        root.runtime.watch(root.backend.resolvePool(built.request.tokenAId, built.request.tokenBId),
            function(pool) {
                if (serial !== root.quoteSerial)
                    return
                if (pool && pool.exists) {
                    root.poolExists = true
                    root.requestAddQuote(serial, built, pool)
                    return
                }
                // resolvePool returns exists:false for BOTH the normal "no pool yet" case and
                // hard failures (no_program_bin, amm_not_initialized, bad_config). Only the
                // former — an empty error or no_pool — is a create-pool signal; surface any other
                // pool.error as a quote error instead of masking it as a create quote (which
                // would hide the backend failure and enable the wrong flow).
                var poolError = pool ? String(pool.error || "") : ""
                if (poolError.length === 0 || poolError === "no_pool") {
                    root.poolExists = false
                    root.requestCreateQuote(serial, built)
                } else {
                    // Hard failure: leave poolExists unresolved so the form doesn't drop into
                    // create mode on a backend/config error.
                    root.poolExists = undefined
                    root.quoteLoading = false
                    root.quoteStale = false
                    root.quoteErrorCode = ""
                    root.newPositionQuote = root.quoteError(poolError)
                }
            },
            function(error) {
                if (serial !== root.quoteSerial)
                    return
                root.quoteLoading = false
                root.quoteStale = true
                root.quoteErrorCode = "backend_error"
            })
    }

    // Add-liquidity preview via the lean addLiquidityQuote; reserves + fee come from the
    // resolvePool read. The result is assembled into the shape the form already consumes.
    function requestAddQuote(serial, built, pool) {
        root.runtime.watch(root.backend.addLiquidityQuote({
            "tokenAId": built.request.tokenAId,
            "tokenBId": built.request.tokenBId,
            "maxAmountARaw": built.request.maxAmountARaw,
            "maxAmountBRaw": built.request.maxAmountBRaw,
            "slippageBps": built.request.slippageBps
        }),
            function(quote) {
                if (serial !== root.quoteSerial)
                    return
                root.quoteLoading = false
                root.quoteStale = false
                root.quoteErrorCode = ""
                if (quote && quote.status === "ok")
                    root.newPositionQuote = root.assembleAddQuote(built, pool, quote)
                else
                    root.newPositionQuote = root.quoteError((quote && quote.error) || "backend_error")
            },
            function(error) {
                if (serial !== root.quoteSerial)
                    return
                root.quoteLoading = false
                root.quoteStale = true
                root.quoteErrorCode = "backend_error"
            })
    }

    // Create-pool preview via the lean createPoolQuote (dual-mode: price-only returns the
    // minimum opening deposit; supplied amounts return the actual). Assembled into the
    // missing-pool shape the form consumes. built.request carries the price (+ amounts once
    // the user edits past the minimum), so it can be forwarded as-is.
    function requestCreateQuote(serial, built) {
        root.runtime.watch(root.backend.createPoolQuote(built.request),
            function(quote) {
                if (serial !== root.quoteSerial)
                    return
                root.quoteLoading = false
                root.quoteStale = false
                root.quoteErrorCode = ""
                if (quote && quote.status === "ok")
                    root.newPositionQuote = root.assembleCreateQuote(built, quote)
                else
                    root.newPositionQuote = root.quoteError((quote && quote.error) || "backend_error")
            },
            function(error) {
                if (serial !== root.quoteSerial)
                    return
                root.quoteLoading = false
                root.quoteStale = true
                root.quoteErrorCode = "backend_error"
            })
    }

    // Maps createPoolQuote into the quote shape NewPositionForm reads for a missing pool.
    // Amounts are in the request's (canonical) order, matching the form's displayIsCanonical
    // mapping; minimumAmount* is what the form validates the entered deposit against.
    function assembleCreateQuote(built, quote) {
        return {
            "status": "ok",
            "tokenAId": built.request.tokenAId,
            "tokenBId": built.request.tokenBId,
            "actualAmountARaw": String(quote.actualAmountARaw || "0"),
            "actualAmountBRaw": String(quote.actualAmountBRaw || "0"),
            "minimumAmountARaw": String(quote.minimumAmountARaw || "0"),
            "minimumAmountBRaw": String(quote.minimumAmountBRaw || "0"),
            "expectedLpRaw": String(quote.expectedLpRaw || "0"),
            "lockedLpRaw": String(quote.lockedLpRaw || "0"),
            "priceRaw": String(quote.priceRaw || "0")
        }
    }

    // Maps addLiquidityQuote + the pool read into the quote shape NewPositionForm reads for an
    // active pool. Amounts/reserves are in the request's (canonical) order, matching the form's
    // displayIsCanonical mapping; minimumLpRaw is the slippage floor the module computed.
    function assembleAddQuote(built, pool, quote) {
        return {
            "status": "ok",
            "tokenAId": built.request.tokenAId,
            "tokenBId": built.request.tokenBId,
            "actualAmountARaw": String(quote.amountARaw || "0"),
            "actualAmountBRaw": String(quote.amountBRaw || "0"),
            "expectedLpRaw": String(quote.expectedLpRaw || "0"),
            "minimumLpRaw": String(quote.minimumLpRaw || "0"),
            "reserveARaw": String(pool.reserveA || "0"),
            "reserveBRaw": String(pool.reserveB || "0"),
            "poolFeeBps": pool.feeBps,
            "priceRaw": String(quote.priceRaw || "0")
        }
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

        // Route by pool state: creation (priceRaw is set only on the missing-pool
        // path) goes through createPool; the active-pool branch through addLiquidity. Both
        // mint a fresh LP holding then submit via the lean module ops (hex ids,
        // caller-provided accounts). Quoting for both branches is now on the lean ops
        // (createPoolQuote / addLiquidityQuote), routed by resolvePool in requestQuoteNow.
        if (snapshot.request.priceRaw !== undefined)
            root.createPool(snapshot)
        else
            root.addLiquidity(snapshot)
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

    // Add liquidity to an existing pool via the new addLiquidity op. Like createPool a fresh
    // LP holding receives the minted LP, so create one then submit. The submit reuses the
    // addLiquidityQuote result (maxAmounts + minimumLpRaw) carried on the snapshot. No
    // confirmation poll yet.
    function addLiquidity(snapshot) {
        root.runtime.watch(root.backend.createAccountPublic(),
            function(lpId) {
                if (!lpId || String(lpId).length === 0) {
                    root.finishSubmitFailure(root.quoteError("wallet_submission_failed"))
                    return
                }
                root.submitAddLiquidity(snapshot, String(lpId))
            },
            function(error) {
                root.finishSubmitFailure(root.quoteError("wallet_submission_failed"))
            })
    }

    function submitAddLiquidity(snapshot, lpHoldingId) {
        var request = {
            "tokenAId": snapshot.request.tokenAId,
            "tokenBId": snapshot.request.tokenBId,
            "holdingAId": snapshot.holdingAId,
            "holdingBId": snapshot.holdingBId,
            "lpHoldingId": lpHoldingId,
            "maxAmountARaw": snapshot.request.maxAmountARaw,
            "maxAmountBRaw": snapshot.request.maxAmountBRaw,
            "minLpRaw": snapshot.minLpRaw,
            // u64-max sentinel = no deadline, same as the swap submits.
            "deadlineMs": "18446744073709551615"
        }
        root.runtime.watch(root.backend.addLiquidity(request),
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
        const code = result && result.code ? result.code : "wallet_submission_failed"
        root.flowErrorCode = code
        root.submitFailed()
        // The lean submit ops report only a status/error (never a re-quote), so a failure
        // always re-quotes to refresh state against the current pool.
        root.scheduleQuote(true, root.pendingQuoteRequest)
    }

    function draftChanged() {
        root.invalidateQuote()
        root.transactionId = ""
        root.flowErrorCode = ""
        root.contextErrorCode = ""
        root.quoteErrorCode = ""
    }

    // The selected pair changed, so the pool it maps to is unknown until the next
    // resolvePool. Clearing poolExists drops both activePool/missingPool to false, which
    // keeps requestQuote from short-circuiting an active pool's empty-amount probe and lets
    // buildQuoteRequest emit the price+probe request a fresh selection would — reloading the
    // reserves (add) or the opening minimum (create) for the new pair.
    function resetPoolExistence() {
        root.poolExists = undefined
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
            "tokens": []
        }
    }

    function quoteError(code) {
        return {
            "status": "error",
            "code": code,
            "errors": [{
                "code": code,
                "blockingFields": [],
                "details": ({})
            }]
        }
    }
}
