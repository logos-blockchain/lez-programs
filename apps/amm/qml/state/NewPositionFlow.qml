import QtQml

QtObject {
    id: root

    property var backend: null
    property var runtime: null
    property bool active: false

    readonly property bool walletStateReady: root.backend !== null
                                               && root.backend.walletStateReady === true
    readonly property var viewState: ({
        "quote": root.newPositionQuote,
        "quoteLoading": root.quoteLoading,
        "quoteStale": root.quoteStale,
        "submitting": root.submitting,
        "transactionId": root.transactionId,
        // Create-vs-add routing signal, from the resolvePool read: true = add (pool exists),
        // false = create, undefined = not resolved yet (a new pair, still resolving).
        "poolExists": root.poolExists,
        "errorCode": root.flowErrorCode || root.quoteErrorCode
    })

    property var newPositionQuote: ({})
    // Whether the selected pair's pool exists (from resolvePool); drives create-vs-add.
    // undefined until the first resolve for the current pair lands.
    property var poolExists: undefined
    property int quoteSerial: 0
    property bool quoteLoading: false
    property bool quoteStale: true
    property bool submitting: false
    property string transactionId: ""
    property string flowErrorCode: ""
    property string quoteErrorCode: ""
    property var pendingQuoteRequest: ({ "ok": false, "request": ({}) })

    signal quoteRefreshRequested(bool immediate)
    signal submitSucceeded
    signal submitFailed

    objectName: "newPositionFlow"

    property Timer quoteDebounce: Timer {
        interval: 250
        repeat: false
        onTriggered: root.requestQuoteNow(root.quoteSerial)
    }

    onWalletStateReadyChanged: root.invalidateQuote()

    onActiveChanged: {
        if (!root.active)
            return
        Qt.callLater(function() {
            if (root.active && root.walletStateReady)
                root.quoteRefreshRequested(true)
        })
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

        // Route on pool existence (read the pool account), like the swap card.
        // resolvePoolAccount returns status:"ok" with reserves oriented to our requested token
        // order (reserveA is tokenAId's), or status:"error" (no_pool / hard failure).
        root.runtime.watch(root.backend.resolvePoolAccount(built.request.tokenAId, built.request.tokenBId),
            function(pool) {
                if (serial !== root.quoteSerial)
                    return
                if (pool && pool.status === "ok") {
                    root.poolExists = true
                    root.requestAddQuote(serial, built, pool)
                    return
                }
                // A status:"error" result is EITHER the normal "no pool yet" case (error
                // "no_pool") or a hard failure (no_program_bin, amm_not_initialized, bad_config).
                // Only the former is a create-pool signal; surface any other pool.error as a quote
                // error instead of masking it as a create quote (which would enable the wrong flow).
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
            "maxAmountA": built.request.maxAmountA,
            "maxAmountB": built.request.maxAmountB,
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
            "actualAmountA": String(quote.actualAmountA || "0"),
            "actualAmountB": String(quote.actualAmountB || "0"),
            "minimumAmountA": String(quote.minimumAmountA || "0"),
            "minimumAmountB": String(quote.minimumAmountB || "0"),
            "expectedLp": String(quote.expectedLp || "0"),
            "lockedLp": String(quote.lockedLp || "0"),
            "price": String(quote.price || "0")
        }
    }

    // Maps addLiquidityQuote + the pool read into the quote shape NewPositionForm reads for an
    // active pool. Amounts/reserves are in the request's (canonical) order, matching the form's
    // displayIsCanonical mapping; minimumLp is the slippage floor the module computed.
    function assembleAddQuote(built, pool, quote) {
        return {
            "status": "ok",
            "tokenAId": built.request.tokenAId,
            "tokenBId": built.request.tokenBId,
            "actualAmountA": String(quote.amountA || "0"),
            "actualAmountB": String(quote.amountB || "0"),
            "expectedLp": String(quote.expectedLp || "0"),
            "minimumLp": String(quote.minimumLp || "0"),
            "reserveA": String(pool.reserveA || "0"),
            "reserveB": String(pool.reserveB || "0"),
            "poolFeeBps": pool.feeBps,
            "price": String(quote.price || "0"),
            // The pool's LP token (base58, matching the holdings' definitionId) so the form
            // can offer the wallet's existing LP holdings as the mint destination.
            "lpDefinitionId": String(quote.lpDefinitionId || "")
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

        // Route by pool state: creation (price is set only on the missing-pool
        // path) goes through createPool; the active-pool branch through addLiquidity. Both
        // mint a fresh LP holding then submit via the lean module ops (hex ids,
        // caller-provided accounts). Quoting for both branches is now on the lean ops
        // (createPoolQuote / addLiquidityQuote), routed by resolvePool in requestQuoteNow.
        if (snapshot.request.price !== undefined)
            root.createPool(snapshot)
        else
            root.addLiquidity(snapshot)
    }

    // Create a pool via the new createPool op. A new pool has no pre-existing LP
    // holding, so the caller provides a fresh account: create one, then submit. No
    // confirmation poll yet (transactionStatus is pending an upstream dependency).
    function createPool(snapshot) {
        // If the user picked an existing LP holding, mint straight into it; otherwise create a
        // fresh account first. Create-pool has no existing LP, so it always takes the fresh path.
        if (!snapshot.createLpNew && String(snapshot.lpHoldingId || "").length > 0) {
            root.submitCreatePool(snapshot, String(snapshot.lpHoldingId))
            return
        }
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
            "amountA": snapshot.request.amountA,
            "amountB": snapshot.request.amountB,
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

    // Add liquidity to an existing pool via the addLiquidity op. The minted LP goes to the
    // holding the user chose (createLpNew=false), consolidating an existing position — or to a
    // fresh account when they opt to create one. The submit reuses the addLiquidityQuote result
    // (maxAmounts + minimumLp) carried on the snapshot. No confirmation poll yet.
    function addLiquidity(snapshot) {
        if (!snapshot.createLpNew && String(snapshot.lpHoldingId || "").length > 0) {
            root.submitAddLiquidity(snapshot, String(snapshot.lpHoldingId))
            return
        }
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
            "maxAmountA": snapshot.request.maxAmountA,
            "maxAmountB": snapshot.request.maxAmountB,
            "minLp": snapshot.minLp,
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
