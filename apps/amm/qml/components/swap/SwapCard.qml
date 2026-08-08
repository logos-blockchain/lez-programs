import QtQuick 2.15
import QtQuick.Controls 2.15
import QtQuick.Layouts 1.15
import "../shared"
import "../../state"

// The real swap UI: two token inputs (sell/buy), a token picker (backed by
// AmmUiBackend::tokenList()/TOKENS_CONFIG via SwapPage). Editing either side
// server-quotes that direction (swapExactInQuote / swapExactOutQuote) and the
// matching submit slot (swapExactInput / swapExactOutput) runs on confirm — see
// apps/amm/src/AmmUiBackend.rep for the exact contract.
Rectangle {
    id: root

    property var theme
    property var tokens: []
    // Real backend replica (logos.module("amm_ui")), wired from SwapPage.
    property var backend: null

    // The wallet's token holdings (backend.tokenHoldings()), fed to each slot's
    // account selector; the chosen input/output holding ids drive the submit.
    property var holdings: []
    readonly property string sellHolding: sellTokenInput.selectedHoldingId
    readonly property string buyHolding: buyTokenInput.selectedHoldingId

    property var sellToken: null
    property var buyToken: null
    property string sellInput: ""
    property string buyInput: ""
    property string editingSide: "sell"
    property real slippageTolerancePercent: 0.5

    // ── Pool resolution (backend.resolvePool) ───────────────────────────────
    // Existence and fee drive the UI; the swap quotes read the pool and
    // price/orient the swap server-side, so the client no longer prices against
    // the reserves. The raw reserves are still surfaced (observability only, not
    // used for pricing) so the e2e test can assert they changed on-chain after a
    // swap.
    property bool poolLoading: false
    property bool poolResolved: false
    property bool poolExists: false
    property int poolFeeBps: 30
    property string poolReserveA: "0"
    property string poolReserveB: "0"
    property string poolError: ""

    // ── Exact-input quote (backend.swapExactInQuote) ────────────────────────
    property bool quoteInLoading: false
    property string quoteInError: ""
    property string quoteExpectedOutRaw: "0"
    property string quoteMinReceivedRaw: "0"
    property int quotePriceImpactBps: 0

    // ── Exact-output quote (backend.swapExactOutQuote) ──────────────────────
    property bool quoteOutLoading: false
    property string quoteOutError: ""
    property string quoteRequiredInRaw: "0"
    property string quoteMaxInRaw: "0"
    property int quoteOutPriceImpactBps: 0

    // ── Swap submission (backend.swapExactInput) ────────────────────────────
    property bool swapInProgress: false
    property string swapError: ""

    signal requestTokenSelect(string side)
    signal submitRequested(var snapshot)
    // Emitted after a real swapExactInput call completes.
    signal swapSucceeded(var result)
    signal swapFailed(string message)

    DummySwapState {
        id: swapState
        feeBps: root.poolFeeBps
    }

    function setToken(side, token) {
        if (side === "sell") root.sellToken = token
        else root.buyToken = token
    }

    function resetAmounts() {
        root.sellInput = ""
        root.buyInput = ""
        root.editingSide = "sell"
    }

    // ── Pool resolution ──────────────────────────────────────────────────────
    Timer {
        id: resolveDebounce
        interval: 500
        repeat: false
        onTriggered: root.doResolvePool()
    }

    function requestResolve() {
        root.poolResolved = false
        root.poolExists = false
        root.poolError = ""
        if (root.sellToken && root.buyToken)
            resolveDebounce.restart()
        else
            resolveDebounce.stop()
    }

    onSellTokenChanged: { root.requestResolve(); root.requestQuoteIn(); root.requestQuoteOut() }
    onBuyTokenChanged: { root.requestResolve(); root.requestQuoteIn(); root.requestQuoteOut() }
    onSellInputChanged: root.requestQuoteIn()
    onBuyInputChanged: root.requestQuoteOut()
    onEditingSideChanged: { root.requestQuoteIn(); root.requestQuoteOut() }
    onSlippageTolerancePercentChanged: { root.requestQuoteIn(); root.requestQuoteOut() }

    function doResolvePool() {
        if (!root.backend || !root.sellToken || !root.buyToken)
            return

        // Capture the pair this request is for. resolvePool callbacks can arrive
        // out of order: if the user switches tokens while an earlier resolve is
        // still in flight, its (stale) callback must NOT overwrite the current
        // pair's pool state — that would corrupt the preview and the submitted
        // min_out. A stale callback also leaves poolLoading alone, since the
        // newer in-flight request owns it.
        var reqSell = root.sellToken.definitionId
        var reqBuy = root.buyToken.definitionId
        function isStale() {
            return !root.sellToken || !root.buyToken
                || root.sellToken.definitionId !== reqSell
                || root.buyToken.definitionId !== reqBuy
        }

        root.poolLoading = true
        logos.watch(root.backend.resolvePool(reqSell, reqBuy),
            function (pool) {
                if (isStale())
                    return
                root.poolLoading = false
                root.poolResolved = true
                root.poolExists = !!(pool && pool.exists)
                root.poolReserveA = (pool && pool.reserveA) || "0"
                root.poolReserveB = (pool && pool.reserveB) || "0"
                // feeBps === 0 is a legitimate zero-fee pool; only fall back
                // to the 30bps default when the backend didn't send a value.
                root.poolFeeBps = (pool && pool.feeBps !== undefined) ? pool.feeBps : 30
                root.poolError = (pool && pool.error && pool.error !== "no_pool") ? pool.error : ""
            },
            function (error) {
                if (isStale())
                    return
                console.warn("resolvePool error:", error)
                root.poolLoading = false
                root.poolResolved = true
                root.poolExists = false
                root.poolError = qsTr("Failed to resolve pool: %1").arg(error)
            })
    }

    // ── Exact-input quote ──────────────────────────────────────────────────────
    Timer {
        id: quoteInDebounce
        interval: 350
        repeat: false
        onTriggered: root.doQuoteIn()
    }

    function resetQuoteIn() {
        root.quoteExpectedOutRaw = "0"
        root.quoteMinReceivedRaw = "0"
        root.quotePriceImpactBps = 0
    }

    function requestQuoteIn() {
        root.quoteInError = ""
        // Only the exact-input (Sell) direction is server-quoted; the Buy
        // direction is still a local preview (see DummySwapState).
        if (root.backend && root.editingSide === "sell" && root.sellToken && root.buyToken
            && root.parsedSellInput > 0) {
            // Invalidate the previous quote up front: while a re-quote is pending
            // (debounce + in-flight), the stale expected-out / min_out must not be
            // shown or submittable. quoteInLoading gates canSubmit until the fresh
            // quote lands. Gating on backend here avoids setting the loading flag
            // when doQuoteIn would only bail — which would leave it stuck.
            root.resetQuoteIn()
            root.quoteInLoading = true
            quoteInDebounce.restart()
        } else {
            quoteInDebounce.stop()
            root.quoteInLoading = false
            root.resetQuoteIn()
        }
    }

    function doQuoteIn() {
        if (!root.backend || root.editingSide !== "sell"
            || !root.sellToken || !root.buyToken || root.parsedSellInput <= 0) {
            root.quoteInLoading = false
            return
        }

        // Capture the request identity: quote callbacks can arrive out of order,
        // so a stale one (tokens or the typed amount changed since) must not
        // overwrite the current preview or the submitted min_out.
        var reqSell = root.sellToken.definitionId
        var reqBuy = root.buyToken.definitionId
        var reqAmount = root.sellInput
        function isStale() {
            return root.editingSide !== "sell"
                || !root.sellToken || !root.buyToken
                || root.sellToken.definitionId !== reqSell
                || root.buyToken.definitionId !== reqBuy
                || root.sellInput !== reqAmount
        }

        var slippageBps = Math.round(root.slippageTolerancePercent * 100)
        root.quoteInLoading = true
        logos.watch(root.backend.swapExactInQuote(reqSell, reqBuy, reqAmount, slippageBps),
            function (quote) {
                if (isStale())
                    return
                root.quoteInLoading = false
                if (quote && quote.status === "ok") {
                    root.quoteExpectedOutRaw = quote.expectedOutRaw || "0"
                    root.quoteMinReceivedRaw = quote.minReceivedRaw || "0"
                    root.quotePriceImpactBps = quote.priceImpactBps || 0
                    root.quoteInError = ""
                } else {
                    root.resetQuoteIn()
                    // no_pool is surfaced via the pool status text, not as an error.
                    var code = (quote && quote.error) || "backend_error"
                    root.quoteInError = code === "no_pool" ? "" : code
                }
            },
            function (error) {
                if (isStale())
                    return
                console.warn("swapExactInQuote error:", error)
                root.quoteInLoading = false
                root.resetQuoteIn()
                root.quoteInError = String(error)
            })
    }

    // ── Exact-output quote ─────────────────────────────────────────────────────
    Timer {
        id: quoteOutDebounce
        interval: 350
        repeat: false
        onTriggered: root.doQuoteOut()
    }

    function resetQuoteOut() {
        root.quoteRequiredInRaw = "0"
        root.quoteMaxInRaw = "0"
        root.quoteOutPriceImpactBps = 0
    }

    // The Buy field is free-form (not digitsOnly like Sell), so its text is
    // normalized to a base-units integer before quoting: trim whitespace and
    // accept only a positive run of digits. Decimals / exponents / signs / empty
    // yield "" (invalid), so the backend call is skipped rather than forwarding an
    // amount that would come back as a confusing quote failure.
    function normalizedAmountOut() {
        var s = String(root.buyInput).trim()
        return (/^\d+$/.test(s) && /[1-9]/.test(s)) ? s : ""
    }

    function requestQuoteOut() {
        root.quoteOutError = ""
        // Mirror of requestQuoteIn for the Buy direction: price the input needed
        // for the typed output. Invalidate the previous quote up front so a stale
        // required-in isn't shown while the re-quote is pending. Invalid input
        // (see normalizedAmountOut) takes the else branch, clearing the loading
        // flag so it can't get stuck.
        if (root.editingSide === "buy" && root.sellToken && root.buyToken
            && root.normalizedAmountOut() !== "") {
            root.resetQuoteOut()
            root.quoteOutLoading = true
            quoteOutDebounce.restart()
        } else {
            quoteOutDebounce.stop()
            root.quoteOutLoading = false
            root.resetQuoteOut()
        }
    }

    function doQuoteOut() {
        var amountOut = root.normalizedAmountOut()
        if (!root.backend || root.editingSide !== "buy"
            || !root.sellToken || !root.buyToken || amountOut === "") {
            root.quoteOutLoading = false
            return
        }

        var reqSell = root.sellToken.definitionId
        var reqBuy = root.buyToken.definitionId
        // Staleness is keyed on the raw field text (a further edit re-quotes),
        // while the backend gets the normalized base-units amount.
        var reqInput = root.buyInput
        function isStale() {
            return root.editingSide !== "buy"
                || !root.sellToken || !root.buyToken
                || root.sellToken.definitionId !== reqSell
                || root.buyToken.definitionId !== reqBuy
                || root.buyInput !== reqInput
        }

        var slippageBps = Math.round(root.slippageTolerancePercent * 100)
        root.quoteOutLoading = true
        // tokenIn is the sold token (sell), tokenOut is the bought token (buy).
        logos.watch(root.backend.swapExactOutQuote(reqSell, reqBuy, amountOut, slippageBps),
            function (quote) {
                if (isStale())
                    return
                root.quoteOutLoading = false
                if (quote && quote.status === "ok") {
                    root.quoteRequiredInRaw = quote.requiredInRaw || "0"
                    root.quoteMaxInRaw = quote.maxInRaw || "0"
                    root.quoteOutPriceImpactBps = quote.priceImpactBps || 0
                    root.quoteOutError = ""
                } else {
                    root.resetQuoteOut()
                    // no_pool is surfaced via the pool status text, not as an error.
                    var code = (quote && quote.error) || "backend_error"
                    root.quoteOutError = code === "no_pool" ? "" : code
                }
            },
            function (error) {
                if (isStale())
                    return
                console.warn("swapExactOutQuote error:", error)
                root.quoteOutLoading = false
                root.resetQuoteOut()
                root.quoteOutError = String(error)
            })
    }

    readonly property real parsedSellInput: {
        var amt = parseFloat(sellInput)
        return isNaN(amt) || amt < 0 ? 0 : amt
    }

    readonly property real parsedBuyInput: {
        var amt = parseFloat(buyInput)
        return isNaN(amt) || amt < 0 ? 0 : amt
    }

    // The computed side comes from the server quote: exact-input (Sell) yields the
    // expected output, exact-output (Buy) yields the required input. Number() may
    // lose precision on large base-unit values, so these drive gating only — the
    // exact figures shown and submitted come from the raw quote strings directly.
    readonly property real parsedSellAmount: editingSide === "sell"
        ? parsedSellInput
        : (Number(root.quoteRequiredInRaw) || 0)

    readonly property real parsedBuyAmount: editingSide === "buy"
        ? parsedBuyInput
        : (Number(root.quoteExpectedOutRaw) || 0)

    readonly property real feeAmount: swapState.feeAmount(parsedSellAmount)

    // Slippage bound: exact input floors the received amount (Min received), exact
    // output caps the spent amount (Maximum sent). Both come from the quote.
    readonly property string boundLabel: editingSide === "sell" ? qsTr("Min received") : qsTr("Maximum sent")
    // The quote's exact-integer bound, verbatim (no Number()/double round-trip,
    // which would lose precision on large u128 values and diverge from execution):
    // min received (exact input) or max sent (exact output).
    readonly property string boundRaw: editingSide === "sell" ? root.quoteMinReceivedRaw : root.quoteMaxInRaw
    readonly property string boundSymbol: editingSide === "sell"
        ? (buyToken ? buyToken.symbol : "")
        : (sellToken ? sellToken.symbol : "")

    readonly property real priceImpactPercent: editingSide === "sell"
        ? root.quotePriceImpactBps / 100
        : root.quoteOutPriceImpactBps / 100

    readonly property string swapModeText: editingSide === "buy" ? qsTr("Exact output") : qsTr("Exact input")

    readonly property bool hasAmount: editingSide === "sell" ? parsedSellInput > 0 : parsedBuyInput > 0
    readonly property bool tokensSelected: sellToken !== null && buyToken !== null
    // Exact output only: the module reports output_exceeds_liquidity when the
    // requested output is at least the pool's reserve. Exact input can never
    // exceed the reserve, so it has no such case.
    readonly property bool outputExceedsLiquidity: editingSide === "buy" && root.quoteOutError === "output_exceeds_liquidity"
    // Loading flag for whichever direction the user is editing.
    readonly property bool quoteLoading: editingSide === "sell" ? root.quoteInLoading : root.quoteOutLoading
    // True only when THIS app's wallet is connected. The backend also enforces
    // this before submitting, but gate the UI too so a disconnected app never
    // even initiates a swap against the shared wallet.
    readonly property bool walletOpen: root.backend !== null && root.backend.isWalletOpen

    // Both directions are submittable: exact input via swapExactInput, exact
    // output via swapExactOutput. The typed side and the quoted side must both be
    // positive (a fresh quote has landed) and the active quote must not be
    // in-flight.
    readonly property bool canSubmit: tokensSelected && hasAmount
                                       && parsedSellAmount > 0 && parsedBuyAmount > 0
                                       && root.poolResolved && root.poolExists
                                       && !outputExceedsLiquidity && !root.swapInProgress
                                       && !root.quoteLoading && root.walletOpen
                                       // Both holdings must be chosen (auto-selected
                                       // when the wallet has exactly one per token).
                                       && root.sellHolding.length > 0
                                       && root.buyHolding.length > 0

    readonly property string submitButtonText: {
        if (!tokensSelected) return qsTr("Select tokens")
        if (root.swapInProgress) return qsTr("Submitting…")
        if (!hasAmount) return qsTr("Enter an amount")
        if (root.poolLoading || !root.poolResolved) return qsTr("Resolving pool…")
        if (!root.poolExists) return qsTr("No pool / no liquidity")
        if (root.quoteLoading) return qsTr("Quoting…")
        if (outputExceedsLiquidity) return qsTr("Insufficient liquidity")
        if (parsedSellAmount <= 0 || parsedBuyAmount <= 0) return qsTr("Amount too small")
        if (!root.walletOpen) return qsTr("Connect wallet to swap")
        if (root.sellHolding.length === 0 || root.buyHolding.length === 0)
            return qsTr("Select token accounts")
        return qsTr("Swap")
    }

    readonly property string poolStatusText: {
        if (!root.backend) return qsTr("Wallet backend not ready.")
        if (!tokensSelected) return ""
        if (root.poolLoading) return qsTr("Looking up pool…")
        if (root.poolError.length > 0) return root.poolError
        if (root.poolResolved && !root.poolExists) return qsTr("No pool / no liquidity for this pair.")
        if (root.outputExceedsLiquidity) return qsTr("Not enough liquidity for that output amount.")
        if (root.quoteInError.length > 0) return qsTr("Quote failed: %1").arg(root.quoteInError)
        if (root.quoteOutError.length > 0) return qsTr("Quote failed: %1").arg(root.quoteOutError)
        return ""
    }

    // The computed side is shown as the quote's exact-integer string verbatim (no
    // double round-trip): the required input in the Buy direction, the expected
    // output in the Sell direction.
    readonly property string sellDisplay: editingSide === "sell"
        ? sellInput
        : ((root.quoteRequiredInRaw && root.quoteRequiredInRaw !== "0") ? root.quoteRequiredInRaw : "")

    readonly property string buyDisplay: editingSide === "buy"
        ? buyInput
        : ((root.quoteExpectedOutRaw && root.quoteExpectedOutRaw !== "0") ? root.quoteExpectedOutRaw : "")

    // Confirmation-dialog preview. The typed side is exact; the quoted side and
    // the slippage bound come from the quote's exact-integer strings. boundValue
    // is the min received (exact input) or the max sent (exact output).
    function buildSnapshot() {
        var isExactIn = editingSide === "sell"
        return {
            "sellToken": sellToken ? sellToken.symbol : "",
            "buyToken": buyToken ? buyToken.symbol : "",
            "sellAmount": isExactIn ? root.sellInput : root.quoteRequiredInRaw,
            "buyAmount": isExactIn ? root.quoteExpectedOutRaw : root.buyInput,
            "boundValue": isExactIn ? root.quoteMinReceivedRaw : root.quoteMaxInRaw,
            "feeAmount": swapState.formatTokenAmount(feeAmount, sellToken ? sellToken.symbol : ""),
            "priceImpactPercent": swapState.formatPercent(priceImpactPercent),
            "priceImpactPercentValue": priceImpactPercent,
            "slippageTolerance": swapState.formatSlippagePercent(slippageTolerancePercent),
            "swapMode": isExactIn ? "swap-exact-input" : "swap-exact-output",
            "swapModeText": swapModeText
        }
    }

    // Called by SwapPage once the user confirms in SwapConfirmationDialog.
    // Submits the real on-chain swap for the tokens/amounts in SwapCard's live
    // state, in whichever direction the user is editing.
    function executeSwap() {
        if (!root.backend || !root.canSubmit)
            return

        root.swapInProgress = true
        root.swapError = ""

        // Max u64 sentinel: "ignore deadline", per AmmUiBackend.rep.
        var deadline = "18446744073709551615"
        var inDef = root.sellToken.definitionId
        var outDef = root.buyToken.definitionId
        // Holdings come from the per-slot account selector, not the token config.
        var inHolding = root.sellHolding
        var outHolding = root.buyHolding

        // The on-chain guard is the quote's exact-integer bound: the exact-input
        // floor (minReceivedRaw) or the exact-output ceiling (maxInRaw). The typed
        // side (sellInput / buyInput) is the exact amount for that direction.
        var pending = root.editingSide === "sell"
            ? root.backend.swapExactInput(inDef, outDef, inHolding, outHolding,
                                          root.sellInput, root.quoteMinReceivedRaw, deadline)
            : root.backend.swapExactOutput(inDef, outDef, inHolding, outHolding,
                                           root.buyInput, root.quoteMaxInRaw, deadline)

        logos.watch(pending,
            function (txHash) {
                root.swapInProgress = false
                if (txHash && txHash.length > 0) {
                    root.swapSucceeded({
                        "txHash": txHash,
                        "sellToken": root.sellToken.symbol,
                        "buyToken": root.buyToken.symbol
                    })
                    root.resetAmounts()
                    resolveDebounce.restart()
                } else {
                    root.swapError = qsTr("Swap failed (empty response from sequencer).")
                    root.swapFailed(root.swapError)
                }
            },
            function (error) {
                console.warn("swap error:", error)
                root.swapInProgress = false
                root.swapError = qsTr("Swap error: %1").arg(error)
                root.swapFailed(root.swapError)
            })
    }

    radius: 24
    color: theme.colors.cardBg
    border.color: theme.colors.border
    border.width: 1
    implicitWidth: 480
    implicitHeight: cardLayout.implicitHeight + 16

    Behavior on color { ColorAnimation { duration: 300 } }

    ColumnLayout {
        id: cardLayout
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.margins: 8
        spacing: 0

        TokenInput {
            id: sellTokenInput
            Layout.fillWidth: true
            theme: root.theme
            label: "Sell"
            inputObjectName: "swapSellInput"
            buttonObjectName: "swapSellTokenButton"
            selectorObjectName: "swapSellAccountSelector"
            amount: root.sellDisplay
            token: root.sellToken
            holdings: root.holdings
            active: root.editingSide === "sell"
            // Sell amount is sent to the backend as a raw base-units integer
            // string; reject fractional entry rather than fail opaquely.
            digitsOnly: true
            onInputEdited: function(v) {
                root.sellInput = v
                if (root.editingSide !== "sell") root.editingSide = "sell"
            }
            onTokenClicked: root.requestTokenSelect("sell")
        }

        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: 40

            Rectangle {
                anchors.verticalCenter: parent.verticalCenter
                anchors.left: parent.left
                anchors.right: parent.right
                height: 1
                color: theme.colors.divider
            }

            Rectangle {
                anchors.centerIn: parent
                width: 36; height: 36; radius: 18
                color: swapHover.containsMouse ? theme.colors.panelHoverBg : theme.colors.panelBg
                border.color: theme.colors.borderStrong
                border.width: 1
                Behavior on color { ColorAnimation { duration: 120 } }

                Text {
                    anchors.centerIn: parent
                    text: "↓"
                    color: theme.colors.textPrimary
                    font.pixelSize: 16
                }

                MouseArea {
                    id: swapHover
                    anchors.fill: parent
                    hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: {
                        var tmp = root.sellToken
                        root.sellToken = root.buyToken
                        root.buyToken = tmp
                    }
                }
            }
        }

        TokenInput {
            id: buyTokenInput
            Layout.fillWidth: true
            theme: root.theme
            label: "Buy"
            inputObjectName: "swapBuyInput"
            buttonObjectName: "swapBuyTokenButton"
            selectorObjectName: "swapBuyAccountSelector"
            amount: root.buyDisplay
            token: root.buyToken
            holdings: root.holdings
            active: root.editingSide === "buy"
            // Exact-output amount is sent to the backend as a raw base-units
            // integer string; reject fractional entry rather than fail opaquely.
            digitsOnly: true
            onInputEdited: function(v) {
                root.buyInput = v
                if (root.editingSide !== "buy") root.editingSide = "buy"
            }
            onTokenClicked: root.requestTokenSelect("buy")
        }

        Text {
            Layout.fillWidth: true
            Layout.topMargin: 8
            Layout.leftMargin: 16
            Layout.rightMargin: 16
            visible: root.poolStatusText.length > 0
            text: root.poolStatusText
            color: (root.poolError.length > 0 || (root.poolResolved && !root.poolExists))
                   ? "#F08A76" : theme.colors.textSecondary
            font.pixelSize: 12
            wrapMode: Text.WordWrap
        }

        Text {
            Layout.fillWidth: true
            Layout.leftMargin: 16
            Layout.rightMargin: 16
            visible: root.swapError.length > 0
            text: root.swapError
            color: "#F08A76"
            font.pixelSize: 12
            wrapMode: Text.WordWrap
        }

        SwapSummary {
            Layout.fillWidth: true
            Layout.topMargin: 12
            Layout.leftMargin: 16
            Layout.rightMargin: 16
            theme: root.theme
            visible: root.tokensSelected && root.hasAmount
            swapModeText: root.swapModeText
            feeText: swapState.formatTokenAmount(root.feeAmount, root.sellToken ? root.sellToken.symbol : "")
            priceImpactText: swapState.formatPercent(root.priceImpactPercent)
            priceImpactPercent: root.priceImpactPercent
            boundLabel: root.boundLabel
            boundText: root.boundSymbol ? (root.boundRaw + " " + root.boundSymbol) : root.boundRaw
        }

        SlippageToleranceControl {
            Layout.fillWidth: true
            Layout.topMargin: 12
            Layout.leftMargin: 16
            Layout.rightMargin: 16
            tolerancePercent: root.slippageTolerancePercent
            visible: root.tokensSelected && root.hasAmount

            onToleranceChangeRequested: function(tolerancePercent) {
                root.slippageTolerancePercent = swapState.clampSlippagePercent(tolerancePercent);
            }
        }

        Rectangle {
            id: ctaBox
            objectName: "swapSubmitButton"
            Layout.fillWidth: true
            Layout.topMargin: 8
            Layout.bottomMargin: 8
            Layout.leftMargin: 8
            Layout.rightMargin: 8
            Layout.preferredHeight: 56
            radius: 20
            color: !root.canSubmit ? theme.colors.panelBg
                                   : ctaHover.containsMouse ? theme.colors.ctaHoverBg
                                                            : theme.colors.ctaBg
            Behavior on color { ColorAnimation { duration: 120 } }

            Text {
                anchors.centerIn: parent
                text: root.submitButtonText
                color: root.canSubmit ? "#ffffff" : theme.colors.textSecondary
                font.pixelSize: 17
                font.weight: Font.Medium
            }

            MouseArea {
                id: ctaHover
                anchors.fill: parent
                hoverEnabled: true
                enabled: root.canSubmit
                cursorShape: root.canSubmit ? Qt.PointingHandCursor : Qt.ArrowCursor
                onClicked: {
                    if (root.canSubmit) root.submitRequested(root.buildSnapshot())
                }
            }
        }
    }
}
